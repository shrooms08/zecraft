//! A second game through the whole kit, unchanged: the coin flip.
//!
//! Entries are paid to a FROST vault on a synthetic chain; the generic
//! engine ingests them, draws with a real drand beacon and settles with the
//! coin flip's rules; a real payout PCZT spending the entry notes is
//! proven, approved by a keyholder's `verify_payout`, FROST-signed 2-of-3,
//! extracted and mined; its openings are recorded; and the independent
//! verifier runs the five checks on the chain data: all pass.

use std::collections::{BTreeMap, HashMap};

use orchard::circuit::{OrchardCircuitVersion, ProvingKey, VerifyingKey};
use orchard::keys::{FullViewingKey, Scope};
use orchard::note_encryption::IronwoodDomain;
use orchard::tree::{MerkleHashOrchard, MerklePath};
use orchard::{Anchor, Note};
use pczt::roles::creator::Creator;
use pczt::roles::io_finalizer::IoFinalizer;
use pczt::roles::tx_extractor::TransactionExtractor;
use rand_09::rngs::StdRng;
use rand_09::SeedableRng;
use rand_core::OsRng;
use shardtree::store::memory::MemoryShardStore;
use shardtree::ShardTree;
use zcash_note_encryption::try_note_decryption;
use zcash_primitives::transaction::builder::{BuildConfig, Builder, BundlePadding, PcztResult};
use zcash_primitives::transaction::fees::zip317;
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::{BranchId, TEST_NETWORK};
use zcash_protocol::memo::MemoBytes;
use zcash_protocol::value::Zatoshis;
use zecraft_coinflip::{CoinFlip, CoinFlipParams, FlipOutcome, Side, FORMAT};
use zecraft_core::draw::Beacon;
use zecraft_core::entry::{generate_sealing_keypair, EntryMemo, PayoutReceiver};
use zecraft_core::params::{ChainParams, Network};
use zecraft_core::proof::{PayoutRecord, PayoutState};
use zecraft_core::rules::PayoutKind;
use zecraft_engine::api::proof_response;
use zecraft_engine::beacon::FixedBeacons;
use zecraft_engine::chain::ingest;
use zecraft_engine::config::SETTLE_CONFIRMATIONS;
use zecraft_engine::coordinator::{payable_receiver, payout_openings};
use zecraft_engine::db::Pool;
use zecraft_engine::decrypt::EntryScanner;
use zecraft_engine::processor::Processor;
use zecraft_engine::store::Store;
use zecraft_engine::synthetic::{SyntheticChain, TestVault};
use zecraft_keyholder::db::NodeDb;
use zecraft_keyholder::verify::{verify_payout, VerifyContext};
use zecraft_vault::{frost, keys, send};
use zecraft_verify::chain::{Blocks, MinedTx};
use zecraft_verify::checks::{verify_round, PublicConfig, RoundInput};
use zecraft_verify::CheckStatus;

/// A post-NU7 testnet height, so every transaction is V6 on the NU7 branch.
const GENESIS: u32 = 4_500_000;
/// A real quicknet beacon (zecraft-core/tests/fixtures/quicknet.json).
const FIXTURE_ROUND: u64 = 10_000_000;
const FIXTURE_SIGNATURE: &str =
    "a187c9f5800521e986b5658924696682071ce7ae965486e8fd08bc2c4758bdbb9ed9ecb7beee392a38214e321ecd434f";

fn start_time(chain: &ChainParams) -> u64 {
    let beacon_time = chain.drand.genesis_time + (FIXTURE_ROUND - 1) * chain.drand.period_secs;
    beacon_time - chain.round_duration_secs().expect("duration") - chain.reveal_delay_secs
}

