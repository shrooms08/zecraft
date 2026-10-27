//! End-to-end, against the raffle: a recorded (synthetic) chain with ticket
//! purchases flows through ingestion, the timing rule, beacon verification
//! and settlement without any network, and the result is reproducible,
//! verifiable and private.

use zecraft_core::draw::RoundStatus;
use zecraft_core::rules::{PayoutKind, RefundReason};
use zecraft_engine::api::{config_response, proof_response, round_response};
use zecraft_engine::chain::ingest;
use zecraft_engine::config::SETTLE_CONFIRMATIONS;
use zecraft_engine::db::{EntryKind, Pool};
use zecraft_engine::decrypt::EntryScanner;
use zecraft_engine::processor::{Event, Processor};
use zecraft_engine::proof;
use zecraft_engine::store::Store;
use zecraft_engine::synthetic::{SyntheticChain, TestVault};
use zecraft_raffle::{Raffle, RaffleOutcome, Tickets, FORMAT};

mod common;
use common::{
    binary, config, fixture_beacons, scenario, start_time, Scenario, FIXTURE_ROUND, GENESIS,
};

const RECEIVED: u64 = 300_000 + 200_000 + 100_000 + 100_000 + 150_000 + 8_000 + 150_000;

#[tokio::test]
async fn round_is_ingested_settled_served_and_reproducible() {
    let Scenario { vault, mut chain } = scenario(0);
    let config = config(&vault);
    let scanner = EntryScanner::new(config.incoming_viewing_key().expect("ivk"));
    let store = Store::open_in_memory().expect("db");
    let mut processor = Processor::new(Raffle, config.clone());
    let mut beacons = fixture_beacons();

    let report = ingest(&mut chain, &store, &scanner, &config.chain)
        .await
        .expect("ingest");
    assert_eq!(
        report.new_notes, 7,
        "payments to the external address, never change"
    );

    // Before the beacon is published nothing settles.
    let beacon_time = start_time(&config.chain) + 600 + 900;
    let events = processor
        .process(&store, &mut beacons, beacon_time - 1)
        .await
        .expect("process");
    assert!(events.is_empty());
    let pending = processor
        .pending_rounds(&store.reader().expect("reader"))
        .expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].drand_round, FIXTURE_ROUND);
    assert_eq!(pending[0].beacon_time, beacon_time);
    assert_eq!(pending[0].status, RoundStatus::Revealable);

    // Once published, the round settles.
    let events = processor
        .process(&store, &mut beacons, beacon_time + 5)
        .await
        .expect("process");
    assert!(matches!(
        events[0],
        Event::Reveal {
            round: 0,
            drand_round: FIXTURE_ROUND,
            ..
        }
    ));
    assert!(matches!(
        events[1],
        Event::Settled {
            round: 0,
            voided: false,
            ..
        }
    ));
    let row = store
        .read(|db| db.settlement(0))
        .expect("row")
        .expect("settled");
    assert!(!row.voided);
    let s = &row.settlement;
    let outcome: RaffleOutcome = serde_json::from_value(s.outcome.clone()).expect("outcome");
    assert_eq!(outcome.tickets_sold, 6);
    let ticket = outcome.winning_ticket.expect("drawn");
    assert!(ticket < 6);
    let winner = outcome.winner.as_ref().expect("winner");
    // Draw order is (height, txid, action): player 1 owns 0..3, player 2
    // 3..5, player 3 ticket 5.
    let (first, tickets) = match ticket {
        0..=2 => (0, 3),
        3 | 4 => (3, 2),
        _ => (5, 1),
    };
    assert_eq!((winner.first_ticket, winner.tickets), (first, tickets));
    assert!(outcome.winner_paid);

    assert_eq!(s.received, RECEIVED);
    assert_eq!(s.carry_in, 0);
    let wins: Vec<_> = s
        .payouts
        .iter()
        .filter(|p| p.kind == PayoutKind::Win)
        .collect();
    assert_eq!(wins.len(), 1);
    assert_eq!(
        wins[0].gross,
        600_000 - 3 * 5_000,
        "the pot minus three entry fees"
    );
    assert_eq!(wins[0].net, 600_000 - 3 * 5_000 - 5_000);
    let refund = |reason: RefundReason| {
        s.payouts
            .iter()
            .find(|p| {
                p.kind
                    == PayoutKind::Refund {
                        reason: reason.clone(),
                    }
            })
            .map(|p| p.net)
    };
    assert_eq!(
        refund(RefundReason::WrongRound { memo_round: 7 }),
        Some(90_000)
    );
    assert_eq!(
        refund(RefundReason::WrongValue { expected: 200_000 }),
        Some(140_000)
    );
    assert_eq!(s.payouts.len(), 3);
    assert_eq!(
        s.payout_inputs.len(),
        5,
        "three entries and two refunds are spent"
    );
    assert_eq!(s.fees, 5 * 5_000 + 3 * 5_000);
    assert_eq!(s.carry_out, 8_000 + 150_000, "the kept notes carry over");
    assert_eq!(s.received + s.carry_in, s.paid + s.fees + s.carry_out);
    let inputs = processor.payout_inputs(&row);
    assert!(inputs.windows(2).all(|w| w[0] < w[1]), "canonical order");

    // Entries are stored generically: payload bytes and sealed receivers.
    let entries = store.read(|db| db.entries(0)).expect("entries");
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.kind == EntryKind::Valid)
            .count(),
        3
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.kind == EntryKind::Refund)
            .count(),
        2
    );
    assert!(entries
        .iter()
        .all(|e| e.payload.len() == 2 && e.sealed_receiver.len() == 92));
    assert!(entries
        .iter()
        .any(|e| e.payload == Tickets(3).0.to_be_bytes()));

    // API views.
    let config_view = config_response::<Raffle>(&config, true);
    assert_eq!(config_view.chain.genesis_height, GENESIS);
    assert_eq!(config_view.memo.magic, "ZRFL");
    assert_eq!(config_view.memo.text_prefix, "zrfl1:");
    assert_eq!(
        config_view.vault_address.as_deref(),
        Some(config.vault_address.as_str())
    );
    // With entries off, everything a payment needs is withheld.
    let closed = config_response::<Raffle>(&config, false);
    assert!(!closed.entries_open);
    assert!(
        closed.vault_address.is_none()
            && closed.sealing_public_key.is_none()
            && closed.uivk.is_none()
    );
    let json = serde_json::to_string(&closed).expect("json");
    assert!(
        !json.contains(&config.vault_address),
        "the address never appears"
    );
    let round_view =
        round_response(&processor, &store.reader().expect("reader"), 0).expect("round view");
    assert_eq!(round_view.status, RoundStatus::Settled);
    assert_eq!((round_view.entries, round_view.refunds), (3, 2));
    assert_eq!(round_view.game["tickets_sold"], 6);
    let settled = round_view.settled.as_ref().expect("settled view");
    assert_eq!(settled.payouts, s.payouts);
    assert_eq!(settled.outcome, s.outcome);
    let board = processor
        .board(&store.reader().expect("reader"))
        .expect("board");
    assert_eq!(board.round, 1, "the tip is in round 1 now");
    assert_eq!(board.carry, s.carry_out);
    assert_eq!(board.game["game"], "raffle");

    // The proof bundle recomputes to the identical settlement.
    let bundle = proof_response(&processor, &store.reader().expect("reader"), 0)
        .expect("proof")
        .expect("settled");
    let recomputed = proof::recompute(&Raffle, &bundle).expect("recompute");
    assert_eq!(
        serde_json::to_string(&recomputed).expect("json"),
        serde_json::to_string(&bundle.settlement).expect("json")
    );
    assert!(
        proof_response(&processor, &store.reader().expect("reader"), 1)
            .expect("proof")
            .is_none()
    );
    // A tampered bundle does not recompute to its settlement.
    let mut forged = bundle.clone();
    forged.carry_in += 1;
    forged.settlement.carry_in += 1;
    assert!(proof::recompute(&Raffle, &forged).map_or(true, |s| s != forged.settlement));

    // Privacy: no API output contains an opened payout receiver (the players'
    // receivers are [player; 43]); the sealed bytes are there instead.
    let mut all_json = String::new();
    for text in [
        serde_json::to_string(&config_view).expect("json"),
        serde_json::to_string(&round_view).expect("json"),
        serde_json::to_string(&board).expect("json"),
        serde_json::to_string(&bundle).expect("json"),
        serde_json::to_string(&events).expect("json"),
    ] {
        all_json.push_str(&text);
    }
    for player in 1u8..=6 {
        assert!(
            !all_json.contains(&hex::encode([player; 43])),
            "opened receiver of player {player} leaked"
        );
    }
    assert!(all_json.contains(&hex::encode(s.payouts[0].sealed.as_bytes())));
    // The engine never needed the sealing private key; a holder of it can open.
    let opened = vault
        .entry_memo(&FORMAT, 0, Tickets(3), 1)
        .open(
            &FORMAT,
            zecraft_core::params::Network::Test,
            &vault.sealing_private,
        )
        .expect("open");
    assert_eq!(opened.orchard, [1u8; 43]);

    // Idempotent restart: a fresh processor over the same database adds nothing,
    // and a fresh database over the same chain reproduces the state byte for byte.
    let dump = store.read(|db| db.dump()).expect("dump");
    let mut restarted = Processor::new(Raffle, config.clone());
    let events = restarted
        .process(&store, &mut beacons, beacon_time + 1_000)
        .await
        .expect("process");
    assert!(events.is_empty());
    let again = ingest(&mut chain, &store, &scanner, &config.chain)
        .await
        .expect("ingest");
    assert_eq!(again.new_blocks, 0);
    assert_eq!(store.read(|db| db.dump()).expect("dump"), dump);

    let fresh = Store::open_in_memory().expect("db");
    ingest(&mut chain, &fresh, &scanner, &config.chain)
        .await
        .expect("ingest");
    let mut fresh_processor = Processor::new(Raffle, config.clone());
    fresh_processor
        .process(&fresh, &mut fixture_beacons(), beacon_time + 5)
        .await
        .expect("process");
    assert_eq!(fresh.read(|db| db.dump()).expect("dump"), dump);
}

