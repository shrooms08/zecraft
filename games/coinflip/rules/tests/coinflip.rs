//! The coin flip's rules: classification, the draw, settlement, conservation.

use proptest::collection::vec;
use proptest::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;
use zecraft_coinflip::{CoinFlip, CoinFlipParams, FlipOutcome, Side, DRAW_DOMAIN, FORMAT};
use zecraft_core::draw::{round_window, uniform_below, Randomness};
use zecraft_core::entry::{generate_sealing_keypair, EntryMemo, PayoutReceiver};
use zecraft_core::params::{ChainParams, Network};
use zecraft_core::rules::{
    check_settlement, settle_checked, ForfeitReason, PayoutKind, ReceivedNote, RefundReason,
    RoundInfo, Rules,
};

fn p() -> CoinFlipParams {
    CoinFlipParams::default()
}

const STAKE: u64 = 100_000;
const FEE: u64 = 5_000;

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

fn memo(round: u32, side: u8, player: u8) -> EntryMemo<Side> {
    let (_, pk) = generate_sealing_keypair(&mut StdRng::seed_from_u64(1));
    let receiver = PayoutReceiver {
        network: Network::Test,
        orchard: [player; 43],
    };
    EntryMemo::seal(
        &FORMAT,
        round,
        Side(side),
        &receiver,
        &pk,
        &mut StdRng::seed_from_u64(u64::from(player)),
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

fn entry(round: u32, side: u8, value: u64, height: u32, i: u16) -> ReceivedNote {
    note(
        height,
        i,
        value,
        memo(round, side, i as u8).encode(&FORMAT).to_vec(),
    )
}

#[test]
fn classification_table() {
    let w = info(3, false).window;
    let h = w.start + 1;
    let text = note(h, 9, STAKE, memo(3, 1, 9).encode_text(&FORMAT).into_bytes());
    let c = CoinFlip.classify(
        &p(),
        &info(3, false),
        &[
            entry(3, 0, STAKE, h, 1),      // valid heads
            text,                          // valid tails, text memo
            entry(4, 0, STAKE, h, 2),      // another round: refund
            entry(3, 1, STAKE, w.end, 3),  // outside the window: refund
            entry(3, 0, STAKE + 1, h, 4),  // wrong stake: refund
            entry(3, 0, 2 * FEE, h, 5),    // cannot pay its refund: kept
            note(h, 6, STAKE, vec![0xF6]), // not a coin flip memo: kept
        ],
    );
    let sides: Vec<u8> = c.valid.iter().map(|v| v.entry.0).collect();
    assert_eq!(sides, vec![0, 1]);
    let reasons: Vec<RefundReason> = c.refunds.iter().map(|r| r.reason.clone()).collect();
    assert_eq!(
        reasons,
        vec![
            RefundReason::WrongRound { memo_round: 4 },
            RefundReason::OutOfWindow,
            RefundReason::WrongValue { expected: STAKE },
        ]
    );
    assert_eq!(c.forfeits.len(), 2);
    assert_eq!(
        c.forfeits[0].reason,
        ForfeitReason::TooSmallToRefund { fees: 2 * FEE }
    );
    let board = CoinFlip.board(&p(), &c);
    assert_eq!(
        (board["heads"].clone(), board["tails"].clone()),
        (1.into(), 1.into())
    );
    // In a voided round every decodable entry is refunded.
    let voided = CoinFlip.classify(&p(), &info(3, true), &[entry(3, 0, STAKE, h, 1)]);
    assert!(voided.valid.is_empty());
    assert_eq!(voided.refunds[0].reason, RefundReason::RoundVoided);
}

/// Pinned draws for `DRAW_DOMAIN` (rounds 0..8, randomness 0x5a…5a), so the
/// engine, the keyholders and every verifier can never disagree on a side.
/// `zecraft new` recomputes this line for a new game's domain.
const KAT: [u32; 8] = [0, 1, 0, 0, 1, 0, 1, 1]; // zecraft-new: KAT

#[test]
fn draw_known_answer_is_stable() {
    let r = Randomness([0x5a; 32]);
    let drawn: Vec<u32> = (0..8)
        .map(|round| uniform_below(DRAW_DOMAIN, round, &r, 2).expect("draw"))
        .collect();
    assert_eq!(drawn, KAT);
    // And through the settlement: round 1 is tails.
    let w = info(1, false).window;
    let c = CoinFlip.classify(
        &p(),
        &info(1, false),
        &[
            entry(1, 0, STAKE, w.start, 1),
            entry(1, 1, STAKE, w.start, 2),
        ],
    );
    let s = settle_checked(&CoinFlip, &p(), &c, 0, Some(&r)).expect("settle");
    assert_eq!(s.outcome.side, Some(KAT[1] as u8));
}

#[test]
fn winners_split_the_pot_and_the_carry() {
    let w = info(2, false).window;
    let notes: Vec<ReceivedNote> = (0..5)
        .map(|i| entry(2, (i % 2) as u8, STAKE, w.start, i))
        .collect();
    let c = CoinFlip.classify(&p(), &info(2, false), &notes);
    for seed in 0..16u8 {
        let s = settle_checked(&CoinFlip, &p(), &c, 1_001, Some(&Randomness([seed; 32])))
            .expect("settle");
        let side = s.outcome.side.expect("drawn");
        let winners = notes
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i % 2) as u8 == side)
            .count();
        let pot = 5 * (STAKE - FEE) + 1_001;
        let share = pot / winners as u64;
        assert_eq!(s.outcome.winners as usize, winners);
        assert!(s
            .payouts
            .iter()
            .all(|o| o.kind == PayoutKind::Win && o.gross == share && o.net == share - FEE));
        assert_eq!(
            s.carry_out,
            pot - share * winners as u64,
            "the dust carries"
        );
    }
}

