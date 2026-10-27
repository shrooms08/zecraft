//! The raffle's rules (docs/plan.md, section 3).

use proptest::collection::vec;
use proptest::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;
use zecraft_core::draw::{round_window, uniform_below, Randomness};
use zecraft_core::entry::{generate_sealing_keypair, EntryMemo, PayoutReceiver, SealingPublicKey};
use zecraft_core::params::{ChainParams, Network};
use zecraft_core::rules::{
    check_settlement, settle_checked, ForfeitReason, PayoutKind, ReceivedNote, RefundReason,
    RoundInfo, Rules,
};
use zecraft_raffle::{
    owner_of, ticket_ranges, Raffle, RaffleOutcome, RaffleParams, Tickets, DRAW_DOMAIN, FORMAT,
};

const PRICE: u64 = 100_000;
const FEE: u64 = 5_000;

fn p() -> RaffleParams {
    RaffleParams::default()
}

fn chain() -> ChainParams {
    ChainParams::testnet(10_000)
}

fn info(round: u32, voided: bool) -> RoundInfo {
    RoundInfo {
        round,
        window: round_window(&chain(), round).expect("window"),
        voided,
    }
}

fn key() -> SealingPublicKey {
    generate_sealing_keypair(&mut StdRng::seed_from_u64(1)).1
}

fn memo(round: u32, tickets: u16, player: u8) -> EntryMemo<Tickets> {
    let receiver = PayoutReceiver {
        network: Network::Test,
        orchard: [player; 43],
    };
    EntryMemo::seal(
        &FORMAT,
        round,
        Tickets(tickets),
        &receiver,
        &key(),
        &mut StdRng::seed_from_u64(u64::from(player) << 32 | u64::from(round)),
    )
    .expect("seal")
}

fn note(height: u32, i: u16, value: u64, memo: Vec<u8>) -> ReceivedNote {
    ReceivedNote {
        txid: format!("{:064x}", u64::from(i) + 1),
        output_index: 0,
        height,
        value,
        memo,
    }
}

fn entry(round: u32, tickets: u16, value: u64, height: u32, i: u16) -> ReceivedNote {
    note(
        height,
        i,
        value,
        memo(round, tickets, i as u8).encode(&FORMAT).to_vec(),
    )
}

fn text_entry(round: u32, tickets: u16, value: u64, height: u32, i: u16) -> ReceivedNote {
    note(
        height,
        i,
        value,
        memo(round, tickets, i as u8)
            .encode_text(&FORMAT)
            .into_bytes(),
    )
}

#[derive(Debug, PartialEq)]
enum Class {
    Valid(u16),
    Refund(RefundReason),
    Kept,
}

fn classify_one(n: ReceivedNote, voided: bool) -> Class {
    let c = Raffle.classify(&p(), &info(3, voided), &[n]);
    if let Some(v) = c.valid.first() {
        Class::Valid(v.entry.0)
    } else if let Some(r) = c.refunds.first() {
        Class::Refund(r.reason.clone())
    } else {
        assert_eq!(c.forfeits.len(), 1);
        Class::Kept
    }
}