#[tokio::test]
async fn initial_carry_is_round_zero_carry_in_and_goes_to_the_winner() {
    let Scenario { vault, mut chain } = scenario(0);
    let mut config = config(&vault);
    config.initial_carry_zat = 777_000;
    let scanner = EntryScanner::new(config.incoming_viewing_key().expect("ivk"));
    let store = Store::open_in_memory().expect("db");
    let mut processor = Processor::new(Raffle, config.clone());
    assert_eq!(
        processor
            .carry(&store.reader().expect("reader"))
            .expect("carry"),
        777_000
    );
    assert_eq!(
        config_response::<Raffle>(&config, true).initial_carry_zat,
        777_000
    );
    ingest(&mut chain, &store, &scanner, &config.chain)
        .await
        .expect("ingest");
    let beacon_time = start_time(&config.chain) + 600 + 900;
    processor
        .process(&store, &mut fixture_beacons(), beacon_time + 5)
        .await
        .expect("process");
    let row = store
        .read(|db| db.settlement(0))
        .expect("row")
        .expect("settled");
    assert_eq!(row.settlement.carry_in, 777_000);
    let win = row
        .settlement
        .payouts
        .iter()
        .find(|p| p.kind == PayoutKind::Win)
        .expect("win");
    assert_eq!(win.gross, 585_000 + 777_000, "the carry goes to the winner");
    assert_eq!(row.settlement.carry_out, 158_000);
    let bundle = proof_response(&processor, &store.reader().expect("reader"), 0)
        .expect("proof")
        .expect("settled");
    assert_eq!(
        proof::recompute(&Raffle, &bundle).expect("recompute"),
        bundle.settlement
    );
}

