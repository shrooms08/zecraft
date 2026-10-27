//! `zecraft-coinflip`: a coin flip on shielded Zcash, built with Zecraft.
//!
//! Players stake exactly `stake_zat` on heads or tails. When the round
//! closes, one draw from a drand beacon published afterwards picks a side;
//! everyone who picked it splits the pot (the stakes after their entry fees,
//! plus whatever earlier rounds carried over) equally. Nobody on the winning
//! side: everything carries to the next round.
//!
//! This is the smallest complete Zecraft game, and the template
//! `zecraft new` copies. Lines marked `YOUR GAME:` are the ones a new game
//! usually changes.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use zecraft_core::draw::{uniform_below, Randomness};
use zecraft_core::entry::{MemoFormat, Payload, PayloadError};
use zecraft_core::params::{ChainParams, ZIP317_MARGINAL_FEE_ZAT};
use zecraft_core::rules::{
    screen, Classified, Forfeit, ForfeitReason, InputRef, Payout, PayoutKind, ReceivedNote, Refund,
    RefundReason, RoundInfo, Rules, RulesError, Screened, Settlement, Valid,
};

/// The memo every entry carries: `0xFF | magic | version | round | payload |
/// sealed payout address`. YOUR GAME: pick your own 4-byte magic and prefix.
pub const FORMAT: MemoFormat = MemoFormat {
    magic: *b"ZCFL",
    version: 1,
    text_prefix: "zcfl1:",
    seal_info: b"zecraft/coinflip/payout-seal/v1",
};

/// Domain of the draw. YOUR GAME: one domain per independent draw.
pub const DRAW_DOMAIN: &[u8] = b"zecraft/coinflip/v1";

/// What one entry plays: the side it picks. YOUR GAME: your payload (a
/// fixed number of bytes, decoded strictly).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Side(pub u8);

impl Side {
    pub const HEADS: Side = Side(0);
    pub const TAILS: Side = Side(1);
}

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

/// Game parameters, published in the game config. YOUR GAME: your knobs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinFlipParams {
    /// The exact amount of one entry.
    pub stake_zat: u64,
    /// Charged to every entry note the payout transaction spends.
    pub entry_fee_zat: u64,
    /// Charged to every payout or refund output.
    pub output_fee_zat: u64,
}

impl Default for CoinFlipParams {
    /// Testnet defaults: 0.001 TAZ per entry, ZIP 317 fees.
    fn default() -> Self {
        CoinFlipParams {
            stake_zat: 100_000,
            entry_fee_zat: ZIP317_MARGINAL_FEE_ZAT,
            output_fee_zat: ZIP317_MARGINAL_FEE_ZAT,
        }
    }
}

/// The round's outcome, published in the settlement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlipOutcome {
    /// The drawn side; `None` when the round was voided.
    pub side: Option<u8>,
    /// Valid entries.
    pub entries: u32,
    /// Entries on the drawn side that were paid.
    pub winners: u32,
    /// What each winner received before the output fee.
    pub share_zat: u64,
}

/// The coin flip.
#[derive(Clone, Copy, Debug, Default)]
pub struct CoinFlip;

fn sum(mut values: impl Iterator<Item = u64>) -> Result<u64, RulesError> {
    values.try_fold(0u64, |a, v| a.checked_add(v).ok_or(RulesError::Overflow))
}

fn sub(a: u64, b: u64) -> Result<u64, RulesError> {
    a.checked_sub(b).ok_or(RulesError::Overflow)
}

impl Rules for CoinFlip {
    const FORMAT: MemoFormat = FORMAT;
    type Params = CoinFlipParams;
    type Entry = Side;
    type Outcome = FlipOutcome;

