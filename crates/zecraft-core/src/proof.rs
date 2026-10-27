//! Proof bundles: everything an independent verifier needs to recompute a
//! round's settlement with nothing but `zecraft-core` and the game's rules.
//! This module lives in `zecraft-core` so verifiers, including a browser
//! one, can use it without the engine.

use serde::{Deserialize, Serialize};

use crate::draw::{
    self, beacon_round_for, beacon_time, check_window_close, round_window, Beacon, Randomness,
    RoundWindow, TimingVerdict,
};
use crate::params::ChainParams;
use crate::rules::{settle_checked, ReceivedNote, RoundInfo, Rules, Settlement, SettlementError};

/// Bundle format version.
pub const PROOF_VERSION: u32 = 1;

/// A self-contained description of one settled round of a game whose
/// parameters are `P`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProofBundle<P> {
    /// Bundle format version ([`PROOF_VERSION`]).
    pub version: u32,
    /// Chain parameters the round was played under.
    pub chain: ChainParams,
    /// The game's parameters.
    pub game: P,
    pub round: u32,
    pub window: RoundWindow,
    /// Every note received at the vault's external address inside the
    /// window, in chain order, with its full memo.
    pub notes: Vec<ReceivedNote>,
    /// `nTime` of the window's first block (fixes the bound drand round).
    pub start_block_time: u64,
    /// `nTime` of the `mtp_window` blocks after the window, in height order.
    pub post_close_block_times: Vec<u64>,
    /// The bound drand round.
    pub drand_round: u64,
    /// Publication time of that drand round.
    pub beacon_time: u64,
    /// The beacon, when the round was drawn (absent for voided rounds).
    pub beacon: Option<Beacon>,
    /// Verified randomness of the beacon.
    pub randomness: Option<Randomness>,
    pub timing: TimingVerdict,
    /// Value carried into this round.
    pub carry_in: u64,
    /// The settlement as the engine computed it.
    pub settlement: Settlement,
    /// Payout transaction record, once the keyholders signed and it was broadcast.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout: Option<PayoutRecord>,
    /// Openings of the payout transaction's payout outputs, once it is mined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout_openings: Option<PayoutOpenings>,
    /// Settled payouts that could not be paid because their sealed receiver
    /// does not open to a valid address (a malformed memo). Their value
    /// stayed in the vault. Only the sealing key's holders (the keyholders,
    /// each of whom checks it before signing) can confirm this.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unclaimable_payouts: Vec<UnclaimablePayout>,
}

/// A settled payout with no payable receiver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnclaimablePayout {
    /// Index of the payout in `settlement.payouts`.
    pub payout_index: u32,
    /// Its net amount, which stayed in the vault.
    pub net: u64,
    /// Why it cannot be paid.
    pub reason: String,
}

/// The minimal note openings that let anyone recompute the note commitment
/// (`cmx`) of each payout output in the payout transaction, and so check its
/// amount (Zcash protocol spec § 5.4.8.4, Sinsemilla commitments; § 4.16,
/// note commitment extraction).
///
/// An Orchard-protocol note is `(d, pk_d, v, rho, rseed)`. `rho` of an
/// output note is the nullifier of the same action's spent note (§ 4.7.3),
/// which is public in the transaction, so it is not repeated here.
/// Disclosed per output: the recipient (a one-time diversified address of
/// the winner's wallet), the value (already public in the settlement) and
/// `rseed`. Nothing here lets anyone derive the note's nullifier, which
/// needs the recipient's nullifier key, so a later spend of the note stays
/// unlinkable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutOpenings {
    /// The payout transaction (display hex).
    pub txid: String,
    /// Height it was mined at.
    pub mined_height: u32,
    /// One opening per paid settlement payout, in settlement order.
    pub outputs: Vec<PayoutOpening>,
}

/// The opening of one payout output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutOpening {
    /// Index of the payout in `settlement.payouts`.
    pub payout_index: u32,
    /// Index of the action in the transaction's Ironwood bundle.
    pub action_index: u32,
    /// Raw 43-byte Orchard-protocol address, hex.
    pub recipient: String,
    /// Note value (the payout's net amount).
    pub value: u64,
    /// The note's `rseed`, hex.
    pub rseed: String,
    /// Note plaintext version (2 Orchard, 3 Ironwood).
    pub note_version: u8,
}

/// State of a payout transaction for one round.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayoutState {
    /// A signing session was opened; the transaction may or may not have
    /// been broadcast (inspect before retrying).
    Signing,
    /// Broadcast and recorded in the vault wallet.
    Broadcast,
}

