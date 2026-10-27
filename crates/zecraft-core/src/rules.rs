//! The rules seam: the only place a game plugs into the kit.
//!
//! A game implements [`Rules`]: how a round's received notes are
//! [`classify`](Rules::classify)'d into valid entries, refunds and
//! forfeits, how a classified round is [`settle`](Rules::settle)d into
//! payouts once its beacon is known, and what its public
//! [`board`](Rules::board) shows. Everything else (following the chain,
//! the timing rule, beacons, the payout transaction, keyholder checks and
//! public verification) is generic and reads only the [`Settlement`].
//!
//! The kit never trusts a game's settlement blindly: [`settle_checked`]
//! runs [`check_settlement`] on every result, so the engine, every keyholder
//! and every verifier enforce the same invariants whatever the game:
//!
//! ```text
//! received + carry_in == paid + fees + carry_out          (conservation)
//! ```
//!
//! plus: every payout is `gross = fee + net` with `net > 0`; it pays a
//! receiver sealed by an entry of this round, at most once per entry; a
//! voided round pays refunds only; the payout inputs are notes of this
//! round, in canonical order. A violation is an error, never a panic.

use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::draw::{Randomness, RoundWindow};
use crate::entry::{EntryMemo, MemoError, MemoFormat, Payload, SealedReceiver};
use crate::params::ChainParams;

/// A shielded note the vault received, as the wallet reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedNote {
    /// Transaction id (display hex).
    pub txid: String,
    /// Action index within the transaction.
    pub output_index: u16,
    /// Block height the note was mined at.
    pub height: u32,
    /// Note value in zatoshis.
    pub value: u64,
    /// The 512-byte memo field.
    #[serde(with = "hex_bytes")]
    pub memo: Vec<u8>,
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        hex::decode(text).map_err(serde::de::Error::custom)
    }
}

/// The round being classified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundInfo {
    pub round: u32,
    pub window: RoundWindow,
    /// The timing rule failed: nothing is drawn, every decodable entry is refunded.
    pub voided: bool,
}

/// Why an entry is refunded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum RefundReason {
    /// The memo names a different round.
    WrongRound { memo_round: u32 },
    /// The note was mined outside the round's window.
    OutOfWindow,
    /// The value is not what the entry's payload requires.
    WrongValue { expected: u64 },
    /// The payload decodes but the game does not accept it (for example
    /// zero tickets).
    InvalidEntry,
    /// The round was voided by the timing rule.
    RoundVoided,
}

/// Why a note is kept by the vault (added to the carry).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ForfeitReason {
    /// The memo is not a decodable entry of this game: there is no sealed
    /// receiver to refund to.
    UndecodableMemo { error: String },
    /// The value cannot pay the fees of refunding it.
    TooSmallToRefund { fees: u64 },
}

/// An entry that takes part in the draw.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Valid<E> {
    pub note: ReceivedNote,
    pub entry: E,
    pub sealed: SealedReceiver,
    /// The AAD the sealed receiver is bound to (the memo header).
    #[serde(with = "hex_bytes")]
    pub aad: Vec<u8>,
}

/// An entry returned to its player.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refund {
    pub note: ReceivedNote,
    pub sealed: SealedReceiver,
    #[serde(with = "hex_bytes")]
    pub aad: Vec<u8>,
    pub reason: RefundReason,
}

/// A note the vault keeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forfeit {
    pub note: ReceivedNote,
    pub reason: ForfeitReason,
}

/// Every note of a round, classified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classified<E> {
    pub round: u32,
    pub voided: bool,
    pub valid: Vec<Valid<E>>,
    pub refunds: Vec<Refund>,
    pub forfeits: Vec<Forfeit>,
}

impl<E> Classified<E> {
    /// An empty classification of `round`.
    pub fn empty(round: &RoundInfo) -> Self {
        Classified {
            round: round.round,
            voided: round.voided,
            valid: Vec::new(),
            refunds: Vec::new(),
            forfeits: Vec::new(),
        }
    }

