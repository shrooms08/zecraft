//! Regression tests of the generic primitives against Zeek's real testnet
//! rounds (`docs/examples/`, copied from Zeek @ e58587e).
//!
//! Zeek is not a Zecraft game any more: its tile settlement is gone. What
//! these 26 files still pin down is that the kit's **generic** primitives
//! reproduce, byte for byte, what Zeek's engine computed on the live chain:
//!
//! * `ChainParams` deserializes from Zeek's `params` (its chain half);
//! * round windows, the beacon round binding and the round status;
//! * drand quicknet beacon verification and its randomness;
//! * the timing (median-time-past) rule's verdict;
//! * `uniform_below` with Zeek's draw domains reproducing Zeek's treasure
//!   tile and Deep Cache trigger;
//! * the entry memo header decode (binary and text) with Zeek's magic and
//!   prefix, matching every payout's round, tile and sealed receiver;
//! * HPKE seal/open under Zeek's HPKE context.
//!
//! Every Zeek constant below is supplied by this test: none of them exists
//! in the kit's non-test code any more.

use std::fs;
use std::path::{Path, PathBuf};

use rand::rngs::StdRng;
use rand::SeedableRng;
use serde_json::Value;
use zecraft_core::draw::{self, Beacon, Randomness, RoundContext, RoundStatus, RoundWindow};
use zecraft_core::entry::{
    self, generate_sealing_keypair, EntryMemo, MemoFormat, Payload, PayloadError, PayoutReceiver,
    SealError, SealedReceiver,
};
use zecraft_core::params::{ChainParams, Network};

/// Zeek's dig memo, as a caller-supplied format.
const ZEEK: MemoFormat = MemoFormat {
    magic: *b"ZEEK",
    version: 1,
    text_prefix: "zeek1:",
    seal_info: b"zeek/payout-seal/v1",
};
const ZEEK_TREASURE_DOMAIN: &[u8] = b"zeek/treasure/v1";
const ZEEK_CACHE_DOMAIN: &[u8] = b"zeek/cache/v1";
const ZEEK_TILES: u32 = 25;

/// Zeek's payload: one tile byte, 0..25.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tile(u8);

impl Payload for Tile {
    const LEN: usize = 1;
    fn to_bytes(&self) -> Vec<u8> {
        vec![self.0]
    }
    fn from_bytes(bytes: &[u8]) -> Result<Self, PayloadError> {
        match bytes {
            [t] if u32::from(*t) < ZEEK_TILES => Ok(Tile(*t)),
            _ => Err(PayloadError("tile out of range".into())),
        }
    }
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples")
}

fn files(suffix: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, suffix: &str, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, suffix, out);
            } else if path.to_string_lossy().ends_with(suffix)
                // Zecraft's own rounds (raffle-round-*.json) live next to
                // Zeek's fixtures; they are tested by games/raffle.
                && !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("raffle-"))
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&examples(), suffix, &mut out);
    out.sort();
    out
}

fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read")).expect("json")
}

fn chain_of(bundle: &Value) -> ChainParams {
    let chain: ChainParams =
        serde_json::from_value(bundle["params"].clone()).expect("chain params from Zeek params");
    chain.validate().expect("valid chain params");
    chain
}

fn u64_of(v: &Value) -> u64 {
    v.as_u64().expect("u64")
}

#[test]
fn there_are_26_fixture_files() {
    assert_eq!(files(".proof.json").len(), 14);
    assert_eq!(files(".round.json").len(), 12);
}