#[tokio::test]
async fn late_window_is_voided_and_refunded() {
    // The window closes 20 minutes late: the post-close median lands after
    // the beacon, so the timing rule voids the round.
    let Scenario { vault, mut chain } = scenario(1_200);
    let config = config(&vault);
    let scanner = EntryScanner::new(config.incoming_viewing_key().expect("ivk"));
    let store = Store::open_in_memory().expect("db");
    let mut processor = Processor::new(Raffle, config.clone());
    ingest(&mut chain, &store, &scanner, &config.chain)
        .await
        .expect("ingest");
    let (_, status, _) = processor
        .status(&store.reader().expect("reader"), 0)
        .expect("status");
    assert_eq!(status, RoundStatus::Voided);

    let events = processor
        .process(&store, &mut fixture_beacons(), 0)
        .await
        .expect("process");
    assert!(matches!(
        events[0],
        Event::Settled {
            round: 0,
            voided: true,
            ..
        }
    ));
    let row = store
        .read(|db| db.settlement(0))
        .expect("row")
        .expect("settled");
    let s = &row.settlement;
    assert!(row.voided && s.voided);
    assert!(s
        .payouts
        .iter()
        .all(|p| matches!(p.kind, PayoutKind::Refund { .. })));
    assert_eq!(
        s.payouts.len(),
        5,
        "every decodable entry that can pay its fees"
    );
    assert_eq!(s.paid, 290_000 + 190_000 + 90_000 + 90_000 + 140_000);
    assert_eq!(s.carry_out, 158_000, "kept notes still carry over");
    let bundle = proof_response(&processor, &store.reader().expect("reader"), 0)
        .expect("proof")
        .expect("settled");
    assert!(bundle.beacon.is_none());
    assert_eq!(
        proof::recompute(&Raffle, &bundle).expect("recompute"),
        bundle.settlement
    );
}