    /// Every classified note.
    pub fn notes(&self) -> impl Iterator<Item = &ReceivedNote> {
        self.valid
            .iter()
            .map(|v| &v.note)
            .chain(self.refunds.iter().map(|r| &r.note))
            .chain(self.forfeits.iter().map(|f| &f.note))
    }

    /// Sum of every classified note: what the round received.
    pub fn received(&self) -> Option<u64> {
        self.notes()
            .try_fold(0u64, |acc, n| acc.checked_add(n.value))
    }
}

/// What the generic part of classification decided about one note; the
/// game then applies its own value rules to the candidates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screened<P> {
    /// Not an entry of this game.
    Undecodable(MemoError),
    /// A decodable entry that cannot play: voided round, outside the window
    /// or another round.
    Refund(EntryMemo<P>, RefundReason),
    /// A decodable entry of this round, inside the window.
    Candidate(EntryMemo<P>),
}

/// The game-independent first step of classification: decode the memo
/// (binary or text) under `format`, then apply the voided, window and round
/// rules, in that order.
pub fn screen<P: Payload>(
    format: &MemoFormat,
    round: &RoundInfo,
    note: &ReceivedNote,
) -> Screened<P> {
    let memo = match EntryMemo::<P>::decode_any(format, &note.memo) {
        Ok(memo) => memo,
        Err(e) => return Screened::Undecodable(e),
    };
    if round.voided {
        Screened::Refund(memo, RefundReason::RoundVoided)
    } else if !round.window.contains(note.height) {
        Screened::Refund(memo, RefundReason::OutOfWindow)
    } else if memo.round != round.round {
        let memo_round = memo.round;
        Screened::Refund(memo, RefundReason::WrongRound { memo_round })
    } else {
        Screened::Candidate(memo)
    }
}

/// Why an output is paid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PayoutKind {
    Win,
    Refund { reason: RefundReason },
}

/// One shielded output the vault must send.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payout {
    /// Sealed receiver of the entry being paid.
    pub sealed: SealedReceiver,
    /// AAD the sealed receiver is bound to (needed to open it).
    #[serde(with = "hex_bytes")]
    pub aad: Vec<u8>,
    /// Amount before the output fee.
    pub gross: u64,
    /// Output fee charged to this output.
    pub fee: u64,
    /// Amount actually sent.
    pub net: u64,
    pub kind: PayoutKind,
}

/// A note the payout transaction spends.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InputRef {
    pub height: u32,
    /// Display-hex txid.
    pub txid: String,
    pub output_index: u16,
    pub value: u64,
}

impl From<&ReceivedNote> for InputRef {
    fn from(n: &ReceivedNote) -> Self {
        InputRef {
            height: n.height,
            txid: n.txid.clone(),
            output_index: n.output_index,
            value: n.value,
        }
    }
}

/// The result of settling a round: what the coordinator pays, what the
/// keyholders check and what verifiers recompute. `O` is the game's own
/// outcome (a winning ticket, a coin side); the kit stores and compares it
/// as JSON (`Settlement<serde_json::Value>`, see [`Settlement::erase`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement<O = serde_json::Value> {
    pub round: u32,
    pub voided: bool,
    /// The game's outcome.
    pub outcome: O,
    /// Sum of every note the round received (valid, refunded, forfeited).
    pub received: u64,
    /// Value carried into this round from earlier rounds.
    pub carry_in: u64,
    /// Outputs to send.
    pub payouts: Vec<Payout>,
    /// Notes the payout transaction spends, in canonical order
    /// `(height, txid, output_index)`.
    pub payout_inputs: Vec<InputRef>,
    /// Sum of the payouts' net amounts.
    pub paid: u64,
    /// Every fee charged: per spent input plus per output.
    pub fees: u64,
    /// Value carried to the next round.
    pub carry_out: u64,
}

