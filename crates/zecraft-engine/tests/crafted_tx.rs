//! A crafted payment to the vault never halts the chain follower (audit
//! 2026-10-28-b, H1; Zeek's R1).
//!
//! Compact blocks carry only the first 52 bytes of an output's ciphertext:
//! enough to detect a payment and read its value, not its memo or its
//! authentication tag. Consensus does not check that a non-coinbase output
//! decrypts, so anyone can pay the public vault address an output that is
//! valid for those 52 bytes and corrupt after them. The engine and every
//! keyholder (they run the same `chain::ingest`) failed on that block on
//! every pass, so nothing settled or paid again. Such an output is not a
//! game note: it is skipped. A real entry is never skipped: a source that
//! serves other bytes than the block describes is an error.

mod common;

use std::collections::BTreeMap;

use async_trait::async_trait;
use zcash_client_backend::proto::compact_formats::CompactBlock;
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::BranchId;
use zecraft_engine::api::proof_response;
use zecraft_engine::chain::{ingest, ChainSource};
use zecraft_engine::db::Pool;
use zecraft_engine::decrypt::EntryScanner;
use zecraft_engine::processor::Processor;
use zecraft_engine::store::Store;
use zecraft_engine::synthetic::{SyntheticChain, TestVault};
use zecraft_engine::EngineError;
use zecraft_raffle::{Raffle, Tickets, FORMAT};
use zecraft_verify::chain::{Blocks, MinedTx};
use zecraft_verify::checks::{block_range, verify_round, PublicConfig, RoundInput};
use zecraft_verify::{CheckId, CheckStatus};

use common::{config, fixture_beacons, scenario, scenario_with, start_time, Scenario, GENESIS};

/// Notes the shared round-0 scenario pays to the vault's external address.
const SCENARIO_NOTES: u64 = 7;

/// Where the corruption goes in the 580-byte ciphertext: the compact form
/// covers bytes 0..52, the memo follows, the last 16 bytes are the tag.
#[derive(Clone, Copy, Debug)]
enum Corrupt {
    FirstByteAfterCompact,
    Memo,
    Tag,
}

/// A payment to the vault carrying a valid round-0 entry (2 tickets) whose
/// ciphertexts are corrupted past the compact part, as anyone could mine it.
fn crafted(chain: &mut SyntheticChain, vault: &TestVault, at: Corrupt) -> Vec<u8> {
    let memo = vault.entry_memo(&FORMAT, 0, Tickets(2), 9).encode(&FORMAT);
    let spec = chain.payment(200_000, Pool::Ironwood, memo);
    corrupt_all(chain.unmined_transaction(&spec), at)
}

/// `raw` with every Ironwood action's ciphertext corrupted at `at` (one of
/// them pays the vault; the other is the builder's dummy).
fn corrupt_all(raw: Vec<u8>, at: Corrupt) -> Vec<u8> {
    let actions = Transaction::read(&raw[..], BranchId::Nu6_3)
        .expect("parse")
        .ironwood_bundle()
        .expect("an Ironwood bundle")
        .actions()
        .len();
    (0..actions).fold(raw, |raw, index| corrupt_one(raw, index, at))
}

/// `raw` with Ironwood action `index`'s ciphertext corrupted at `at`.
fn corrupt_one(mut raw: Vec<u8>, index: usize, at: Corrupt) -> Vec<u8> {
    let tx = Transaction::read(&raw[..], BranchId::Nu6_3).expect("parse");
    let ciphertext = tx.ironwood_bundle().expect("an Ironwood bundle").actions()[index]
        .encrypted_note()
        .enc_ciphertext
        .to_vec();
    let start = raw
        .windows(ciphertext.len())
        .position(|w| w == ciphertext.as_slice())
        .expect("the ciphertext is in the serialization");
    let offset = match at {
        Corrupt::FirstByteAfterCompact => 52,
        Corrupt::Memo => 300,
        Corrupt::Tag => ciphertext.len() - 1,
    };
    raw[start + offset] ^= 0x01;
    raw
}

