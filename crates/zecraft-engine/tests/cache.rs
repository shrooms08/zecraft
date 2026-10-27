//! Which proof bundles may be cached for good (`/v1/rounds/{n}/proof`).

use zecraft_core::draw::{RoundWindow, TimingVerdict};
use zecraft_core::entry::SealedReceiver;
use zecraft_core::params::ChainParams;
use zecraft_core::proof::{PayoutOpenings, ProofBundle, PROOF_VERSION};
use zecraft_core::rules::{Payout, PayoutKind, Settlement};
use zecraft_engine::api::bundle_is_final;

fn bundle(payouts: usize, openings: bool) -> ProofBundle<()> {
    let payout = Payout {
        sealed: SealedReceiver::from_bytes(&[7; 92]).expect("sealed"),
        aad: vec![1],
        gross: 10_000,
        fee: 5_000,
        net: 5_000,
        kind: PayoutKind::Win,
    };
    ProofBundle {
        version: PROOF_VERSION,
        chain: ChainParams::testnet(1),
        game: (),
        round: 0,
        window: RoundWindow { start: 1, end: 25 },
        notes: vec![],
        start_block_time: 0,
        post_close_block_times: vec![],
        drand_round: 1,
        beacon_time: 0,
        beacon: None,
        randomness: None,
        timing: TimingVerdict::Unsafe {
            median_time_past: 0,
            beacon_time: 0,
            deficit_secs: 1,
        },
        carry_in: 0,
        settlement: Settlement {
            round: 0,
            voided: true,
            outcome: serde_json::Value::Null,
            received: 0,
            carry_in: 0,
            payouts: vec![payout; payouts],
            payout_inputs: vec![],
            paid: 0,
            fees: 0,
            carry_out: 0,
        },
        payout: None,
        payout_openings: openings.then(|| PayoutOpenings {
            txid: "00".repeat(32),
            mined_height: 30,
            outputs: vec![],
        }),
        unclaimable_payouts: vec![],
    }
}

#[test]
fn a_mined_payout_or_nothing_to_pay_is_final() {
    assert!(bundle_is_final(&bundle(1, true)));
    assert!(bundle_is_final(&bundle(0, false)));
}

#[test]
fn a_payout_not_mined_yet_is_not_final() {
    assert!(!bundle_is_final(&bundle(1, false)));
}