impl<O: Serialize> Settlement<O> {
    /// The same settlement with its outcome as JSON.
    pub fn erase(self) -> Result<Settlement, SettlementError> {
        Ok(Settlement {
            outcome: serde_json::to_value(&self.outcome)
                .map_err(|e| SettlementError::Outcome(e.to_string()))?,
            round: self.round,
            voided: self.voided,
            received: self.received,
            carry_in: self.carry_in,
            payouts: self.payouts,
            payout_inputs: self.payout_inputs,
            paid: self.paid,
            fees: self.fees,
            carry_out: self.carry_out,
        })
    }
}

/// Errors a game's rules report.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum RulesError {
    #[error("arithmetic overflow in the game's rules")]
    Overflow,
    #[error("{0}")]
    Invalid(String),
}

/// A settlement that breaks the kit's invariants, or could not be computed.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettlementError {
    #[error(transparent)]
    Rules(#[from] RulesError),
    #[error("a voided round must be settled without a draw, and a drawn round with one")]
    DrawMismatch,
    #[error("settlement is for round {got} (voided: {got_voided}), expected round {expected} (voided: {expected_voided})")]
    WrongRound {
        expected: u32,
        expected_voided: bool,
        got: u32,
        got_voided: bool,
    },
    #[error("settlement states {stated} zatoshis received, the round's notes sum to {actual}")]
    Received { stated: u64, actual: u64 },
    #[error("payout {index}: gross {gross} != fee {fee} + net {net}, or net is zero")]
    PayoutArithmetic {
        index: usize,
        gross: u64,
        fee: u64,
        net: u64,
    },
    #[error("payout {index} pays a receiver that no entry of this round sealed")]
    UnknownReceiver { index: usize },
    #[error("payout {index} pays an entry that is already paid")]
    EntryPaidTwice { index: usize },
    #[error("payout {index} is a win, but its entry was refunded or the round voided")]
    WinFromRefund { index: usize },
    #[error("settlement states {stated} zatoshis paid, the payouts sum to {actual}")]
    Paid { stated: u64, actual: u64 },
    #[error("fees {fees} are below the payouts' own output fees {output_fees}")]
    Fees { fees: u64, output_fees: u64 },
    #[error("payout input {txid}:{output_index} is not a refunded or valid note of this round")]
    UnknownInput { txid: String, output_index: u16 },
    #[error("payout inputs are not in strictly increasing (height, txid, output_index) order")]
    InputOrder,
    #[error("conservation violated: received {received} + carry_in {carry_in} != paid {paid} + fees {fees} + carry_out {carry_out}")]
    Conservation {
        received: u64,
        carry_in: u64,
        paid: u64,
        fees: u64,
        carry_out: u64,
    },
    #[error("arithmetic overflow while checking the settlement")]
    Overflow,
    #[error("the outcome does not serialize: {0}")]
    Outcome(String),
}

/// A game.
pub trait Rules: Send + Sync + 'static {
    /// The game's memo format: magic, version, text prefix and HPKE context.
    /// The kit's servers need it to open the receivers a settlement pays.
    const FORMAT: MemoFormat;
    /// Game parameters (published in the game config and proof bundles).
    type Params: Clone + Debug + PartialEq + Serialize + DeserializeOwned + Send + Sync + 'static;
    /// What one valid entry plays (the memo's payload).
    type Entry: Payload + Serialize + DeserializeOwned + Send + Sync + 'static;
    /// The game's outcome of a settled round.
    type Outcome: Clone + Debug + PartialEq + Serialize + DeserializeOwned + Send + Sync + 'static;

    /// Classifies every note received in `round`'s block range (in chain
    /// order). Must not look inside sealed receivers.
    fn classify(
        &self,
        p: &Self::Params,
        round: &RoundInfo,
        notes: &[ReceivedNote],
    ) -> Classified<Self::Entry>;

    /// Settles a classified round. `draw` is the round's verified beacon
    /// randomness, `None` exactly when the round is voided.
    fn settle(
        &self,
        p: &Self::Params,
        classified: &Classified<Self::Entry>,
        carry_in: u64,
        draw: Option<&Randomness>,
    ) -> Result<Settlement<Self::Outcome>, RulesError>;

    /// The game's public view of a round so far (served as-is).
    fn board(&self, p: &Self::Params, classified: &Classified<Self::Entry>) -> serde_json::Value;

    /// Checks the game's parameters against the chain's (called at startup).
    fn validate(&self, _chain: &ChainParams, _p: &Self::Params) -> Result<(), String> {
        Ok(())
    }
}