/// Follows `source` and settles round 0, as the engine does and as each
/// keyholder does (its follower runs the same `chain::ingest` and
/// `Processor::process`), into a store of its own.
async fn follow_and_settle(
    source: &mut dyn ChainSource,
    vault: &TestVault,
) -> Result<(Store, Processor<Raffle>), EngineError> {
    let config = config(vault);
    let store = Store::open_in_memory()?;
    let scanner = EntryScanner::new(config.incoming_viewing_key()?);
    ingest(source, &store, &scanner, &config.chain).await?;
    let mut processor = Processor::new(Raffle, config.clone());
    let now = start_time(&config.chain) + 600 + 900 + 5;
    processor
        .process(&store, &mut fixture_beacons(), now)
        .await?;
    Ok((store, processor))
}

fn settlement_json(store: &Store) -> String {
    let row = store
        .read(|db| db.settlement(0))
        .expect("read")
        .expect("round 0 settled");
    serde_json::to_string(&row.settlement).expect("json")
}

/// Every transaction in blocks `start..=end` of `chain`, by display txid.
fn mined(chain: &SyntheticChain, start: u32, end: u32) -> BTreeMap<String, MinedTx> {
    let mut txs = BTreeMap::new();
    for height in start..=end {
        for tx in &chain.block(height).expect("block").vtx {
            let mut display = tx.txid.clone();
            display.reverse();
            txs.insert(
                hex::encode(display),
                MinedTx {
                    raw: chain.transaction(&tx.txid).expect("raw"),
                    height,
                },
            );
        }
    }
    txs
}

/// The verifier's Entries check (check 2) on round 0, with chain data
/// straight from `chain` and transactions from `txs`.
fn entries_check(
    store: &Store,
    processor: &Processor<Raffle>,
    vault: &TestVault,
    chain: &SyntheticChain,
    txs: &BTreeMap<String, MinedTx>,
) -> (CheckStatus, String) {
    let config = config(vault);
    let bundle = proof_response(processor, &store.reader().expect("reader"), 0)
        .expect("bundle")
        .expect("settled");
    let public = PublicConfig {
        chain: config.chain.clone(),
        game: config.game.clone(),
        vault_address: config.vault_address.clone(),
        uivk: config.uivk.clone(),
    };
    let (start, end) = block_range(&bundle);
    let blocks = Blocks::new((start..=end).map(|h| chain.block(h).expect("block").clone()));
    let report = verify_round(&RoundInput {
        rules: &Raffle,
        config: &public,
        bundle: &bundle,
        blocks: &blocks,
        txs,
        ivk: &public.ivk().expect("ivk"),
    });
    let check = report
        .checks
        .into_iter()
        .find(|c| c.id == CheckId::Entries)
        .expect("check 2 runs");
    (
        check.status,
        format!("{}: {:?}", check.summary, check.detail),
    )
}

#[tokio::test]
async fn a_crafted_payment_does_not_halt_the_follower() {
    for at in [Corrupt::FirstByteAfterCompact, Corrupt::Memo, Corrupt::Tag] {
        let Scenario { vault, mut chain } = scenario_with(0, |chain, vault| {
            let raw = crafted(chain, vault, at);
            chain.push_raw_transaction(raw).expect("mined");
        });
        let (store, _) = follow_and_settle(&mut chain, &vault)
            .await
            .unwrap_or_else(|e| panic!("{at:?}: the follower halted: {e}"));
        // The crafted output is not a game note; the real ones are all kept.
        assert_eq!(
            store.read(|db| db.note_count()).expect("count"),
            SCENARIO_NOTES,
            "{at:?}"
        );
    }
}

