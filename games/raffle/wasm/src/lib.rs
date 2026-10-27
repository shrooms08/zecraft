//! The raffle in the browser, for `wasm32-unknown-unknown` on the stable
//! toolchain (single-threaded: no atomics, no rayon).
//!
//! * The verifier: the same five checks as `zecraft-raffle-verify round`,
//!   including the Halo2 proof and signatures of the payout transaction,
//!   in three steps so the page fetches chain data in between:
//!   [`verify_plan`] (block range), [`verify_scan`] (txids to fetch),
//!   [`verify_round`] (the report).
//! * [`build_entry`]: seals a fresh payout address into a `ZRFL` memo and
//!   builds the ZIP 321 request any wallet can pay.
//!
//! Everything goes in and out as JSON strings and byte arrays, so the
//! JavaScript side needs no generated types. Chain data comes from a
//! Zecraft relay: `/v1/chain/blocks` (u32 LE length-prefixed compact
//! blocks) and `/v1/tx/{txid}` (protobuf `RawTransaction`).

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use prost::Message;
use rand_chacha_09::rand_core::SeedableRng;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use zecraft_core::entry::{entry_request_uri, EntryMemo, PayoutReceiver};
use zecraft_core::params::Network;
use zecraft_core::proof::ProofBundle;
use zecraft_raffle::{Raffle, RaffleParams, Tickets, FORMAT};
use zecraft_verify::chain::{decode_relay_blocks, parse_tx, Blocks, MinedTx};
use zecraft_verify::checks::{self, PublicConfig, RoundInput};

type Bundle = ProofBundle<RaffleParams>;
type Config = PublicConfig<RaffleParams>;

fn err(message: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&message.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(json: &str, what: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|e| format!("{what} is not valid JSON for the raffle: {e}"))
}