fn add(a: u64, b: u64) -> Result<u64, SettlementError> {
    a.checked_add(b).ok_or(SettlementError::Overflow)
}

/// Checks `settlement` against the round it settles; see the module docs.
pub fn check_settlement<E, O>(
    classified: &Classified<E>,
    settlement: &Settlement<O>,
) -> Result<(), SettlementError> {
    if settlement.round != classified.round || settlement.voided != classified.voided {
        return Err(SettlementError::WrongRound {
            expected: classified.round,
            expected_voided: classified.voided,
            got: settlement.round,
            got_voided: settlement.voided,
        });
    }
    let received = classified.received().ok_or(SettlementError::Overflow)?;
    if settlement.received != received {
        return Err(SettlementError::Received {
            stated: settlement.received,
            actual: received,
        });
    }

    // Sealed receivers of this round's entries: (sealed, aad, may win).
    let mut entries: Vec<(&SealedReceiver, &[u8], bool, bool)> = classified
        .valid
        .iter()
        .map(|v| (&v.sealed, v.aad.as_slice(), !classified.voided, false))
        .chain(
            classified
                .refunds
                .iter()
                .map(|r| (&r.sealed, r.aad.as_slice(), false, false)),
        )
        .collect();
    let mut paid = 0u64;
    let mut output_fees = 0u64;
    for (index, payout) in settlement.payouts.iter().enumerate() {
        if payout.net == 0 || payout.fee.checked_add(payout.net) != Some(payout.gross) {
            return Err(SettlementError::PayoutArithmetic {
                index,
                gross: payout.gross,
                fee: payout.fee,
                net: payout.net,
            });
        }
        let matches = |(s, a, _, _): &(&SealedReceiver, &[u8], bool, bool)| {
            *s == &payout.sealed && *a == payout.aad.as_slice()
        };
        let Some(position) = entries.iter().position(|e| matches(e) && !e.3) else {
            return Err(if entries.iter().any(matches) {
                SettlementError::EntryPaidTwice { index }
            } else {
                SettlementError::UnknownReceiver { index }
            });
        };
        let entry = &mut entries[position];
        if matches!(payout.kind, PayoutKind::Win) && !entry.2 {
            return Err(SettlementError::WinFromRefund { index });
        }
        entry.3 = true;
        paid = add(paid, payout.net)?;
        output_fees = add(output_fees, payout.fee)?;
    }
    if paid != settlement.paid {
        return Err(SettlementError::Paid {
            stated: settlement.paid,
            actual: paid,
        });
    }
    if settlement.fees < output_fees {
        return Err(SettlementError::Fees {
            fees: settlement.fees,
            output_fees,
        });
    }

    let spendable: Vec<InputRef> = classified
        .valid
        .iter()
        .map(|v| InputRef::from(&v.note))
        .chain(classified.refunds.iter().map(|r| InputRef::from(&r.note)))
        .collect();
    for input in &settlement.payout_inputs {
        if !spendable.contains(input) {
            return Err(SettlementError::UnknownInput {
                txid: input.txid.clone(),
                output_index: input.output_index,
            });
        }
    }
    // `InputRef` orders by (height, txid, output_index) first.
    if settlement.payout_inputs.windows(2).any(|w| w[0] >= w[1]) {
        return Err(SettlementError::InputOrder);
    }

    let inflow = add(received, settlement.carry_in)?;
    let outflow = add(add(settlement.paid, settlement.fees)?, settlement.carry_out)?;
    if inflow != outflow {
        return Err(SettlementError::Conservation {
            received,
            carry_in: settlement.carry_in,
            paid: settlement.paid,
            fees: settlement.fees,
            carry_out: settlement.carry_out,
        });
    }
    Ok(())
}