/// The HTTP layer serves the same views, and 404s unsettled proofs.
#[tokio::test]
async fn http_api_serves_the_round() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tokio::sync::broadcast;
    use tower::ServiceExt;
    use zecraft_engine::api::{router, AppState, HealthResponse, PendingResponse, RoundResponse};
    use zecraft_engine::proof::ProofBundle;

    let Scenario { vault, mut chain } = scenario(0);
    let config = config(&vault);
    let scanner = EntryScanner::new(config.incoming_viewing_key().expect("ivk"));
    let store = Store::open_in_memory().expect("db");
    let mut processor = Processor::new(Raffle, config.clone());
    ingest(&mut chain, &store, &scanner, &config.chain)
        .await
        .expect("ingest");
    let beacon_time = start_time(&config.chain) + 600 + 900;
    processor
        .process(&store, &mut fixture_beacons(), beacon_time + 5)
        .await
        .expect("process");
    let (events, _) = broadcast::channel(8);
    let state = Arc::new(AppState {
        processor,
        store: Arc::new(store),
        events,
        started: std::time::Instant::now(),
        ledger: None,
        chain_servers: None,
        entries_open: true,
        stream_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(
            zecraft_engine::api::MAX_STREAMS,
        )),
        relay: None,
        relay_upstream: None,
        web_dir: None,
        proof_limiter: zecraft_engine::relay::RateLimiter::new(10),
        keyholder_ids: vec![1, 2, 3],
        threshold: 2,
    });
    let web = tempfile::tempdir().expect("web dir");
    std::fs::write(web.path().join("index.html"), "<title>Zecraft</title>").expect("index");
    std::fs::write(web.path().join("app.js"), "console.log(1)").expect("asset");
    let state = Arc::new(AppState {
        web_dir: Some(web.path().to_path_buf()),
        ..Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique"))
    });
    let app = router(state, &["http://localhost:7173".to_owned()]);

    async fn get(app: &axum::Router, path: &str) -> (StatusCode, Vec<u8>) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (status, body)
    }

    // The page: files, and index.html for any other path; the API still wins.
    let (status, body) = get(&app, "/").await;
    assert_eq!(
        (status, String::from_utf8_lossy(&body).contains("Zecraft")),
        (StatusCode::OK, true)
    );
    let (status, body) = get(&app, "/app.js").await;
    assert_eq!((status, body), (StatusCode::OK, b"console.log(1)".to_vec()));
    let (status, body) = get(&app, "/rounds/0").await;
    assert_eq!(
        (status, String::from_utf8_lossy(&body).contains("Zecraft")),
        (StatusCode::OK, true)
    );

    let (status, body) = get(&app, "/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    let health: HealthResponse = serde_json::from_slice(&body).expect("json");
    assert_eq!(health.settled_rounds, 1);

    let (status, body) = get(&app, "/v1/status").await;
    assert_eq!(status, StatusCode::OK);
    let view: zecraft_engine::api::StatusResponse = serde_json::from_slice(&body).expect("json");
    assert_eq!(view.engine.last_settled_round, Some(0));
    assert_eq!(view.engine.stored_tip, health.tip_height);
    assert_eq!(
        view.keyholders.len(),
        3,
        "configured keyholders listed even before a probe"
    );
    assert!(view.entries_open);

    let (status, body) = get(&app, "/v1/rounds/0").await;
    assert_eq!(status, StatusCode::OK);
    let round: RoundResponse = serde_json::from_slice(&body).expect("json");
    assert_eq!(round.status, RoundStatus::Settled);
    assert!(round.settled.is_some());

    let (status, body) = get(&app, "/v1/rounds/0/proof").await;
    assert_eq!(status, StatusCode::OK);
    let bundle: ProofBundle<zecraft_raffle::RaffleParams> =
        serde_json::from_slice(&body).expect("json");
    assert_eq!(
        proof::recompute(&Raffle, &bundle).expect("recompute"),
        bundle.settlement
    );
    for player in 1u8..=6 {
        assert!(!String::from_utf8_lossy(&body).contains(&hex::encode([player; 43])));
    }
    let (status, _) = get(&app, "/v1/rounds/1/proof").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = get(&app, "/v1/rounds/pending").await;
    assert_eq!(status, StatusCode::OK);
    let pending: PendingResponse = serde_json::from_slice(&body).expect("json");
    assert!(pending.rounds.is_empty());

    let (status, body) = get(&app, "/v1/board").await;
    assert_eq!(status, StatusCode::OK);
    let board: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        board["game"]["game"], "raffle",
        "the game's board is served as-is"
    );
    assert!(board["blocks_remaining"].is_u64() && board["round"].is_u64());
    let (status, body) = get(&app, "/v1/config").await;
    assert_eq!(status, StatusCode::OK);
    let cfg: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(cfg["game"]["ticket_price_zat"], 100_000);

    let (status, body) = get(&app, "/v1/rounds").await;
    assert_eq!(status, StatusCode::OK);
    let rounds: zecraft_engine::api::RoundsResponse = serde_json::from_slice(&body).expect("json");
    assert_eq!(rounds.rounds.len(), 1, "round 0 had entries");
    assert_eq!(rounds.rounds[0].status, "paying");
    assert_eq!((rounds.rounds[0].wins, rounds.rounds[0].refunds), (1, 2));
    assert_eq!(rounds.rounds[0].game["tickets_sold"], 6);
    assert_eq!(rounds.rounds[0].drand_round, FIXTURE_ROUND);
    let (status, body) = get(&app, "/v1/rounds/0/digests").await;
    assert_eq!(status, StatusCode::OK);
    let digests: zecraft_engine::api::DigestsResponse =
        serde_json::from_slice(&body).expect("json");
    assert!(digests.notes.iter().all(|n| n.memo_sha256.len() == 64));
    let (status, body) = get(&app, "/v1/keyholders/status").await;
    assert_eq!(status, StatusCode::OK);
    let keys: zecraft_engine::api::KeyholdersResponse =
        serde_json::from_slice(&body).expect("json");
    assert_eq!(keys.threshold, 2);
    assert_eq!(keys.keyholders[0].label, "Keyholder A");
}