fn json(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

/// The block range and payout height a round's checks need.
#[derive(Serialize)]
pub struct Plan {
    pub start: u32,
    pub end: u32,
    pub payout_height: Option<u32>,
}

/// Step 1: checks the bundle's window and returns the compact blocks to
/// fetch (`start..=end`, plus `payout_height` when outside it).
pub fn plan(bundle: &str) -> Result<Plan, String> {
    let bundle: Bundle = parse(bundle, "the proof bundle")?;
    checks::check_window(&bundle).map_err(|e| e.to_string())?;
    let (start, end) = checks::block_range(&bundle);
    Ok(Plan {
        start,
        end,
        payout_height: checks::payout_height(&bundle),
    })
}

/// Step 2: trial-decrypts the window with the public UIVK and returns the
/// display-hex txids whose full transactions step 3 needs.
pub fn scan(bundle: &str, config: &str, blocks: &[u8]) -> Result<Vec<String>, String> {
    let bundle: Bundle = parse(bundle, "the proof bundle")?;
    let config: Config = parse(config, "the game config")?;
    let ivk = config.ivk().map_err(|e| e.to_string())?;
    let blocks = Blocks::new(decode_relay_blocks(blocks).map_err(|e| e.to_string())?);
    let hits = checks::scan_window(&bundle, &ivk, &blocks);
    Ok(checks::transactions_to_fetch(&bundle, &hits))
}

/// One check, as the page shows it.
#[derive(Serialize)]
pub struct Check {
    pub name: String,
    pub pass: bool,
    pub status: zecraft_verify::CheckStatus,
    pub summary: String,
    pub detail: Vec<String>,
}

/// The five checks.
#[derive(Serialize)]
pub struct Report {
    pub round: u32,
    pub passed: bool,
    pub checks: Vec<Check>,
}

/// Step 3: the five checks. `blocks` holds every block of the plan (relay
/// encoding), `txs` the relay's `RawTransaction` bytes for the txids of
/// step 2.
pub fn verify(
    bundle: &str,
    config: &str,
    blocks: &[u8],
    txs: &[Vec<u8>],
) -> Result<Report, String> {
    let bundle: Bundle = parse(bundle, "the proof bundle")?;
    let config: Config = parse(config, "the game config")?;
    let ivk = config.ivk().map_err(|e| e.to_string())?;
    let blocks = Blocks::new(decode_relay_blocks(blocks).map_err(|e| e.to_string())?);
    let mut mined = BTreeMap::new();
    for bytes in txs {
        let raw = zcash_client_backend::proto::service::RawTransaction::decode(bytes.as_slice())
            .map_err(|e| format!("raw transaction: {e}"))?;
        let height = u32::try_from(raw.height).map_err(|_| "transaction height overflows")?;
        let tx = MinedTx {
            raw: raw.data,
            height,
        };
        let parsed = parse_tx(bundle.chain.network, "fetched", &tx).map_err(|e| e.to_string())?;
        mined.insert(parsed.txid().to_string(), tx);
    }
    let report = checks::verify_round(&RoundInput {
        rules: &Raffle,
        config: &config,
        bundle: &bundle,
        blocks: &blocks,
        txs: &mined,
        ivk: &ivk,
    });
    Ok(Report {
        round: report.round,
        passed: report.passed(),
        checks: report
            .checks
            .into_iter()
            .map(|c| Check {
                name: c.title,
                pass: matches!(
                    c.status,
                    zecraft_verify::CheckStatus::Pass | zecraft_verify::CheckStatus::Warn
                ),
                status: c.status,
                summary: c.summary,
                detail: c.detail,
            })
            .collect(),
    })
}

/// A ticket purchase, ready to pay.
#[derive(Serialize)]
pub struct Entry {
    pub uri: String,
    pub round: u32,
    pub tickets: u16,
    pub amount_zat: u64,
    /// The memo as sent (`zrfl1:...` text, or hex of the binary memo).
    pub memo: String,
}

/// The part of the engine's `/v1/config` an entry needs.
#[derive(serde::Deserialize)]
struct EntryConfig {
    chain: zecraft_core::params::ChainParams,
    game: RaffleParams,
    vault_address: Option<String>,
    sealing_public_key: Option<zecraft_core::entry::SealingPublicKey>,
}

/// Seals `payout_address` (a unified address with an Orchard receiver)
/// into a `ZRFL` memo for `tickets` tickets in `round`, and builds the
/// ZIP 321 request paying the vault. `seed` is 32 bytes of fresh randomness.
pub fn entry(
    config: &str,
    round: u32,
    tickets: u16,
    payout_address: &str,
    text_memo: bool,
    seed: [u8; 32],
) -> Result<Entry, String> {
    let config: EntryConfig = parse(config, "the game config")?;
    let network = config.chain.network;
    if network != Network::Test {
        return Err("this page plays on testnet only".into());
    }
    let (Some(vault), Some(sealing_key)) = (config.vault_address, config.sealing_public_key) else {
        return Err("entries are closed: the engine withholds the vault address".into());
    };
    if tickets == 0 {
        return Err("buy at least one ticket".into());
    }
    let amount_zat = config
        .game
        .price_of(Tickets(tickets))
        .ok_or("the ticket price overflows")?;
    let receiver = PayoutReceiver::from_unified_address(network, payout_address)
        .map_err(|e| format!("payout address: {e}"))?;
    let mut rng = rand_chacha_09::ChaCha20Rng::from_seed(seed);
    let memo = EntryMemo::seal(
        &FORMAT,
        round,
        Tickets(tickets),
        &receiver,
        &sealing_key,
        &mut rng,
    )
    .map_err(|e| e.to_string())?;
    let (bytes, shown) = if text_memo {
        let text = memo.encode_text(&FORMAT);
        (text.clone().into_bytes(), text)
    } else {
        let binary = memo.encode_unpadded(&FORMAT);
        let hex = binary.iter().map(|b| format!("{b:02x}")).collect();
        (binary, hex)
    };
    let uri = entry_request_uri(network, &vault, amount_zat, &bytes).map_err(|e| e.to_string())?;
    Ok(Entry {
        uri,
        round,
        tickets,
        amount_zat,
        memo: shown,
    })
}

fn fresh_seed() -> Result<[u8; 32], JsValue> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(err)?;
    Ok(seed)
}