#[test]
fn classification_table() {
    let w = info(3, false).window;
    let in_window = w.start + 2;
    let other_game = {
        let mut bytes = memo(3, 1, 9).encode(&FORMAT);
        bytes[1..5].copy_from_slice(b"ZEEK");
        note(in_window, 9, PRICE, bytes.to_vec())
    };
    let rows: Vec<(&str, ReceivedNote, bool, Class)> = vec![
        (
            "valid, 1 ticket",
            entry(3, 1, PRICE, in_window, 1),
            false,
            Class::Valid(1),
        ),
        (
            "valid, 5 tickets",
            entry(3, 5, 5 * PRICE, w.start, 2),
            false,
            Class::Valid(5),
        ),
        (
            "valid text memo",
            text_entry(3, 2, 2 * PRICE, w.last(), 3),
            false,
            Class::Valid(2),
        ),
        (
            "another round",
            entry(4, 1, PRICE, in_window, 4),
            false,
            Class::Refund(RefundReason::WrongRound { memo_round: 4 }),
        ),
        (
            "outside the window",
            entry(3, 1, PRICE, w.end, 5),
            false,
            Class::Refund(RefundReason::OutOfWindow),
        ),
        (
            "zero tickets",
            entry(3, 0, PRICE, in_window, 6),
            false,
            Class::Refund(RefundReason::InvalidEntry),
        ),
        (
            "underpaid",
            entry(3, 2, PRICE, in_window, 7),
            false,
            Class::Refund(RefundReason::WrongValue {
                expected: 2 * PRICE,
            }),
        ),
        (
            "overpaid",
            entry(3, 1, PRICE + 1, in_window, 8),
            false,
            Class::Refund(RefundReason::WrongValue { expected: PRICE }),
        ),
        (
            "voided round",
            entry(3, 1, PRICE, in_window, 10),
            true,
            Class::Refund(RefundReason::RoundVoided),
        ),
        (
            "undecodable memo",
            note(in_window, 11, PRICE, vec![0xF6]),
            false,
            Class::Kept,
        ),
        ("another game's magic", other_game, false, Class::Kept),
        (
            "refund cannot pay its fees",
            entry(3, 1, 2 * FEE, in_window, 12),
            false,
            Class::Kept,
        ),
        (
            "just pays its fees",
            entry(3, 1, 2 * FEE + 1, in_window, 13),
            false,
            Class::Refund(RefundReason::WrongValue { expected: PRICE }),
        ),
    ];
    for (name, n, voided, expected) in rows {
        assert_eq!(classify_one(n, voided), expected, "{name}");
    }
    // A kept note says why.
    let c = Raffle.classify(
        &p(),
        &info(3, false),
        &[entry(3, 1, 2 * FEE, in_window, 14)],
    );
    assert_eq!(
        c.forfeits[0].reason,
        ForfeitReason::TooSmallToRefund { fees: 2 * FEE }
    );
}

#[test]
fn tickets_map_to_entries_in_height_txid_action_order() {
    let w = info(3, false).window;
    // Listed out of chain order on purpose.
    let notes = vec![
        entry(3, 2, 2 * PRICE, w.start + 5, 1), // third: tickets 4..6
        entry(3, 3, 3 * PRICE, w.start + 1, 2), // first: tickets 0..3
        {
            let mut n = entry(3, 1, PRICE, w.start + 1, 2); // same txid, action 1: 3
            n.output_index = 1;
            n
        },
    ];
    let c = Raffle.classify(&p(), &info(3, false), &notes);
    assert_eq!(c.valid.len(), 3);
    let ranges = ticket_ranges(&c.valid).expect("ranges");
    assert_eq!(ranges, vec![(1, 0), (2, 3), (0, 4)]);
    let owner = |t| owner_of(&c.valid, t).expect("owner").map(|(i, _)| i);
    for (ticket, expected) in [(0, 1), (2, 1), (3, 2), (4, 0), (5, 0)] {
        assert_eq!(owner(ticket), Some(expected), "ticket {ticket}");
    }
    assert_eq!(owner(6), None, "past the last ticket");
}

#[test]
fn draw_known_answer_is_stable() {
    // Pinned on 2026-10-27, so engine, keyholders and verifiers can never
    // silently disagree on a winning ticket once deployed.
    let r = Randomness(
        <[u8; 32]>::try_from(
            (0..32)
                .map(|i| {
                    u8::from_str_radix(
                        &"7f7f0a0e0e2c1c84a4c9f5fcd5b06b8f45b6b9a3a2d8c0e1f2a3b4c5d6e7f809"
                            [2 * i..2 * i + 2],
                        16,
                    )
                    .expect("hex")
                })
                .collect::<Vec<u8>>(),
        )
        .expect("32 bytes"),
    );
    assert_eq!(DRAW_DOMAIN, b"zecraft/raffle/v1");
    assert_eq!(uniform_below(DRAW_DOMAIN, 42, &r, 6).expect("draw"), 0);
    assert_eq!(uniform_below(DRAW_DOMAIN, 0, &r, 1_000).expect("draw"), 928);
    assert_eq!(
        uniform_below(DRAW_DOMAIN, 7, &r, 65_535).expect("draw"),
        20_416
    );

    // And through the settlement: six tickets in round 42 with this beacon
    // go to ticket 0, the first entry's.
    let w = round_window(&chain(), 42).expect("window");
    let notes = vec![
        entry(42, 3, 3 * PRICE, w.start, 1),
        entry(42, 3, 3 * PRICE, w.start + 1, 2),
    ];
    let info = RoundInfo {
        round: 42,
        window: w,
        voided: false,
    };
    let c = Raffle.classify(&p(), &info, &notes);
    let s = settle_checked(&Raffle, &p(), &c, 0, Some(&r)).expect("settle");
    assert_eq!(s.outcome.winning_ticket, Some(0));
    assert_eq!(
        s.outcome.winner.as_ref().map(|w| w.txid.clone()),
        Some(notes[0].txid.clone())
    );
}

