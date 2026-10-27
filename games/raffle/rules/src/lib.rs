//! `zecraft-raffle`: the Zecraft example game, a provably fair raffle on
//! shielded Zcash testnet (docs/plan.md, section 3).
//!
//! # Rules
//!
//! * **Buying tickets.** A player pays exactly `tickets × ticket_price_zat`
//!   to the vault with a [`FORMAT`] memo (magic `ZRFL`, text prefix
//!   `zrfl1:`) whose payload is the ticket count ([`Tickets`], `u16`) and
//!   whose sealed receiver is a fresh address of the player's wallet.
//! * **Classification.** Inside the round's window, naming this round, with
//!   at least one ticket and the exact value: valid. A decodable entry with
//!   another round, outside the window, zero tickets, the wrong value, or in
//!   a voided round: refunded. An undecodable memo, or a refund that cannot
//!   pay its own fees (`value ≤ entry_fee + output_fee`): kept by the vault
//!   and added to the carry.
//! * **Draw.** Valid entries are ordered by `(height, txid, action)`. Entry
//!   `i` owns tickets `[sᵢ, sᵢ + tᵢ)` where `sᵢ` is the sum of the tickets
//!   before it. The winning ticket is
//!   `uniform_below("zecraft/raffle/v1", round, beacon, T)` with `T` the
//!   tickets sold.
//! * **Settlement.** Every valid entry pays `entry_fee_zat` for the action
//!   that spends its note. The winner gets
//!   `Σ valid − entry fees + carry_in − output_fee` in one output. There is
//!   no rake. A round with no tickets carries everything over. A refund pays
//!   back `value − entry_fee − output_fee`. Everything the vault keeps
//!   (forfeits, and a winning amount too small to pay its output fee) goes to
//!   the carry.
//!
//! The timing rule, the reveal delay and settlement after the window plus
//! the post-close blocks are the kit's (see `zecraft-core`).

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use zecraft_core::draw::{uniform_below, Randomness};
use zecraft_core::entry::{MemoFormat, Payload, PayloadError};
use zecraft_core::params::{ChainParams, ZIP317_MARGINAL_FEE_ZAT};
use zecraft_core::rules::{
    screen, Classified, Forfeit, ForfeitReason, InputRef, Payout, PayoutKind, ReceivedNote, Refund,
    RefundReason, RoundInfo, Rules, RulesError, Screened, Settlement, Valid,
};
use zecraft_core::COIN;

/// The raffle's memo: `0xFF | "ZRFL" | 1 | round | tickets u16 BE | sealed receiver`.
pub const FORMAT: MemoFormat = MemoFormat {
    magic: *b"ZRFL",
    version: 1,
    text_prefix: "zrfl1:",
    seal_info: b"zecraft/raffle/payout-seal/v1",
};

/// Domain of the winning-ticket draw.
pub const DRAW_DOMAIN: &[u8] = b"zecraft/raffle/v1";

/// Default ticket price: 0.001 TAZ.
pub const DEFAULT_TICKET_PRICE_ZAT: u64 = COIN / 1000;

/// The payload: how many tickets the entry buys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tickets(pub u16);

impl Payload for Tickets {
    const LEN: usize = 2;
    fn to_bytes(&self) -> Vec<u8> {
        self.0.to_be_bytes().to_vec()
    }
    fn from_bytes(bytes: &[u8]) -> Result<Self, PayloadError> {
        <[u8; 2]>::try_from(bytes)
            .map(|b| Tickets(u16::from_be_bytes(b)))
            .map_err(|_| PayloadError("tickets are two bytes".into()))
    }
}

/// Raffle parameters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaffleParams {
    /// Price of one ticket.
    pub ticket_price_zat: u64,
    /// Charged to every spent entry note (the action that spends it).
    pub entry_fee_zat: u64,
    /// Charged to every payout or refund output.
    pub output_fee_zat: u64,
}

impl Default for RaffleParams {
    fn default() -> Self {
        RaffleParams {
            ticket_price_zat: DEFAULT_TICKET_PRICE_ZAT,
            entry_fee_zat: ZIP317_MARGINAL_FEE_ZAT,
            output_fee_zat: ZIP317_MARGINAL_FEE_ZAT,
        }
    }
}