// ---- JavaScript API ----

/// `verifyPlan(bundleJson) -> {"start", "end", "payout_height"}` (JSON).
#[wasm_bindgen(js_name = verifyPlan)]
pub fn verify_plan(bundle: &str) -> Result<String, JsValue> {
    plan(bundle).and_then(|p| json(&p)).map_err(err)
}

/// `verifyScan(bundleJson, configJson, blocks) -> ["txid", ...]` (JSON).
#[wasm_bindgen(js_name = verifyScan)]
pub fn verify_scan(bundle: &str, config: &str, blocks: &[u8]) -> Result<String, JsValue> {
    scan(bundle, config, blocks)
        .and_then(|t| json(&t))
        .map_err(err)
}

/// `verifyRound(bundleJson, configJson, blocks, [rawTx, ...]) -> report` (JSON).
#[wasm_bindgen(js_name = verifyRound)]
pub fn verify_round(
    bundle: &str,
    config: &str,
    blocks: &[u8],
    txs: js_sys::Array,
) -> Result<String, JsValue> {
    let txs: Vec<Vec<u8>> = txs
        .iter()
        .map(|t| js_sys::Uint8Array::new(&t).to_vec())
        .collect();
    verify(bundle, config, blocks, &txs)
        .and_then(|r| json(&r))
        .map_err(err)
}

/// `buildEntry(configJson, round, tickets, payoutAddress, textMemo) -> entry` (JSON).
#[wasm_bindgen(js_name = buildEntry)]
pub fn build_entry(
    config: &str,
    round: u32,
    tickets: u16,
    payout_address: &str,
    text_memo: bool,
) -> Result<String, JsValue> {
    let seed = fresh_seed()?;
    entry(config, round, tickets, payout_address, text_memo, seed)
        .and_then(|e| json(&e))
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zecraft_core::entry::generate_sealing_keypair;

    #[test]
    fn an_entry_seals_a_zrfl_memo_the_vault_can_open() {
        let mut rng = rand_chacha_09::ChaCha20Rng::from_seed([1; 32]);
        let (sk, pk) = generate_sealing_keypair(&mut rng);
        let vault = PayoutReceiver {
            network: Network::Test,
            orchard: [5; 43],
        }
        .to_unified_address()
        .expect("vault");
        let payout = PayoutReceiver {
            network: Network::Test,
            orchard: [6; 43],
        };
        let config = serde_json::json!({
            "chain": zecraft_core::params::ChainParams::testnet(10),
            "game": RaffleParams::default(),
            "vault_address": vault,
            "sealing_public_key": pk,
        })
        .to_string();
        for text in [false, true] {
            let e = entry(
                &config,
                7,
                2,
                &payout.to_unified_address().expect("ua"),
                text,
                [9; 32],
            )
            .expect("entry");
            assert_eq!((e.round, e.tickets, e.amount_zat), (7, 2, 200_000));
            assert!(e
                .uri
                .starts_with(&format!("zcash:{vault}?amount=0.002&memo=")));
            let memo_bytes = if text {
                e.memo.into_bytes()
            } else {
                (0..e.memo.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&e.memo[i..i + 2], 16).expect("hex"))
                    .collect()
            };
            let memo = EntryMemo::<Tickets>::decode_any(&FORMAT, &memo_bytes).expect("ZRFL");
            assert_eq!(
                memo.open(&FORMAT, Network::Test, &sk).expect("open"),
                payout
            );
        }
        assert!(entry(&config, 7, 0, "x", false, [0; 32]).is_err());
    }

    #[test]
    fn the_saved_round_plans_its_block_range() {
        let bundle = include_str!("../../../../docs/examples/raffle-round-0.proof.json");
        let p = plan(bundle).expect("plan");
        assert_eq!(
            (p.start, p.end, p.payout_height),
            (4_547_139, 4_547_173, Some(4_547_201))
        );
    }
}