#[test]
fn the_winner_gets_the_pot_and_the_carry_in_one_output() {
    let w = info(3, false).window;
    let notes = vec![
        entry(3, 2, 2 * PRICE, w.start, 1),
        entry(3, 1, PRICE, w.start, 2),
        entry(5, 1, PRICE, w.start, 3),       // refund
        note(w.start, 4, 70_000, vec![0xF6]), // kept
    ];
    let c = Raffle.classify(&p(), &info(3, false), &notes);
    let s = settle_checked(&Raffle, &p(), &c, 1_234, Some(&Randomness([3; 32]))).expect("settle");
    let wins: Vec<_> = s
        .payouts
        .iter()
        .filter(|o| o.kind == PayoutKind::Win)
        .collect();
    assert_eq!(wins.len(), 1);
    assert_eq!(wins[0].gross, 3 * PRICE - 2 * FEE + 1_234);
    assert_eq!(wins[0].net, wins[0].gross - FEE);
    assert_eq!(s.payouts.len(), 2);
    assert_eq!(s.payout_inputs.len(), 3);
    assert_eq!(s.fees, 3 * FEE + 2 * FEE);
    assert_eq!(
        s.carry_out, 70_000,
        "the carry went to the winner; the kept note stays"
    );
    assert!(s.outcome.winner_paid);
}

#[test]
fn a_voided_round_refunds_everything() {
    let w = info(3, true).window;
    let notes = vec![
        entry(3, 2, 2 * PRICE, w.start, 1),
        entry(3, 1, PRICE, w.start, 2),
        entry(8, 1, PRICE, w.start, 3),
        entry(3, 1, 9_000, w.start, 4), // cannot pay its refund fees
        note(w.start, 5, 30_000, vec![0xF6]), // undecodable
    ];
    let c = Raffle.classify(&p(), &info(3, true), &notes);
    assert!(c.valid.is_empty());
    assert_eq!(c.refunds.len(), 3);
    let s = settle_checked(&Raffle, &p(), &c, 500, None).expect("settle");
    assert!(s.voided);
    assert!(s
        .payouts
        .iter()
        .all(|o| matches!(o.kind, PayoutKind::Refund { .. })));
    assert_eq!(s.paid, (2 * PRICE - 2 * FEE) + (PRICE - 2 * FEE) * 2);
    assert_eq!(
        s.outcome,
        RaffleOutcome {
            tickets_sold: 0,
            winning_ticket: None,
            winner: None,
            winner_paid: false
        }
    );
    assert_eq!(s.carry_out, 500 + 9_000 + 30_000);
    // A voided round settled with a draw is refused by the kit.
    assert!(settle_checked(&Raffle, &p(), &c, 0, Some(&Randomness([1; 32]))).is_err());
}