impl RaffleParams {
    /// The fees a refund must cover: its input and its output.
    pub fn refund_fees(&self) -> Option<u64> {
        self.entry_fee_zat.checked_add(self.output_fee_zat)
    }

    /// The exact value of an entry of `tickets` tickets.
    pub fn price_of(&self, tickets: Tickets) -> Option<u64> {
        self.ticket_price_zat.checked_mul(u64::from(tickets.0))
    }
}

/// The winning entry of a drawn round.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Winner {
    /// Index of the entry among the round's valid entries (in draw order).
    pub entry_index: usize,
    pub txid: String,
    pub output_index: u16,
    /// First ticket the entry owns, and how many.
    pub first_ticket: u32,
    pub tickets: u16,
}

/// The raffle's outcome of a round.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaffleOutcome {
    /// Tickets sold (valid entries only).
    pub tickets_sold: u32,
    /// The drawn ticket; `None` when voided or nothing was sold.
    pub winning_ticket: Option<u32>,
    pub winner: Option<Winner>,
    /// Whether the winner's output was paid (false when the prize could not
    /// pay its output fee and carried over instead).
    pub winner_paid: bool,
}

/// The raffle game.
#[derive(Clone, Copy, Debug, Default)]
pub struct Raffle;

/// Valid entries in draw order, with each entry's first ticket.
pub fn ticket_ranges(valid: &[Valid<Tickets>]) -> Result<Vec<(usize, u32)>, RulesError> {
    let mut order: Vec<usize> = (0..valid.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&valid[a].note, &valid[b].note);
        (x.height, &x.txid, x.output_index).cmp(&(y.height, &y.txid, y.output_index))
    });
    let mut next = 0u32;
    let mut out = Vec::with_capacity(order.len());
    for i in order {
        out.push((i, next));
        next = next
            .checked_add(u32::from(valid[i].entry.0))
            .ok_or(RulesError::Overflow)?;
    }
    Ok(out)
}

/// The entry (index into `valid`) owning ticket `ticket`, with its first ticket.
pub fn owner_of(valid: &[Valid<Tickets>], ticket: u32) -> Result<Option<(usize, u32)>, RulesError> {
    Ok(ticket_ranges(valid)?.into_iter().find(|&(i, first)| {
        ticket >= first && u64::from(ticket) < u64::from(first) + u64::from(valid[i].entry.0)
    }))
}

fn sum(mut values: impl Iterator<Item = u64>) -> Result<u64, RulesError> {
    values.try_fold(0u64, |a, v| a.checked_add(v).ok_or(RulesError::Overflow))
}

fn sub(a: u64, b: u64) -> Result<u64, RulesError> {
    a.checked_sub(b).ok_or(RulesError::Overflow)
}

impl Rules for Raffle {
    const FORMAT: MemoFormat = FORMAT;
    type Params = RaffleParams;
    type Entry = Tickets;
    type Outcome = RaffleOutcome;