#[test]
fn windows_beacon_binding_timing_and_draws_match_zeek() {
    for path in files(".proof.json") {
        let name = path.display().to_string();
        let b = json(&path);
        let chain = chain_of(&b);
        let round = u32::try_from(u64_of(&b["round"])).expect("round");

        let window: RoundWindow = serde_json::from_value(b["window"].clone()).expect("window");
        assert_eq!(
            draw::round_window(&chain, round).expect("window"),
            window,
            "{name}"
        );

        let start = u64_of(&b["start_block_time"]);
        let drand_round = draw::beacon_round_for(&chain, start).expect("drand round");
        assert_eq!(
            drand_round,
            u64_of(&b["drand_round"]),
            "{name}: beacon binding"
        );
        let beacon_time = draw::beacon_time(&chain, drand_round).expect("beacon time");
        assert_eq!(beacon_time, u64_of(&b["beacon_time"]), "{name}");

        let times: Vec<u64> =
            serde_json::from_value(b["post_close_block_times"].clone()).expect("times");
        let verdict = draw::check_window_close(&chain, beacon_time, &times).expect("verdict");
        let expected: draw::TimingVerdict =
            serde_json::from_value(b["timing"].clone()).expect("timing");
        assert_eq!(verdict, expected, "{name}: timing rule");

        let outcome = &b["outcome"];
        if outcome["outcome"] == "voided" {
            assert!(
                matches!(verdict, draw::TimingVerdict::Unsafe { .. }),
                "{name}"
            );
            continue;
        }
        let beacon: Beacon = serde_json::from_value(b["beacon"].clone()).expect("beacon");
        assert_eq!(beacon.round, drand_round, "{name}");
        let randomness = draw::verify_beacon(&chain.drand, &beacon).expect("beacon verifies");
        let stated: Randomness =
            serde_json::from_value(b["randomness"].clone()).expect("randomness");
        assert_eq!(randomness, stated, "{name}: randomness");

        let tile = draw::uniform_below(ZEEK_TREASURE_DOMAIN, round, &randomness, ZEEK_TILES)
            .expect("tile");
        assert_eq!(
            u64::from(tile),
            u64_of(&outcome["treasure_tile"]),
            "{name}: tile"
        );
        let cache_odds = u32::try_from(u64_of(&b["params"]["cache_odds"])).expect("odds");
        let cache = draw::uniform_below(ZEEK_CACHE_DOMAIN, round, &randomness, cache_odds)
            .expect("cache")
            == 0;
        assert_eq!(
            cache,
            outcome["cache_triggered"].as_bool().expect("bool"),
            "{name}: cache trigger"
        );
    }
}

#[test]
fn round_status_matches_zeek_round_views() {
    for path in files(".round.json") {
        let name = path.display().to_string();
        let view = json(&path);
        let proof_path = PathBuf::from(name.replace(".round.json", ".proof.json"));
        let chain = chain_of(&json(&proof_path));
        let round = u32::try_from(u64_of(&view["round"])).expect("round");
        let window: RoundWindow = serde_json::from_value(view["window"].clone()).expect("window");
        assert_eq!(
            draw::round_window(&chain, round).expect("window"),
            window,
            "{name}"
        );
        let ctx: RoundContext = serde_json::from_value(view["context"].clone()).expect("context");
        let status: RoundStatus = serde_json::from_value(view["status"].clone()).expect("status");
        assert_eq!(
            draw::round_status(&chain, round, &ctx).expect("status"),
            status,
            "{name}: status"
        );
    }
}

/// Decodes a Zeek note memo with the generic decoder: binary always, text
/// from the round's text-memo activation height. A Campfire dig (Zeek
/// version 2) appends a message after the same header and sealed receiver;
/// its first bytes are decoded as the version 1 layout they share (Zeek's AAD
/// kept version 1 for both).
fn decode_zeek(bundle: &Value, note: &Value) -> Option<EntryMemo<Tile>> {
    let memo = hex::decode(note["memo"].as_str().expect("memo hex")).expect("hex");
    let height = u64_of(&note["height"]);
    let text_from = bundle["params"]["text_memo_activation_height"].as_u64();
    let text_ok = text_from.is_some_and(|h| height >= h);
    if let Ok(m) = EntryMemo::<Tile>::decode(&ZEEK, &memo) {
        return Some(m);
    }
    if text_ok {
        if let Ok(m) = EntryMemo::<Tile>::decode_text(&ZEEK, &memo) {
            return Some(m);
        }
    }
    let mut memo = memo;
    if text_ok && memo.starts_with(ZEEK.text_prefix.as_bytes()) {
        use base64::Engine;
        let end = memo.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&memo[ZEEK.text_prefix.len()..end])
            .ok()?;
        memo = [vec![0xFF], payload].concat();
    }
    if memo.len() > 5 && memo[0] == 0xFF && memo[1..5] == ZEEK.magic && memo[5] == 2 {
        let len = ZEEK.memo_len(Tile::LEN);
        let mut head = memo[..len].to_vec();
        head[5] = 1;
        return EntryMemo::<Tile>::decode(&ZEEK, &head).ok();
    }
    None
}