#[test]
fn zero_tickets_carry_everything() {
    let w = info(3, false).window;
    let notes = vec![
        entry(4, 1, PRICE, w.start, 1),       // refunded, not a ticket
        note(w.start, 2, 80_000, vec![0xF6]), // kept
    ];
    let c = Raffle.classify(&p(), &info(3, false), &notes);
    let s =
        settle_checked(&Raffle, &p(), &c, 2_000_000, Some(&Randomness([9; 32]))).expect("settle");
    assert_eq!(s.outcome.tickets_sold, 0);
    assert_eq!(s.outcome.winning_ticket, None);
    assert!(s.payouts.iter().all(|o| o.kind != PayoutKind::Win));
    assert_eq!(
        s.carry_out,
        2_000_000 + 80_000,
        "carry and kept notes carry over"
    );
    let empty = Raffle.classify(&p(), &info(3, false), &[]);
    let s = settle_checked(&Raffle, &p(), &empty, 777, Some(&Randomness([9; 32]))).expect("settle");
    assert!(s.payouts.is_empty() && s.payout_inputs.is_empty());
    assert_eq!(s.carry_out, 777);
}

#[test]
fn params_are_validated_against_the_chain() {
    let chain = chain();
    Raffle.validate(&chain, &p()).expect("defaults are valid");
    assert_eq!(p().ticket_price_zat, 100_000, "0.001 TAZ");
    let cheap = RaffleParams {
        ticket_price_zat: 2 * FEE,
        ..p()
    };
    assert!(Raffle.validate(&chain, &cheap).is_err());
    let underfee = RaffleParams {
        output_fee_zat: 1,
        ..p()
    };
    assert!(Raffle.validate(&chain, &underfee).is_err());
}

/// One random note of a round: (kind, tickets, value tweak, height offset).
fn arb_notes() -> impl Strategy<Value = Vec<(u8, u16, u64, u32)>> {
    vec((0u8..6, 0u16..40, 0u64..300_000, 0u32..30), 0..14)
}

fn build(specs: &[(u8, u16, u64, u32)], window_start: u32) -> Vec<ReceivedNote> {
    specs
        .iter()
        .enumerate()
        .map(|(i, &(kind, tickets, tweak, offset))| {
            let i = i as u16 + 1;
            let height = window_start + offset; // offsets ≥ 24 fall outside
            match kind {
                0 | 1 => entry(
                    3,
                    tickets.max(1),
                    PRICE * u64::from(tickets.max(1)),
                    height,
                    i,
                ),
                2 => entry(3, tickets, tweak, height, i),
                3 => entry(4, tickets, tweak + 20_000, height, i),
                4 => note(height, i, tweak + 1, vec![0xF6]),
                _ => text_entry(
                    3,
                    tickets.max(1),
                    PRICE * u64::from(tickets.max(1)),
                    height,
                    i,
                ),
            }
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Whatever the round, the settlement passes the kit's invariants
    /// (conservation included), pays at most one winner, and the winner
    /// gets exactly the pot plus the carry minus one output fee.
    #[test]
    fn settlement_conserves_value(
        specs in arb_notes(),
        carry_in in 0u64..5_000_000,
        voided in any::<bool>(),
        seed in any::<[u8; 32]>(),
    ) {
        let round = info(3, voided);
        let notes = build(&specs, round.window.start);
        let c = Raffle.classify(&p(), &round, &notes);
        let draw = (!voided).then_some(Randomness(seed));
        let s = settle_checked(&Raffle, &p(), &c, carry_in, draw.as_ref()).expect("settles");
        check_settlement(&c, &s).expect("invariants");
        prop_assert_eq!(s.received + s.carry_in, s.paid + s.fees + s.carry_out);
        let wins: Vec<_> = s.payouts.iter().filter(|o| o.kind == PayoutKind::Win).collect();
        prop_assert!(wins.len() <= 1);
        if let Some(win) = wins.first() {
            let pot: u64 = c.valid.iter().map(|v| v.note.value - FEE).sum();
            prop_assert_eq!(win.gross, pot + carry_in);
            prop_assert_eq!(win.net, win.gross - FEE);
        }
        let sold: u64 = c.valid.iter().map(|v| u64::from(v.entry.0)).sum();
        prop_assert_eq!(u64::from(s.outcome.tickets_sold), sold);
        if let Some(t) = s.outcome.winning_ticket {
            prop_assert!(u64::from(t) < sold);
        }
    }
}