#[test]
fn a_voided_round_refunds_and_an_empty_one_carries() {
    let w = info(3, true).window;
    let c = CoinFlip.classify(
        &p(),
        &info(3, true),
        &[
            entry(3, 0, STAKE, w.start, 1),
            note(w.start, 2, 70_000, vec![0xF6]),
        ],
    );
    let s = settle_checked(&CoinFlip, &p(), &c, 400, None).expect("settle");
    assert_eq!(
        s.outcome,
        FlipOutcome {
            side: None,
            entries: 0,
            winners: 0,
            share_zat: 0
        }
    );
    assert_eq!(s.payouts.len(), 1);
    assert_eq!(s.payouts[0].net, STAKE - 2 * FEE);
    assert_eq!(s.carry_out, 400 + 70_000);

    let empty = CoinFlip.classify(&p(), &info(4, false), &[]);
    let s =
        settle_checked(&CoinFlip, &p(), &empty, 9_999, Some(&Randomness([1; 32]))).expect("settle");
    assert!(s.payouts.is_empty());
    assert_eq!(s.carry_out, 9_999);
}

#[test]
fn params_are_validated() {
    CoinFlip
        .validate(&chain(), &p())
        .expect("defaults are valid");
    let cheap = CoinFlipParams {
        stake_zat: 2 * FEE,
        ..p()
    };
    assert!(CoinFlip.validate(&chain(), &cheap).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Whatever arrives, the settlement passes the kit's invariants,
    /// conservation included.
    #[test]
    fn settlement_conserves_value(
        specs in vec((0u8..5, 0u8..2, 0u64..300_000, 0u32..30), 0..14),
        carry_in in 0u64..5_000_000,
        voided in any::<bool>(),
        seed in any::<[u8; 32]>(),
    ) {
        let round = info(3, voided);
        let notes: Vec<ReceivedNote> = specs
            .iter()
            .enumerate()
            .map(|(i, &(kind, side, tweak, offset))| {
                let i = i as u16 + 1;
                let h = round.window.start + offset;
                match kind {
                    0 | 1 => entry(3, side, STAKE, h, i),
                    2 => entry(3, side, tweak + 1, h, i),
                    3 => entry(5, side, tweak + 20_000, h, i),
                    _ => note(h, i, tweak + 1, vec![0xF6]),
                }
            })
            .collect();
        let c = CoinFlip.classify(&p(), &round, &notes);
        let draw = (!voided).then_some(Randomness(seed));
        let s = settle_checked(&CoinFlip, &p(), &c, carry_in, draw.as_ref()).expect("settles");
        check_settlement(&c, &s).expect("invariants");
        prop_assert_eq!(s.received + s.carry_in, s.paid + s.fees + s.carry_out);
    }
}