#[test]
fn memo_headers_decode_and_match_every_payout() {
    let mut decoded_total = 0;
    let mut text_total = 0;
    let mut payouts_total = 0;
    for path in files(".proof.json") {
        let name = path.display().to_string();
        let b = json(&path);
        let notes = b["notes"].as_array().expect("notes");
        let decoded: Vec<EntryMemo<Tile>> =
            notes.iter().filter_map(|n| decode_zeek(&b, n)).collect();
        decoded_total += decoded.len();
        text_total += notes
            .iter()
            .filter(|n| {
                n["memo"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("7a65656b313a"))
            })
            .filter(|n| decode_zeek(&b, n).is_some())
            .count();
        for payout in b["settlement"]["payouts"].as_array().expect("payouts") {
            payouts_total += 1;
            let sealed: SealedReceiver =
                serde_json::from_value(payout["sealed_receiver"].clone()).expect("sealed");
            let round = u32::try_from(u64_of(&payout["memo_round"])).expect("round");
            let tile = u8::try_from(u64_of(&payout["memo_tile"])).expect("tile");
            let entry = decoded
                .iter()
                .find(|m| m.sealed_receiver == sealed)
                .unwrap_or_else(|| panic!("{name}: no decoded memo carries a payout's receiver"));
            assert_eq!((entry.round, entry.payload), (round, Tile(tile)), "{name}");
            // The AAD Zeek bound is exactly the generic header.
            let mut zeek_aad = b"ZEEK\x01".to_vec();
            zeek_aad.extend_from_slice(&round.to_be_bytes());
            zeek_aad.push(tile);
            assert_eq!(entry.aad(&ZEEK), zeek_aad, "{name}");
        }
    }
    assert!(decoded_total >= 150, "decoded {decoded_total}");
    // Two version 1 text digs and one Campfire text dig; the other two text
    // memos are Zeek shouts, which are not entries.
    assert_eq!(text_total, 3, "text memos decoded");
    assert_eq!(payouts_total, 10);
}

#[test]
fn hpke_seal_open_under_zeek_context() {
    // Zeek's sealing key never left Zeek, so its real receivers cannot be
    // opened here. What is checked: every sealed receiver in the fixtures is
    // a well-formed HPKE ciphertext that a wrong key rejects as such (not as
    // malformed), and a receiver sealed with Zeek's context and AAD layout
    // opens only under that context.
    let mut rng = StdRng::seed_from_u64(26);
    let (sk, pk) = generate_sealing_keypair(&mut rng);
    for path in files(".proof.json") {
        let b = json(&path);
        for note in b["notes"].as_array().expect("notes") {
            let Some(memo) = decode_zeek(&b, note) else {
                continue;
            };
            assert!(matches!(
                memo.open(&ZEEK, Network::Test, &sk),
                Err(entry::MemoError::Seal(SealError::Open))
            ));
        }
    }
    let receiver = PayoutReceiver {
        network: Network::Test,
        orchard: [0x2a; 43],
    };
    let memo = EntryMemo::seal(&ZEEK, 251, Tile(8), &receiver, &pk, &mut rng).expect("seal");
    assert_eq!(
        memo.open(&ZEEK, Network::Test, &sk).expect("open"),
        receiver
    );
    let mut zeek_aad = b"ZEEK\x01".to_vec();
    zeek_aad.extend_from_slice(&251u32.to_be_bytes());
    zeek_aad.push(8);
    let plaintext = entry::open(
        b"zeek/payout-seal/v1",
        &sk,
        memo.sealed_receiver.as_bytes(),
        &zeek_aad,
    )
    .expect("opens with Zeek's literal context and AAD");
    assert_eq!(plaintext, receiver.to_plaintext());
    assert_eq!(
        entry::open(
            b"zecraft/other/v1",
            &sk,
            memo.sealed_receiver.as_bytes(),
            &zeek_aad
        ),
        Err(SealError::Open)
    );
}