/// Settles with the game's rules and enforces [`check_settlement`].
pub fn settle_checked<R: Rules>(
    rules: &R,
    p: &R::Params,
    classified: &Classified<R::Entry>,
    carry_in: u64,
    draw: Option<&Randomness>,
) -> Result<Settlement<R::Outcome>, SettlementError> {
    if classified.voided != draw.is_none() {
        return Err(SettlementError::DrawMismatch);
    }
    let settlement = rules.settle(p, classified, carry_in, draw)?;
    check_settlement(classified, &settlement)?;
    Ok(settlement)
}

/// ZIP 317 grace actions: a transaction always pays for at least this many.
pub const ZIP317_GRACE_ACTIONS: u64 = 2;

/// The ZIP 317 fee of an Orchard-shaped transaction with `spends` spends and
/// `outputs` outputs: `marginal_fee · max(grace, spends, outputs)`.
pub fn zip317_fee(chain: &ChainParams, spends: u64, outputs: u64) -> Option<u64> {
    let actions = ZIP317_GRACE_ACTIONS.max(spends).max(outputs);
    chain.marginal_fee_zat.checked_mul(actions)
}

/// Whether `collected` fees cover the ZIP 317 fee of a payout transaction
/// spending `spends` notes and creating `payouts` payout outputs plus one
/// change output.
pub fn fees_cover(chain: &ChainParams, collected: u64, spends: u64, payouts: u64) -> Option<bool> {
    Some(zip317_fee(chain, spends, payouts.checked_add(1)?)? <= collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::round_window;
    use crate::entry::generate_sealing_keypair;
    use crate::entry::memo::tests::{receiver, Digit, FORMAT};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn note(height: u32, memo: Vec<u8>) -> ReceivedNote {
        ReceivedNote {
            txid: "ab".repeat(32),
            output_index: 0,
            height,
            value: 1,
            memo,
        }
    }

    #[test]
    fn screen_applies_voided_window_and_round_in_order() {
        let chain = ChainParams::testnet(1_000);
        let mut rng = StdRng::seed_from_u64(1);
        let (_, pk) = generate_sealing_keypair(&mut rng);
        let window = round_window(&chain, 2).expect("window");
        let memo = |round| {
            EntryMemo::seal(
                &FORMAT,
                round,
                Digit(1),
                &receiver(1),
                &pk,
                &mut StdRng::seed_from_u64(2),
            )
            .expect("seal")
            .encode(&FORMAT)
            .to_vec()
        };
        let info = |voided| RoundInfo {
            round: 2,
            window,
            voided,
        };
        let s =
            |voided, height, m: Vec<u8>| screen::<Digit>(&FORMAT, &info(voided), &note(height, m));
        assert!(matches!(
            s(false, window.start, memo(2)),
            Screened::Candidate(_)
        ));
        assert!(matches!(
            s(true, window.start, memo(2)),
            Screened::Refund(_, RefundReason::RoundVoided)
        ));
        assert!(matches!(
            s(false, window.end, memo(2)),
            Screened::Refund(_, RefundReason::OutOfWindow)
        ));
        assert!(matches!(
            s(false, window.start, memo(3)),
            Screened::Refund(_, RefundReason::WrongRound { memo_round: 3 })
        ));
        assert!(matches!(
            s(false, window.start, vec![0xF6]),
            Screened::Undecodable(_)
        ));
    }

    #[test]
    fn zip317_fee_and_coverage() {
        let chain = ChainParams::testnet(1);
        assert_eq!(zip317_fee(&chain, 0, 1), Some(10_000), "grace actions");
        assert_eq!(zip317_fee(&chain, 5, 3), Some(25_000));
        // One input and one payout plus change: 2 actions, 10,000 collected.
        assert_eq!(fees_cover(&chain, 10_000, 1, 1), Some(true));
        assert_eq!(fees_cover(&chain, 9_999, 1, 1), Some(false));
        assert_eq!(zip317_fee(&chain, u64::MAX, 1), None);
    }
}