/// The engine's record of a round's payout transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutRecord {
    pub round: u32,
    pub state: PayoutState,
    /// Signing session id, hex.
    pub session: String,
    /// Transaction id once broadcast (display hex).
    pub txid: Option<String>,
    /// Chain tip when the record was last updated.
    pub height: u32,
    /// Fee of the payout transaction.
    pub fee: u64,
    /// Keyholders that signed.
    pub signers: Vec<u16>,
}

/// Errors from recomputing a bundle.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProofError {
    #[error("unsupported proof bundle version {0}")]
    Version(u32),
    #[error("window does not match the parameters: expected {expected:?}, got {got:?}")]
    Window {
        expected: RoundWindow,
        got: RoundWindow,
    },
    #[error("a note lies outside the window: height {0}")]
    NoteOutsideWindow(u32),
    #[error("bound drand round mismatch: expected {expected}, bundle says {got}")]
    DrandRound { expected: u64, got: u64 },
    #[error("beacon time mismatch: expected {expected}, bundle says {got}")]
    BeaconTime { expected: u64, got: u64 },
    #[error("timing verdict mismatch: recomputed {recomputed:?}, bundle says {got:?}")]
    Timing {
        recomputed: TimingVerdict,
        got: TimingVerdict,
    },
    #[error("beacon is for drand round {0}, not the bound round")]
    BeaconRound(u64),
    #[error("a drawn round needs a beacon")]
    MissingBeacon,
    #[error("randomness mismatch")]
    Randomness,
    #[error("carry_in mismatch: bundle {bundle}, settlement {settlement}")]
    CarryIn { bundle: u64, settlement: u64 },
    #[error(transparent)]
    Rounds(#[from] draw::RoundError),
    #[error(transparent)]
    Beacon(#[from] draw::BeaconError),
    #[error(transparent)]
    TimingRule(#[from] draw::TimingError),
    #[error(transparent)]
    Settlement(#[from] SettlementError),
}

/// Whether `timing` voids the round.
pub fn is_voided(timing: &TimingVerdict) -> bool {
    matches!(timing, TimingVerdict::Unsafe { .. })
}

/// Recomputes the settlement of `bundle` from its inputs with `rules`,
/// checking every intermediate claim the bundle makes (window, beacon
/// binding, timing verdict, beacon signature and randomness). Returns the
/// recomputed (erased) settlement; the caller compares it with
/// `bundle.settlement`.
pub fn recompute<R: Rules>(
    rules: &R,
    bundle: &ProofBundle<R::Params>,
) -> Result<Settlement, ProofError> {
    if bundle.version != PROOF_VERSION {
        return Err(ProofError::Version(bundle.version));
    }
    let chain = &bundle.chain;
    let window = round_window(chain, bundle.round)?;
    if window != bundle.window {
        return Err(ProofError::Window {
            expected: window,
            got: bundle.window,
        });
    }
    if let Some(note) = bundle.notes.iter().find(|n| !window.contains(n.height)) {
        return Err(ProofError::NoteOutsideWindow(note.height));
    }
    let drand_round = beacon_round_for(chain, bundle.start_block_time)?;
    if drand_round != bundle.drand_round {
        return Err(ProofError::DrandRound {
            expected: drand_round,
            got: bundle.drand_round,
        });
    }
    let t = beacon_time(chain, drand_round)?;
    if t != bundle.beacon_time {
        return Err(ProofError::BeaconTime {
            expected: t,
            got: bundle.beacon_time,
        });
    }
    let verdict = check_window_close(chain, t, &bundle.post_close_block_times)?;
    if verdict != bundle.timing {
        return Err(ProofError::Timing {
            recomputed: verdict,
            got: bundle.timing,
        });
    }
    let voided = is_voided(&verdict);
    let randomness = if voided {
        None
    } else {
        let beacon = bundle.beacon.as_ref().ok_or(ProofError::MissingBeacon)?;
        if beacon.round != drand_round {
            return Err(ProofError::BeaconRound(beacon.round));
        }
        let randomness = draw::verify_beacon(&chain.drand, beacon)?;
        if bundle.randomness != Some(randomness) {
            return Err(ProofError::Randomness);
        }
        Some(randomness)
    };
    if bundle.settlement.carry_in != bundle.carry_in {
        return Err(ProofError::CarryIn {
            bundle: bundle.carry_in,
            settlement: bundle.settlement.carry_in,
        });
    }
    let info = RoundInfo {
        round: bundle.round,
        window,
        voided,
    };
    let classified = rules.classify(&bundle.game, &info, &bundle.notes);
    let settlement = settle_checked(
        rules,
        &bundle.game,
        &classified,
        bundle.carry_in,
        randomness.as_ref(),
    )?;
    Ok(settlement.erase()?)
}