/// A round with no valid tickets carries everything over, and the next
/// round's winner receives it.
#[tokio::test]
async fn an_empty_round_carries_to_the_next_winner() {
    let vault = TestVault::new(79);
    let config = config(&vault);
    let params = config.chain.clone();
    let mut chain = SyntheticChain::new(&vault, GENESIS, start_time(&params));
    chain.push_block(vec![chain.payment(150_000, Pool::Ironwood, [0xF6; 512])]); // kept
    while chain.next_height() < GENESIS + params.blocks_per_round {
        chain.advance(1);
    }
    let round1 = GENESIS + params.blocks_per_round;
    chain.push_block(vec![chain.payment(
        200_000,
        Pool::Ironwood,
        binary(&vault, 1, 2, 1),
    )]);
    while chain.next_height() < round1 + params.blocks_per_round {
        chain.advance(1);
    }
    chain.advance(params.mtp_window + SETTLE_CONFIRMATIONS);
    let scanner = EntryScanner::new(config.incoming_viewing_key().expect("ivk"));
    let store = Store::open_in_memory().expect("db");
    let mut processor = Processor::new(Raffle, config.clone());
    ingest(&mut chain, &store, &scanner, &config.chain)
        .await
        .expect("ingest");
    // Round 0 settles with the fixture beacon; round 1's beacon is not
    // recorded, so it waits.
    let beacon_time = start_time(&params) + 600 + 900;
    processor
        .process(&store, &mut fixture_beacons(), beacon_time + 5)
        .await
        .expect("process");
    let r0 = store
        .read(|db| db.settlement(0))
        .expect("row")
        .expect("settled");
    assert!(r0.settlement.payouts.is_empty());
    assert_eq!(
        r0.settlement.carry_out, 150_000,
        "no tickets: everything carries"
    );
    assert_eq!(
        processor
            .carry(&store.reader().expect("reader"))
            .expect("carry"),
        150_000
    );
    assert!(store.read(|db| db.settlement(1)).expect("row").is_none());
}