    fn classify(
        &self,
        p: &RaffleParams,
        round: &RoundInfo,
        notes: &[ReceivedNote],
    ) -> Classified<Tickets> {
        let mut out = Classified::empty(round);
        let refund_fees = p.refund_fees().unwrap_or(u64::MAX);
        for note in notes {
            let (memo, reason) = match screen::<Tickets>(&FORMAT, round, note) {
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
                Screened::Candidate(memo) => {
                    let reason = if memo.payload.0 == 0 {
                        Some(RefundReason::InvalidEntry)
                    } else {
                        match p.price_of(memo.payload) {
                            Some(price) if price == note.value => None,
                            expected => Some(RefundReason::WrongValue {
                                expected: expected.unwrap_or(u64::MAX),
                            }),
                        }
                    };
                    (memo, reason)
                }
            };
            let aad = memo.aad(&FORMAT);
            match reason {
                None => out.valid.push(Valid {
                    note: note.clone(),
                    entry: memo.payload,
                    sealed: memo.sealed_receiver,
                    aad,
                }),
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

    fn settle(
        &self,
        p: &RaffleParams,
        c: &Classified<Tickets>,
        carry_in: u64,
        draw: Option<&Randomness>,
    ) -> Result<Settlement<RaffleOutcome>, RulesError> {
        let mut payouts = Vec::new();
        let mut inputs: Vec<InputRef> = Vec::new();
        let mut carry = carry_in;

        let tickets_sold = sum(c.valid.iter().map(|v| u64::from(v.entry.0)))?;
        let tickets_sold = u32::try_from(tickets_sold).map_err(|_| RulesError::Overflow)?;
        let mut outcome = RaffleOutcome {
            tickets_sold,
            winning_ticket: None,
            winner: None,
            winner_paid: false,
        };

        match draw {
            // Classification already refunds every decodable entry of a
            // voided round; a valid entry here would be a classification bug.
            None if !c.valid.is_empty() => {
                return Err(RulesError::Invalid(
                    "a voided round has valid entries".into(),
                ))
            }
            None => {}
            Some(_) if tickets_sold == 0 => {}
            Some(randomness) => {
                let ticket = uniform_below(DRAW_DOMAIN, c.round, randomness, tickets_sold)
                    .map_err(|e| RulesError::Invalid(e.to_string()))?;
                let (index, first) = owner_of(&c.valid, ticket)?
                    .ok_or_else(|| RulesError::Invalid("no entry owns the drawn ticket".into()))?;
                let winner = &c.valid[index];
                let entry_fees = p
                    .entry_fee_zat
                    .checked_mul(c.valid.len() as u64)
                    .ok_or(RulesError::Overflow)?;
                let pot = sub(sum(c.valid.iter().map(|v| v.note.value))?, entry_fees)?;
                let gross = pot.checked_add(carry_in).ok_or(RulesError::Overflow)?;
                inputs.extend(c.valid.iter().map(|v| InputRef::from(&v.note)));
                if gross > p.output_fee_zat {
                    payouts.push(Payout {
                        sealed: winner.sealed.clone(),
                        aad: winner.aad.clone(),
                        gross,
                        fee: p.output_fee_zat,
                        net: gross - p.output_fee_zat,
                        kind: PayoutKind::Win,
                    });
                    carry = 0;
                    outcome.winner_paid = true;
                } else {
                    carry = gross;
                }
                outcome.winning_ticket = Some(ticket);
                outcome.winner = Some(Winner {
                    entry_index: index,
                    txid: winner.note.txid.clone(),
                    output_index: winner.note.output_index,
                    first_ticket: first,
                    tickets: winner.entry.0,
                });
            }
        }

        for r in &c.refunds {
            // Classification only refunds notes that cover both fees.
            let gross = sub(r.note.value, p.entry_fee_zat)?;
            let net = sub(gross, p.output_fee_zat)?;
            if net == 0 {
                return Err(RulesError::Invalid(
                    "a refund cannot pay its own fees".into(),
                ));
            }
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

    fn board(&self, p: &RaffleParams, c: &Classified<Tickets>) -> serde_json::Value {
        let tickets: u64 = c.valid.iter().map(|v| u64::from(v.entry.0)).sum();
        let gross: u64 = c.valid.iter().map(|v| v.note.value).sum();
        let pot = gross.saturating_sub(p.entry_fee_zat.saturating_mul(c.valid.len() as u64));
        serde_json::json!({
            "game": "raffle",
            "ticket_price_zat": p.ticket_price_zat,
            "tickets_sold": tickets,
            "entries": c.valid.len(),
            "pot_zat": pot,
            "refunds": c.refunds.len(),
        })
    }

    fn validate(&self, chain: &ChainParams, p: &RaffleParams) -> Result<(), String> {
        let fees = p.refund_fees().ok_or("fees overflow")?;
        if p.ticket_price_zat <= fees {
            return Err(format!(
                "ticket_price_zat {} must exceed entry_fee_zat + output_fee_zat ({fees}) so a one-ticket refund pays out",
                p.ticket_price_zat
            ));
        }
        if p.entry_fee_zat < chain.marginal_fee_zat || p.output_fee_zat < chain.marginal_fee_zat {
            return Err(format!(
                "entry and output fees must be at least the chain's ZIP 317 marginal fee ({}) so the rules pay for the payout transaction",
                chain.marginal_fee_zat
            ));
        }
        Ok(())
    }
}