/// A player's real Orchard address (payouts must go somewhere spendable).
fn player(seed: u8) -> orchard::Address {
    let sk = (0u8..=255)
        .find_map(|n| {
            let mut bytes = [seed; 32];
            bytes[0] = n;
            Option::<orchard::keys::SpendingKey>::from(orchard::keys::SpendingKey::from_bytes(
                bytes,
            ))
        })
        .expect("a valid spending key");
    FullViewingKey::from(&sk as &orchard::keys::SpendingKey).address_at(0u32, Scope::External)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_coin_flip_round_runs_through_the_whole_kit() {
    // A 2-of-3 FROST vault: its Orchard full viewing key comes from the group key.
    let keygen = frost::dealer_keygen(2, 3, OsRng).expect("keygen");
    let group = frost::group_key_bytes(&keygen.public_key_package).expect("group key");
    let fvk = keys::derive_full_viewing_key_from_seed(&group, [7; 32]).expect("fvk");
    let (sealing_private, sealing_public) = generate_sealing_keypair(&mut StdRng::seed_from_u64(7));
    let vault = TestVault {
        ufvk: keys::unified_full_viewing_key(&fvk).expect("ufvk"),
        fvk: fvk.clone(),
        sealing_private,
        sealing_public,
    };
    let config = vault.config(GENESIS, CoinFlipParams::default());
    let params = config.chain.clone();
    let stake = config.game.stake_zat;

    // Four players: heads, tails, tails, heads; each seals its own address.
    let players: Vec<(orchard::Address, u8)> = vec![
        (player(1), 0),
        (player(2), 1),
        (player(3), 1),
        (player(4), 0),
    ];
    let mut chain = SyntheticChain::new(&vault, GENESIS - 3, start_time(&params) - 75);
    chain.advance(3);
    chain.push_block(vec![]); // GENESIS: the round's first block
    chain.advance(2);
    for (i, (address, side)) in players.iter().enumerate() {
        let receiver = PayoutReceiver {
            network: Network::Test,
            orchard: address.to_raw_address_bytes(),
        };
        let memo = EntryMemo::seal(
            &FORMAT,
            0,
            Side(*side),
            &receiver,
            &vault.sealing_public,
            &mut StdRng::seed_from_u64(i as u64),
        )
        .expect("seal");
        chain.push_block(vec![chain.payment(
            stake,
            Pool::Ironwood,
            memo.encode(&FORMAT),
        )]);
    }
    while chain.next_height() < GENESIS + params.blocks_per_round {
        chain.advance(1);
    }
    chain.advance(params.mtp_window + SETTLE_CONFIRMATIONS);

    // The engine: ingest, draw with the beacon, settle with the coin flip's rules.
    let store = Store::open_in_memory().expect("store");
    let scanner = EntryScanner::new(config.incoming_viewing_key().expect("ivk"));
    ingest(&mut chain, &store, &scanner, &params)
        .await
        .expect("ingest");
    let mut processor = Processor::new(CoinFlip, config.clone());
    let mut beacons = FixedBeacons {
        beacons: HashMap::from([(
            FIXTURE_ROUND,
            Beacon {
                round: FIXTURE_ROUND,
                signature: FIXTURE_SIGNATURE.to_owned(),
            },
        )]),
    };
    let beacon_time = start_time(&params) + 600 + 900;
    processor
        .process(&store, &mut beacons, beacon_time + 5)
        .await
        .expect("process");
    let row = store
        .read(|db| db.settlement(0))
        .expect("db")
        .expect("settled");
    let outcome: FlipOutcome =
        serde_json::from_value(row.settlement.outcome.clone()).expect("outcome");
    let side = outcome.side.expect("drawn");
    let winners: Vec<&orchard::Address> = players
        .iter()
        .filter(|(_, s)| *s == side)
        .map(|(a, _)| a)
        .collect();
    assert_eq!(
        (outcome.entries, outcome.winners as usize),
        (4, winners.len())
    );
    let share = (4 * (stake - 5_000)) / 2;
    assert_eq!(outcome.share_zat, share);
    assert_eq!(row.settlement.payouts.len(), 2);
    assert!(row
        .settlement
        .payouts
        .iter()
        .all(|p| p.kind == PayoutKind::Win));

    // The payout transaction: spend the four entry notes, pay both winners.
    let notes = store
        .read(|db| db.notes_in_range(GENESIS, GENESIS + params.blocks_per_round))
        .expect("notes");
    assert_eq!(notes.len(), 4);
    let ivk = fvk.to_ivk(Scope::External).prepare();
    let mut tree =
        ShardTree::<_, 32, 16>::new(MemoryShardStore::<MerkleHashOrchard, u32>::empty(), 100);
    let mut entry_notes: Vec<Note> = Vec::new();
    for row in &notes {
        let raw = chain.transaction(&row.txid).expect("raw");
        let tx = Transaction::read(&raw[..], BranchId::Nu6_3).expect("tx");
        let action =
            &tx.ironwood_bundle().expect("bundle").actions()[usize::from(row.action_index)];
        let (note, _, _) = try_note_decryption(&IronwoodDomain::for_action(action), &ivk, action)
            .expect("decrypt");
        let cmx: orchard::note::ExtractedNoteCommitment = note.commitment().into();
        assert_eq!(
            Some(cmx.to_bytes()),
            row.cmx,
            "the engine recorded the note's cmx"
        );
        tree.append(
            MerkleHashOrchard::from_cmx(&cmx),
            incrementalmerkletree::Retention::Marked,
        )
        .expect("append");
        entry_notes.push(note);
    }
    tree.checkpoint(GENESIS).expect("checkpoint");
    let mut paths: Vec<MerklePath> = Vec::new();
    let mut anchor = None;
    for (i, note) in entry_notes.iter().enumerate() {
        let path = tree
            .witness_at_checkpoint_depth((i as u64).into(), 0)
            .expect("witness")
            .expect("exists");
        let cmx: orchard::note::ExtractedNoteCommitment = note.commitment().into();
        anchor = Some(Anchor::from(path.root(MerkleHashOrchard::from_cmx(&cmx))));
        paths.push(path.into());
    }
    let tip = chain.next_height() - 1;
    let mut builder = Builder::new(
        TEST_NETWORK,
        (tip + 1).into(),
        BuildConfig::Standard {
            sapling_anchor: None,
            orchard_anchor: None,
            ironwood_anchor: anchor,
            orchard_padding: BundlePadding::DEFAULT,
            ironwood_padding: BundlePadding::DEFAULT,
        },
    );
    for (note, path) in entry_notes.iter().zip(&paths) {
        builder
            .add_ironwood_spend::<zip317::FeeError>(fvk.clone(), *note, path.clone())
            .expect("spend");
    }
    let mut paid = 0;
    for payout in &row.settlement.payouts {
        let address = payable_receiver(Network::Test, &FORMAT, &vault.sealing_private, payout)
            .expect("the vault opens the winner's sealed address");
        assert!(winners.contains(&&address), "a winner's own address");
        builder
            .add_ironwood_output::<zip317::FeeError>(
                Some(fvk.to_ovk(Scope::External)),
                address,
                Zatoshis::const_from_u64(payout.net),
                MemoBytes::from_bytes(b"Zecraft round 0: win").expect("memo"),
            )
            .expect("output");
        paid += payout.net;
    }
    let fee = 5_000 * 4; // max(4 spends, 3 outputs)
    builder
        .add_ironwood_output::<zip317::FeeError>(
            Some(fvk.to_ovk(Scope::Internal)),
            fvk.address_at(0u32, Scope::Internal),
            Zatoshis::const_from_u64(4 * stake - paid - fee),
            MemoBytes::empty(),
        )
        .expect("change");
    let PcztResult { pczt_parts, .. } = builder
        .build_for_pczt(OsRng, &zip317::FeeRule::standard())
        .expect("pczt");
    let pczt = IoFinalizer::new(Creator::build_from_parts(pczt_parts).expect("creator"))
        .finalize_io()
        .expect("io");
    let pczt =
        send::prove(pczt, &ProvingKey::build(OrchardCircuitVersion::PostNu6_3)).expect("prove");

    // A keyholder recomputes the round itself and approves exactly this payout.
    let tip_time = store.read(|db| db.tip()).expect("tip").expect("block").time;
    let node = NodeDb::open_in_memory().expect("node db");
    let reader = store.reader().expect("reader");
    let ctx = VerifyContext {
        now: tip_time + 1,
        processor: &processor,
        fvk: &fvk,
        sealing: &vault.sealing_private,
        chain: &reader,
        node: &node,
    };
    let approved =
        verify_payout(&ctx, &pczt.clone().serialize().expect("ser"), &[0]).expect("approved");
    assert_eq!(
        (
            approved.payouts,
            approved.charged_inputs,
            approved.float_inputs
        ),
        (2, 4, 0)
    );
    drop(reader);

    // FROST 2-of-3, extract, mine.
    let signers: Vec<&frost::KeyPackage> = keygen.key_packages.values().take(2).collect();
    let (pczt, signed) =
        send::sign_pczt(pczt, &signers, &keygen.public_key_package).expect("FROST signatures");
    assert_eq!(signed.len(), 4);
    let vk = VerifyingKey::build(OrchardCircuitVersion::PostNu6_3);
    let tx = TransactionExtractor::new(pczt)
        .with_orchard(&vk)
        .extract()
        .expect("a valid transaction");
    let mut raw = Vec::new();
    tx.write(&mut raw).expect("serialize");
    let txid = tx.txid().to_string();
    let mined_at = chain.push_raw_transaction(raw.clone()).expect("mined");
    ingest(&mut chain, &store, &scanner, &params)
        .await
        .expect("ingest");
    let openings = payout_openings(
        &params,
        &FORMAT,
        &fvk,
        &vault.sealing_private,
        &row.settlement,
        &txid,
        mined_at,
        &raw,
    )
    .expect("openings");
    assert_eq!(openings.outputs.len(), 2);
    store
        .write(|db| {
            db.insert_payout(&PayoutRecord {
                round: 0,
                state: PayoutState::Broadcast,
                session: "00".repeat(32),
                txid: Some(txid.clone()),
                height: mined_at,
                fee,
                signers: vec![1, 2],
            })?;
            db.set_payout_openings(0, &openings)
        })
        .expect("record");

    // The independent verifier: chain data straight from the chain.
    let bundle = proof_response(&processor, &store.reader().expect("reader"), 0)
        .expect("bundle")
        .expect("settled");
    let public = PublicConfig {
        chain: params.clone(),
        game: config.game.clone(),
        vault_address: config.vault_address.clone(),
        uivk: config.uivk.clone(),
    };
    let (start, end) = zecraft_verify::checks::block_range(&bundle);
    let blocks = Blocks::new(
        (start..=end)
            .chain([mined_at])
            .map(|h| chain.block(h).expect("block").clone()),
    );
    let mut txs = BTreeMap::new();
    for row in &notes {
        txs.insert(
            row.txid_hex(),
            MinedTx {
                raw: chain.transaction(&row.txid).expect("raw"),
                height: row.height,
            },
        );
    }
    txs.insert(
        txid,
        MinedTx {
            raw,
            height: mined_at,
        },
    );
    let report = verify_round(&RoundInput {
        rules: &CoinFlip,
        config: &public,
        bundle: &bundle,
        blocks: &blocks,
        txs: &txs,
        ivk: &public.ivk().expect("ivk"),
    });
    for check in &report.checks {
        assert_eq!(
            check.status,
            CheckStatus::Pass,
            "{}: {} {:?}",
            check.title,
            check.summary,
            check.detail
        );
    }
    assert_eq!(report.checks.len(), 5);
}