#[tokio::test]
async fn a_round_with_a_crafted_payment_settles_and_verifies_as_without_it() {
    // The baseline mines an ordinary transaction to someone else in the same
    // place, built the same way, so every later height and txid is identical.
    let Scenario {
        vault,
        chain: mut plain,
    } = scenario_with(0, |chain, vault| {
        let memo = vault.entry_memo(&FORMAT, 0, Tickets(2), 9).encode(&FORMAT);
        let spec = chain.payment_to(TestVault::new(7).address(), 200_000, Pool::Ironwood, memo);
        let raw = chain.unmined_transaction(&spec);
        chain.push_raw_transaction(raw).expect("mined");
    });
    let (plain_store, _) = follow_and_settle(&mut plain, &vault)
        .await
        .expect("the plain round settles");

    let Scenario { vault, mut chain } = scenario_with(0, |chain, vault| {
        let raw = crafted(chain, vault, Corrupt::Memo);
        chain.push_raw_transaction(raw).expect("mined");
    });
    let (store, processor) = follow_and_settle(&mut chain, &vault)
        .await
        .expect("the round with a crafted payment settles");
    assert_eq!(
        settlement_json(&store),
        settlement_json(&plain_store),
        "the crafted payment changes nothing in the settlement"
    );

    // The verifier skips it the same way: every real entry is found, none
    // is missing or made up.
    let bundle = proof_response(&processor, &store.reader().expect("reader"), 0)
        .expect("bundle")
        .expect("settled");
    let (start, end) = block_range(&bundle);
    let txs = mined(&chain, start, end);
    let (status, detail) = entries_check(&store, &processor, &vault, &chain, &txs);
    assert_eq!(status, CheckStatus::Pass, "{detail}");
}

/// A source that serves player 1's real entry with one memo byte flipped:
/// the same note commitments, so only the txid shows the bytes changed.
struct Altering {
    chain: SyntheticChain,
    target: Vec<u8>,
}

#[async_trait]
impl ChainSource for Altering {
    async fn latest_height(&mut self) -> Result<u32, EngineError> {
        self.chain.latest_height().await
    }
    async fn block_range(
        &mut self,
        start: u32,
        end: u32,
    ) -> Result<Vec<CompactBlock>, EngineError> {
        self.chain.block_range(start, end).await
    }
    async fn transaction(&mut self, txid: &[u8; 32]) -> Result<Vec<u8>, EngineError> {
        let raw = ChainSource::transaction(&mut self.chain, txid).await?;
        Ok(if txid[..] == self.target[..] {
            corrupt_all(raw, Corrupt::Memo)
        } else {
            raw
        })
    }
    async fn transaction_height(&mut self, txid: &[u8; 32]) -> Result<u32, EngineError> {
        self.chain.transaction_height(txid).await
    }
}

/// Player 1's entry: the first transaction paying the vault in round 0.
fn first_entry(chain: &SyntheticChain) -> Vec<u8> {
    (GENESIS..)
        .find_map(|h| {
            chain
                .block(h)
                .and_then(|b| b.vtx.first())
                .map(|t| t.txid.clone())
        })
        .expect("an entry")
}

#[tokio::test]
async fn a_source_altering_a_real_entry_is_an_error_not_a_skip() {
    let Scenario { vault, chain } = scenario(0);
    let target = first_entry(&chain);
    let mut source = Altering { chain, target };
    let outcome = follow_and_settle(&mut source, &vault).await;
    assert!(
        outcome.is_err(),
        "an altered real entry must stop ingestion (and fail over), never be dropped"
    );
}

#[tokio::test]
async fn the_verifier_fails_a_real_entry_served_with_altered_bytes() {
    let Scenario { vault, mut chain } = scenario(0);
    let (store, processor) = follow_and_settle(&mut chain, &vault)
        .await
        .expect("settles");
    let bundle = proof_response(&processor, &store.reader().expect("reader"), 0)
        .expect("bundle")
        .expect("settled");
    let (start, end) = block_range(&bundle);
    let mut txs = mined(&chain, start, end);
    let mut display = first_entry(&chain);
    display.reverse();
    let entry = txs
        .get_mut(&hex::encode(display))
        .expect("player 1's entry");
    entry.raw = corrupt_all(entry.raw.clone(), Corrupt::Memo);
    let (status, detail) = entries_check(&store, &processor, &vault, &chain, &txs);
    assert_eq!(status, CheckStatus::Fail, "{detail}");
    assert!(
        detail.contains("does not hash to its txid"),
        "fails because the bytes are not the transaction: {detail}"
    );
}