/// The relay serves tip, length-prefixed compact blocks, transactions and
/// broadcasts over HTTP, caps the block range and the body size, and rate
/// limits.
#[tokio::test]
async fn relay_serves_chain_data_with_limits() {
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use prost::Message;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use zcash_client_backend::proto::compact_formats::CompactBlock;
    use zcash_client_backend::proto::service::{RawTransaction, TreeState};
    use zecraft_engine::chain::ChainSource;
    use zecraft_engine::relay::{
        self, RelayBackend, RelayError, RelayState, RelayStatsResponse, TipResponse,
        MAX_BLOCK_RANGE, MAX_TX_BYTES,
    };

    struct Synthetic {
        chain: Mutex<SyntheticChain>,
        sent: Mutex<Vec<Vec<u8>>>,
    }

    #[async_trait]
    impl RelayBackend for Synthetic {
        async fn tip(&self) -> Result<(u32, String), RelayError> {
            let mut chain = self.chain.lock().await;
            let height = chain
                .latest_height()
                .await
                .map_err(|e| RelayError::Upstream(e.to_string()))?;
            let hash = chain
                .block(height)
                .map(|b| b.hash.clone())
                .unwrap_or_default();
            Ok((height, hex::encode(hash)))
        }
        async fn blocks(&self, start: u32, end: u32) -> Result<Vec<CompactBlock>, RelayError> {
            self.chain
                .lock()
                .await
                .block_range(start, end)
                .await
                .map_err(|e| RelayError::Upstream(e.to_string()))
        }
        async fn tree_state(&self, height: u32) -> Result<TreeState, RelayError> {
            Ok(TreeState {
                height: u64::from(height),
                ..TreeState::default()
            })
        }
        async fn broadcast(&self, raw: Vec<u8>) -> Result<(), RelayError> {
            if raw == b"bad" {
                return Err(RelayError::Rejected("bad-txns".into()));
            }
            self.sent.lock().await.push(raw);
            Ok(())
        }
        async fn transaction(&self, txid: [u8; 32]) -> Result<RawTransaction, RelayError> {
            self.chain
                .lock()
                .await
                .transaction(&txid[..])
                .map(|data| RawTransaction { data, height: 7 })
                .ok_or(RelayError::NotFound)
        }
    }

    let vault = TestVault::new(5);
    let mut chain = SyntheticChain::new(&vault, 100, 1_700_000_000);
    chain.advance(3);
    chain.push_block(vec![chain.payment(100_000, Pool::Ironwood, [0xF6; 512])]); // 103
    chain.advance(2);
    let txid_internal: [u8; 32] = chain.block(103).expect("block").vtx[0]
        .txid
        .clone()
        .try_into()
        .expect("32 bytes");
    let mut display = txid_internal;
    display.reverse();
    let backend = Arc::new(Synthetic {
        chain: Mutex::new(chain),
        sent: Mutex::new(Vec::new()),
    });
    struct Shared(Arc<Synthetic>);
    #[async_trait]
    impl RelayBackend for Shared {
        async fn tip(&self) -> Result<(u32, String), RelayError> {
            self.0.tip().await
        }
        async fn blocks(&self, s: u32, e: u32) -> Result<Vec<CompactBlock>, RelayError> {
            self.0.blocks(s, e).await
        }
        async fn tree_state(&self, h: u32) -> Result<TreeState, RelayError> {
            self.0.tree_state(h).await
        }
        async fn broadcast(&self, raw: Vec<u8>) -> Result<(), RelayError> {
            self.0.broadcast(raw).await
        }
        async fn transaction(&self, t: [u8; 32]) -> Result<RawTransaction, RelayError> {
            self.0.transaction(t).await
        }
    }
    let app = relay::router(Arc::new(RelayState::new(
        Box::new(Shared(backend.clone())),
        12,
    )));

    async fn call(app: &axum::Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (status, body)
    }
    let get = |path: &str| {
        Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("request")
    };
    let post = |body: Vec<u8>| {
        Request::builder()
            .method("POST")
            .uri("/v1/tx/broadcast")
            .body(Body::from(body))
            .expect("request")
    };

    let (status, body) = call(&app, get("/v1/chain/tip")).await;
    assert_eq!(status, StatusCode::OK);
    let tip: TipResponse = serde_json::from_slice(&body).expect("json");
    assert_eq!(tip.height, 105);

    let (status, body) = call(&app, get("/v1/chain/blocks?start=102&end=104")).await;
    assert_eq!(status, StatusCode::OK);
    let mut rest = body.as_slice();
    let mut heights = Vec::new();
    let mut outputs = 0;
    while !rest.is_empty() {
        let len = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let block = CompactBlock::decode(&rest[4..4 + len]).expect("decode");
        heights.push(block.height);
        outputs += block
            .vtx
            .iter()
            .map(|t| t.ironwood_actions.len())
            .sum::<usize>();
        rest = &rest[4 + len..];
    }
    assert_eq!(heights, vec![102, 103, 104]);
    assert!(
        outputs >= 1,
        "all compact outputs are relayed, not a filtered view"
    );

    let too_many = format!("/v1/chain/blocks?start=0&end={MAX_BLOCK_RANGE}");
    assert_eq!(call(&app, get(&too_many)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        call(&app, get("/v1/chain/blocks?start=5&end=4")).await.0,
        StatusCode::BAD_REQUEST
    );

    let (status, body) = call(&app, get("/v1/chain/tree-state?height=103")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        TreeState::decode(body.as_slice()).expect("decode").height,
        103
    );

    let (status, body) = call(&app, get(&format!("/v1/tx/{}", hex::encode(display)))).await;
    assert_eq!(status, StatusCode::OK);
    let raw = RawTransaction::decode(body.as_slice()).expect("decode");
    assert_eq!(raw.height, 7);
    assert!(!raw.data.is_empty());
    assert_eq!(
        call(&app, get(&format!("/v1/tx/{}", "00".repeat(32))))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, get("/v1/tx/nothex")).await.0,
        StatusCode::BAD_REQUEST
    );

    assert_eq!(call(&app, post(vec![1, 2, 3])).await.0, StatusCode::OK);
    assert_eq!(backend.sent.lock().await.as_slice(), &[vec![1, 2, 3]]);
    assert_eq!(
        call(&app, post(b"bad".to_vec())).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, post(vec![0; MAX_TX_BYTES + 1])).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );

    // 12 requests per minute: the bucket is empty by now.
    let mut limited = 0;
    for _ in 0..6 {
        if call(&app, get("/v1/chain/tip")).await.0 == StatusCode::TOO_MANY_REQUESTS {
            limited += 1;
        }
    }
    assert!(limited >= 4, "rate limit engaged ({limited})");
    let stats_app = relay::router(Arc::new(RelayState::new(Box::new(Shared(backend)), 600)));
    let (_, body) = call(&stats_app, get("/v1/relay/stats")).await;
    let stats: RelayStatsResponse = serde_json::from_slice(&body).expect("json");
    assert_eq!(stats.rate_per_minute, 600);
    assert_eq!(stats.max_block_range, MAX_BLOCK_RANGE);
}