    /// Sorts every note of the round into valid entries, refunds and kept
    /// notes. `screen` already handled decoding, voided rounds, the window
    /// and the round number; the game adds its own rule (the exact stake).
    fn classify(
        &self,
        p: &CoinFlipParams,
        round: &RoundInfo,
        notes: &[ReceivedNote],
    ) -> Classified<Side> {
        let mut out = Classified::empty(round);
        let refund_fees = p.entry_fee_zat.saturating_add(p.output_fee_zat);
        for note in notes {
            let (memo, reason) = match screen::<Side>(&FORMAT, round, note) {
                Screened::Undecodable(e) => {
                    out.forfeits.push(Forfeit {
                        note: note.clone(),
                        reason: ForfeitReason::UndecodableMemo {
                            error: e.to_string(),
                        },
                    });
                    continue;
                }
                Screened::Refund(memo, reason) => (memo, Some(reason)),
                // YOUR GAME: what makes a decodable entry of this round valid.
                Screened::Candidate(memo) if note.value != p.stake_zat => (
                    memo,
                    Some(RefundReason::WrongValue {
                        expected: p.stake_zat,
                    }),
                ),
                Screened::Candidate(memo) => (memo, None),
            };
            let aad = memo.aad(&FORMAT);
            match reason {
                None => out.valid.push(Valid {
                    note: note.clone(),
                    entry: memo.payload,
                    sealed: memo.sealed_receiver,
                    aad,
                }),
                // A refund that cannot pay its own two fees is kept.
                Some(_) if note.value <= refund_fees => out.forfeits.push(Forfeit {
                    note: note.clone(),
                    reason: ForfeitReason::TooSmallToRefund { fees: refund_fees },
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

    /// Turns a classified round into payouts. The kit checks the result
    /// (conservation, receivers, inputs) whatever this returns.
    fn settle(
        &self,
        p: &CoinFlipParams,
        c: &Classified<Side>,
        carry_in: u64,
        draw: Option<&Randomness>,
    ) -> Result<Settlement<FlipOutcome>, RulesError> {
        let mut payouts = Vec::new();
        let mut inputs: Vec<InputRef> = Vec::new();
        let mut carry = carry_in;
        let mut outcome = FlipOutcome {
            side: None,
            entries: u32::try_from(c.valid.len()).map_err(|_| RulesError::Overflow)?,
            winners: 0,
            share_zat: 0,
        };

        // Refunds: the value back, minus the entry fee and the output fee.
        for r in &c.refunds {
            let gross = sub(r.note.value, p.entry_fee_zat)?;
            let net = sub(gross, p.output_fee_zat)?;
            inputs.push(InputRef::from(&r.note));
            payouts.push(Payout {
                sealed: r.sealed.clone(),
                aad: r.aad.clone(),
                gross,
                fee: p.output_fee_zat,
                net,
                kind: PayoutKind::Refund {
                    reason: r.reason.clone(),
                },
            });
        }

        match draw {
            // A voided round: classification refunded every entry already.
            None if !c.valid.is_empty() => {
                return Err(RulesError::Invalid(
                    "a voided round has valid entries".into(),
                ))
            }
            None => {}
            Some(randomness) => {
                // YOUR GAME: the draw and who wins it.
                let side = uniform_below(DRAW_DOMAIN, c.round, randomness, 2)
                    .map_err(|e| RulesError::Invalid(e.to_string()))?;
                let side = Side(u8::try_from(side).map_err(|_| RulesError::Overflow)?);
                outcome.side = Some(side.0);
                inputs.extend(c.valid.iter().map(|v| InputRef::from(&v.note)));
                let stakes = sum(c.valid.iter().map(|v| v.note.value))?;
                let fees = p
                    .entry_fee_zat
                    .checked_mul(c.valid.len() as u64)
                    .ok_or(RulesError::Overflow)?;
                let pot = sub(stakes, fees)?
                    .checked_add(carry_in)
                    .ok_or(RulesError::Overflow)?;
                let winners: Vec<&Valid<Side>> =
                    c.valid.iter().filter(|v| v.entry == side).collect();
                let share = if winners.is_empty() {
                    0
                } else {
                    pot / winners.len() as u64
                };
                if share > p.output_fee_zat {
                    for w in &winners {
                        payouts.push(Payout {
                            sealed: w.sealed.clone(),
                            aad: w.aad.clone(),
                            gross: share,
                            fee: p.output_fee_zat,
                            net: share - p.output_fee_zat,
                            kind: PayoutKind::Win,
                        });
                    }
                    outcome.winners =
                        u32::try_from(winners.len()).map_err(|_| RulesError::Overflow)?;
                    outcome.share_zat = share;
                    // The rounding dust carries over.
                    carry = pot - share * winners.len() as u64;
                } else {
                    carry = pot;
                }
            }
        }

        // Kept notes (undecodable, or too small to refund) carry over.
        carry = carry
            .checked_add(sum(c.forfeits.iter().map(|f| f.note.value))?)
            .ok_or(RulesError::Overflow)?;
        inputs.sort();
        let fees = p
            .entry_fee_zat
            .checked_mul(inputs.len() as u64)
            .and_then(|i| {
                p.output_fee_zat
                    .checked_mul(payouts.len() as u64)
                    .and_then(|o| i.checked_add(o))
            })
            .ok_or(RulesError::Overflow)?;
        Ok(Settlement {
            round: c.round,
            voided: c.voided,
            outcome,
            received: c.received().ok_or(RulesError::Overflow)?,
            carry_in,
            paid: sum(payouts.iter().map(|o| o.net))?,
            payouts,
            payout_inputs: inputs,
            fees,
            carry_out: carry,
        })
    }

    /// The public view of a round so far. YOUR GAME: whatever your page shows.
    fn board(&self, p: &CoinFlipParams, c: &Classified<Side>) -> serde_json::Value {
        let heads = c.valid.iter().filter(|v| v.entry == Side::HEADS).count();
        let stakes: u64 = c.valid.iter().map(|v| v.note.value).sum();
        serde_json::json!({
            "game": "coinflip",
            "stake_zat": p.stake_zat,
            "entries": c.valid.len(),
            "heads": heads,
            "tails": c.valid.len() - heads,
            "pot_zat": stakes.saturating_sub(p.entry_fee_zat.saturating_mul(c.valid.len() as u64)),
        })
    }

    /// Checked at startup.
    fn validate(&self, chain: &ChainParams, p: &CoinFlipParams) -> Result<(), String> {
        let fees = p.entry_fee_zat.saturating_add(p.output_fee_zat);
        if p.stake_zat <= fees {
            return Err(format!(
                "stake_zat {} must exceed entry_fee_zat + output_fee_zat ({fees})",
                p.stake_zat
            ));
        }
        if p.entry_fee_zat < chain.marginal_fee_zat || p.output_fee_zat < chain.marginal_fee_zat {
            return Err(format!(
                "entry and output fees must be at least the ZIP 317 marginal fee ({})",
                chain.marginal_fee_zat
            ));
        }
        Ok(())
    }
}
