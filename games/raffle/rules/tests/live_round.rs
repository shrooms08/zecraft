//! The first live testnet round (docs/examples/raffle-round-0.proof.json,
//! settled and paid on 2026-10-27) recomputes byte for byte: beacon
//! signature, timing rule, classification, draw and settlement.

use zecraft_core::proof::{recompute, ProofBundle};
use zecraft_raffle::{Raffle, RaffleOutcome, RaffleParams};

#[test]
fn round_0_recomputes_byte_for_byte() {
    let bundle: ProofBundle<RaffleParams> = serde_json::from_str(include_str!(
        "../../../../docs/examples/raffle-round-0.proof.json"
    ))
    .expect("bundle");
    let recomputed = recompute(&Raffle, &bundle).expect("recompute");
    assert_eq!(
        serde_json::to_string(&recomputed).expect("json"),
        serde_json::to_string(&bundle.settlement).expect("json")
    );
    let outcome: RaffleOutcome = serde_json::from_value(recomputed.outcome).expect("outcome");
    assert_eq!((outcome.tickets_sold, outcome.winning_ticket), (6, Some(2)));
    assert_eq!(recomputed.paid, 580_000);
    assert_eq!(bundle.drand_round, 33_431_631);
}
