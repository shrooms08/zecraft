//! A second game behind the `Rules` trait, so the seam is proven against a
//! game it did not come from: a coin flip.
//!
//! An entry stakes exactly `stake_zat` on heads or tails. One draw decides
//! the side; the winners split the pot (the stakes after their entry fees,
//! plus the carry) equally, and the dust carries over. No winner: everything
//! carries. Refunds and forfeits follow the same rules as the raffle: a
//! decodable entry that cannot play is refunded minus its entry fee and its
//! output fee, unless it cannot pay them, in which case it is kept.

use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use zecraft_core::draw::{round_window, uniform_below, Randomness};
use zecraft_core::entry::{
    generate_sealing_keypair, EntryMemo, MemoFormat, Payload, PayloadError, PayoutReceiver,
};
use zecraft_core::params::{ChainParams, Network};
use zecraft_core::rules::{
    screen, settle_checked, Classified, Forfeit, ForfeitReason, InputRef, Payout, PayoutKind,
    ReceivedNote, Refund, RefundReason, RoundInfo, Rules, RulesError, Screened, Settlement,
    SettlementError, Valid,
};

const DOMAIN: &[u8] = b"zecraft-test/coin-flip/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Side(u8);

impl Payload for Side {
    const LEN: usize = 1;
    fn to_bytes(&self) -> Vec<u8> {
        vec![self.0]
    }
    fn from_bytes(bytes: &[u8]) -> Result<Self, PayloadError> {
        match bytes {
            [s @ (0 | 1)] => Ok(Side(*s)),
            _ => Err(PayloadError("a side is 0 (heads) or 1 (tails)".into())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct FlipParams {
    stake_zat: u64,
    entry_fee_zat: u64,
    output_fee_zat: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Flip {
    side: Option<u8>,
    winners: usize,
}

struct CoinFlip;

impl Rules for CoinFlip {
    const FORMAT: MemoFormat = MemoFormat {
        magic: *b"FLIP",
        version: 1,
        text_prefix: "flip1:",
        seal_info: b"zecraft-test/coin-flip/payout-seal/v1",
    };
    type Params = FlipParams;
    type Entry = Side;
    type Outcome = Flip;

    fn classify(
        &self,
        p: &FlipParams,
        round: &RoundInfo,
        notes: &[ReceivedNote],
    ) -> Classified<Side> {
        let mut out = Classified::empty(round);
        let fees = p.entry_fee_zat + p.output_fee_zat;
        for note in notes {
            let (memo, reason) = match screen::<Side>(&Self::FORMAT, round, note) {
                Screened::Undecodable(e) => {
                    let reason = ForfeitReason::UndecodableMemo {
                        error: e.to_string(),
                    };
                    out.forfeits.push(Forfeit {
                        note: note.clone(),
                        reason,
                    });
                    continue;
                }
                Screened::Refund(memo, reason) => (memo, Some(reason)),
                Screened::Candidate(memo) if note.value != p.stake_zat => (
                    memo,
                    Some(RefundReason::WrongValue {
                        expected: p.stake_zat,
                    }),
                ),
                Screened::Candidate(memo) => (memo, None),
            };
            let aad = memo.aad(&Self::FORMAT);
            match reason {
                None => out.valid.push(Valid {
                    note: note.clone(),
                    entry: memo.payload,
                    sealed: memo.sealed_receiver,
                    aad,
                }),
                Some(_) if note.value <= fees => out.forfeits.push(Forfeit {
                    note: note.clone(),
                    reason: ForfeitReason::TooSmallToRefund { fees },
                }),
                Some(reason) => out.refunds.push(Refund {
                    note: note.clone(),
                    sealed: memo.sealed_receiver,
                    aad,
                    reason,
                }),
            }
        }
        out
    }

    fn settle(
        &self,
        p: &FlipParams,
        c: &Classified<Side>,
        carry_in: u64,
        draw: Option<&Randomness>,
    ) -> Result<Settlement<Flip>, RulesError> {
        let of = |o: Option<u64>| o.ok_or(RulesError::Overflow);
        let mut payouts = Vec::new();
        let mut inputs: Vec<InputRef> = Vec::new();
        let mut carry = carry_in;
        let mut refund = |note: &ReceivedNote,
                          sealed,
                          aad: &Vec<u8>,
                          reason: RefundReason,
                          payouts: &mut Vec<Payout>| {
            inputs.push(note.into());
            let gross = note.value - p.entry_fee_zat;
            payouts.push(Payout {
                sealed,
                aad: aad.clone(),
                gross,
                fee: p.output_fee_zat,
                net: gross - p.output_fee_zat,
                kind: PayoutKind::Refund { reason },
            });
        };
        for r in &c.refunds {
            refund(
                &r.note,
                r.sealed.clone(),
                &r.aad,
                r.reason.clone(),
                &mut payouts,
            );
        }
        let side = match draw {
            None => {
                for v in &c.valid {
                    refund(
                        &v.note,
                        v.sealed.clone(),
                        &v.aad,
                        RefundReason::RoundVoided,
                        &mut payouts,
                    );
                }
                None
            }
            Some(randomness) => Some(
                uniform_below(DOMAIN, c.round, randomness, 2)
                    .map_err(|e| RulesError::Invalid(e.to_string()))? as u8,
            ),
        };
        let mut winners = 0;
        if let Some(side) = side {
            let pot = of(c
                .valid
                .iter()
                .try_fold(carry, |a, v| a.checked_add(v.note.value - p.entry_fee_zat)))?;
            carry = pot;
            let won: Vec<&Valid<Side>> = c.valid.iter().filter(|v| v.entry == Side(side)).collect();
            inputs.extend(c.valid.iter().map(|v| InputRef::from(&v.note)));
            let share = if won.is_empty() {
                0
            } else {
                pot / won.len() as u64
            };
            if share > p.output_fee_zat {
                winners = won.len();
                carry = pot - share * won.len() as u64;
                for v in won {
                    payouts.push(Payout {
                        sealed: v.sealed.clone(),
                        aad: v.aad.clone(),
                        gross: share,
                        fee: p.output_fee_zat,
                        net: share - p.output_fee_zat,
                        kind: PayoutKind::Win,
                    });
                }
            }
        }
        carry = of(c
            .forfeits
            .iter()
            .try_fold(carry, |a, f| a.checked_add(f.note.value)))?;
        inputs.sort();
        let paid = payouts.iter().map(|o| o.net).sum();
        let fees = p.entry_fee_zat * inputs.len() as u64 + p.output_fee_zat * payouts.len() as u64;
        Ok(Settlement {
            round: c.round,
            voided: draw.is_none(),
            outcome: Flip { side, winners },
            received: of(c.received())?,
            carry_in,
            payouts,
            payout_inputs: inputs,
            paid,
            fees,
            carry_out: carry,
        })
    }

    fn board(&self, _p: &FlipParams, c: &Classified<Side>) -> serde_json::Value {
        let heads = c.valid.iter().filter(|v| v.entry == Side(0)).count();
        serde_json::json!({ "heads": heads, "tails": c.valid.len() - heads })
    }
}

// ---- tests ----

const P: FlipParams = FlipParams {
    stake_zat: 100_000,
    entry_fee_zat: 5_000,
    output_fee_zat: 5_000,
};

fn chain() -> ChainParams {
    ChainParams::testnet(1_000)
}

fn info(round: u32, voided: bool) -> RoundInfo {
    RoundInfo {
        round,
        window: round_window(&chain(), round).expect("window"),
        voided,
    }
}

fn entry(round: u32, side: u8, value: u64, height: u32, i: u16, rng: &mut StdRng) -> ReceivedNote {
    let (_, pk) = generate_sealing_keypair(rng);
    let receiver = PayoutReceiver {
        network: Network::Test,
        orchard: [i as u8; 43],
    };
    let memo =
        EntryMemo::seal(&CoinFlip::FORMAT, round, Side(side), &receiver, &pk, rng).expect("seal");
    ReceivedNote {
        txid: format!("{i:064x}"),
        output_index: 0,
        height,
        value,
        memo: memo.encode(&CoinFlip::FORMAT).to_vec(),
    }
}

fn notes(rng: &mut StdRng) -> Vec<ReceivedNote> {
    let start = info(3, false).window.start;
    vec![
        entry(3, 0, 100_000, start, 1, rng),
        entry(3, 1, 100_000, start + 1, 2, rng),
        entry(3, 1, 100_000, start + 2, 3, rng),
        entry(4, 0, 100_000, start + 2, 4, rng), // wrong round: refund
        entry(3, 0, 70_000, start + 3, 5, rng),  // wrong value: refund
        entry(3, 0, 8_000, start + 3, 6, rng),   // cannot pay its refund fees: kept
        ReceivedNote {
            txid: format!("{:064x}", 7),
            output_index: 0,
            height: start,
            value: 20_000,
            memo: vec![0xF6],
        },
    ]
}

#[test]
fn classification_screens_then_applies_the_stake() {
    let mut rng = StdRng::seed_from_u64(1);
    let c = CoinFlip.classify(&P, &info(3, false), &notes(&mut rng));
    assert_eq!(
        (c.valid.len(), c.refunds.len(), c.forfeits.len()),
        (3, 2, 2)
    );
    assert_eq!(
        c.refunds[0].reason,
        RefundReason::WrongRound { memo_round: 4 }
    );
    assert_eq!(
        c.refunds[1].reason,
        RefundReason::WrongValue { expected: 100_000 }
    );
    assert_eq!(
        CoinFlip.board(&P, &c),
        serde_json::json!({ "heads": 1, "tails": 2 })
    );
}

#[test]
fn either_side_settles_and_conserves() {
    let mut rng = StdRng::seed_from_u64(2);
    let c = CoinFlip.classify(&P, &info(3, false), &notes(&mut rng));
    let mut sides = [false; 2];
    for seed in 0..40u8 {
        let s = settle_checked(&CoinFlip, &P, &c, 12_345, Some(&Randomness([seed; 32])))
            .expect("settles");
        let side = s.outcome.side.expect("drawn") as usize;
        sides[side] = true;
        let wins = s
            .payouts
            .iter()
            .filter(|o| o.kind == PayoutKind::Win)
            .count();
        assert_eq!(wins, if side == 0 { 1 } else { 2 });
        assert_eq!(
            s.payout_inputs.len(),
            5,
            "three entries and two refunds are spent"
        );
        assert_eq!(s.erase().expect("erase").outcome["winners"], wins);
    }
    assert_eq!(sides, [true, true], "both sides come up");
}

#[test]
fn voided_round_refunds_every_entry() {
    let mut rng = StdRng::seed_from_u64(3);
    let c = CoinFlip.classify(&P, &info(3, true), &notes(&mut rng));
    assert_eq!(
        (c.valid.len(), c.refunds.len()),
        (0, 5),
        "voided: every decodable entry refunds"
    );
    let s = settle_checked(&CoinFlip, &P, &c, 777, None).expect("settles");
    assert_eq!(s.payouts.len(), 5);
    assert!(s
        .payouts
        .iter()
        .all(|o| matches!(o.kind, PayoutKind::Refund { .. })));
    assert_eq!(
        s.carry_out,
        777 + 20_000 + 8_000,
        "carry plus the kept notes"
    );
    assert_eq!(
        settle_checked(&CoinFlip, &P, &c, 0, Some(&Randomness([0; 32]))),
        Err(SettlementError::DrawMismatch)
    );
}

#[test]
fn an_empty_round_carries_everything() {
    let c = CoinFlip.classify(&P, &info(9, false), &[]);
    let s =
        settle_checked(&CoinFlip, &P, &c, 5_000_000, Some(&Randomness([1; 32]))).expect("settles");
    assert!(s.payouts.is_empty());
    assert_eq!(s.carry_out, 5_000_000);
}

/// The kit catches a broken game, whatever the game: overpaying, paying a
/// receiver no entry sealed, paying an entry twice, or a win from a refund.
#[test]
fn generic_checks_reject_a_broken_settlement() {
    let mut rng = StdRng::seed_from_u64(4);
    let c = CoinFlip.classify(&P, &info(3, false), &notes(&mut rng));
    let good = CoinFlip
        .settle(&P, &c, 0, Some(&Randomness([5; 32])))
        .expect("settle");
    let check = |s: &Settlement<Flip>| zecraft_core::rules::check_settlement(&c, s);
    check(&good).expect("the honest settlement passes");

    let mut overpay = good.clone();
    overpay.payouts[0].gross += 1;
    overpay.payouts[0].net += 1;
    overpay.paid += 1;
    assert!(matches!(
        check(&overpay),
        Err(SettlementError::Conservation { .. })
    ));

    let mut stranger = good.clone();
    stranger.payouts[0].aad[0] ^= 1;
    assert_eq!(
        check(&stranger),
        Err(SettlementError::UnknownReceiver { index: 0 })
    );

    let mut twice = good.clone();
    let first = twice.payouts[0].clone();
    twice.payouts.push(first);
    assert_eq!(
        check(&twice),
        Err(SettlementError::EntryPaidTwice {
            index: good.payouts.len()
        })
    );

    let mut win_refund = good.clone();
    win_refund.payouts[0].kind = PayoutKind::Win;
    assert_eq!(
        check(&win_refund),
        Err(SettlementError::WinFromRefund { index: 0 })
    );

    let mut unsorted = good.clone();
    unsorted.payout_inputs.reverse();
    assert_eq!(check(&unsorted), Err(SettlementError::InputOrder));

    let mut no_net = good;
    no_net.payouts[0].net = 0;
    assert!(matches!(
        check(&no_net),
        Err(SettlementError::PayoutArithmetic { index: 0, .. })
    ));
}
