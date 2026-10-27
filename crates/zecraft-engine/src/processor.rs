//! Round processing: status, timing rule, beacon, classification and
//! settlement. The chain side is the kit's (`zecraft-core::draw`); the game
//! side is the game's [`Rules`], always through
//! [`settle_checked`](zecraft_core::rules::settle_checked), so every
//! settlement the engine stores passed the kit's invariants.
//!
//! Rounds settle strictly in order so that the carry passes from each
//! settlement to the next. A round is settled only once the `mtp_window`
//! blocks after its window are stored and the last of them has
//! [`SETTLE_CONFIRMATIONS`] confirmations, so a reorg can no longer change
//! the inputs the settlement was computed from.

use serde::{Deserialize, Serialize};
use zecraft_core::draw::{self, RoundContext, RoundStatus, RoundWindow, TimingVerdict};
use zecraft_core::entry::Payload;
use zecraft_core::rules::{settle_checked, Classified, InputRef, RoundInfo, Rules};

use crate::beacon::{Backoff, BeaconFetcher};
use crate::config::{GameConfig, SETTLE_CONFIRMATIONS};
use crate::db::{BeaconRow, Db, EntryKind, EntryRow, PlanStatus, SettlementRow};
use crate::error::EngineError;
use crate::proof::{ProofBundle, PROOF_VERSION};
use crate::store::Store;

/// Something that happened, pushed to WebSocket clients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The board changed (new block or new entry).
    Board(BoardState),
    /// A round's window closed.
    RoundClosed {
        round: u32,
        window: RoundWindow,
        drand_round: u64,
        beacon_time: u64,
    },
    /// A beacon was verified for a round.
    Reveal {
        round: u32,
        drand_round: u64,
        /// The beacon's randomness, hex.
        randomness: String,
    },
    /// A settlement plan was stored.
    Settled {
        round: u32,
        voided: bool,
        /// The game's outcome.
        outcome: serde_json::Value,
        carry_out: u64,
    },
    /// A payout transaction was broadcast.
    Paid {
        round: u32,
        txid: String,
        signers: Vec<u16>,
    },
}

/// The public board: the round clock (the kit's) and the game's own view of
/// the round, served as the game returns it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardState {
    /// Chain tip the engine has scanned (0 before the game's genesis block).
    pub tip_height: u32,
    /// `nTime` of that tip (0 before genesis).
    pub tip_time: u64,
    /// Chain tip reported by lightwalletd, which may be below genesis.
    pub chain_tip: u32,
    /// Round the tip belongs to, or the first round if the game has not
    /// started.
    pub round: u32,
    pub status: RoundStatus,
    pub window: RoundWindow,
    /// Blocks until the window closes (counting from the tip).
    pub blocks_remaining: u32,
    /// `tip_time + blocks_remaining · target_spacing` (0 before genesis).
    pub estimated_close_time: u64,
    /// Before genesis: `chain_tip_time + (genesis − chain_tip) · target_spacing`;
    /// `None` once the game has started.
    pub estimated_start_time: Option<u64>,
    /// Value carried into this round.
    pub carry: u64,
    /// Valid entries so far.
    pub entries: usize,
    /// Mean spacing of the last [`SPACING_SAMPLE_BLOCKS`] stored blocks, in
    /// seconds (how fast the chain runs now; wallets turn confirmations
    /// into minutes with it). None until enough blocks are stored.
    pub recent_spacing_secs: Option<u64>,
    /// The game's board (`Rules::board`), as-is.
    pub game: serde_json::Value,
}

/// Blocks the recent spacing is measured over.
pub const SPACING_SAMPLE_BLOCKS: u32 = 20;

/// Mean spacing of consecutive block times (None for fewer than two, or
/// times that run backwards overall).
pub fn mean_spacing(times: &[u64]) -> Option<u64> {
    let (first, last) = (times.first()?, times.last()?);
    let gaps = u64::try_from(times.len().checked_sub(1)?).ok()?;
    if gaps == 0 || last <= first {
        return None;
    }
    Some((last - first) / gaps)
}

/// A closed round waiting for its reveal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRound {
    pub round: u32,
    pub window: RoundWindow,
    pub status: RoundStatus,
    pub drand_round: u64,
    /// Expected publication time of the bound beacon.
    pub beacon_time: u64,
}

/// Drives rounds forward for the game `R`.
pub struct Processor<R: Rules> {
    rules: R,
    config: GameConfig<R::Params>,
    backoff: Backoff,
}

impl<R: Rules> Processor<R> {
    /// A processor for `config`.
    pub fn new(rules: R, config: GameConfig<R::Params>) -> Self {
        Processor {
            rules,
            config,
            backoff: Backoff::default(),
        }
    }

    /// The game's rules.
    pub fn rules(&self) -> &R {
        &self.rules
    }

    /// The game config.
    pub fn config(&self) -> &GameConfig<R::Params> {
        &self.config
    }

    /// Carry after the last settled round.
    pub fn carry(&self, db: &Db) -> Result<u64, EngineError> {
        Ok(match db.last_settled_round()? {
            Some(round) => {
                db.settlement(round)?
                    .ok_or_else(|| EngineError::Corrupt(format!("settlement {round} vanished")))?
                    .settlement
                    .carry_out
            }
            None => self.config.initial_carry_zat,
        })
    }

    /// The on-chain context of `round` as currently stored.
    pub fn round_context(
        &self,
        db: &Db,
        round: u32,
    ) -> Result<(RoundWindow, RoundContext), EngineError> {
        let params = &self.config.chain;
        let window = draw::round_window(params, round)?;
        let tip_height = db.tip()?.map(|b| b.height).unwrap_or(0);
        let start_block_time = db.block(window.start)?.map(|b| b.time);
        let post_close_block_times = db.block_times(
            window.end,
            window
                .end
                .checked_add(params.mtp_window)
                .ok_or(EngineError::Overflow("post-close window"))?,
        )?;
        Ok((
            window,
            RoundContext {
                tip_height,
                start_block_time,
                post_close_block_times,
                settled: db.settlement(round)?.is_some(),
            },
        ))
    }

    /// Status of `round`.
    pub fn status(
        &self,
        db: &Db,
        round: u32,
    ) -> Result<(RoundWindow, RoundStatus, RoundContext), EngineError> {
        let (window, ctx) = self.round_context(db, round)?;
        let status = draw::round_status(&self.config.chain, round, &ctx)?;
        Ok((window, status, ctx))
    }

    /// The round the scanned tip belongs to (round 0 before the game starts).
    pub fn current_round(&self, db: &Db) -> Result<u32, EngineError> {
        let tip = db.tip()?.map(|b| b.height).unwrap_or(0);
        Ok(draw::round_for_height(&self.config.chain, tip).unwrap_or(0))
    }

    /// Every note of `round` classified under the game rules.
    pub fn classified(
        &self,
        db: &Db,
        round: u32,
        voided: bool,
    ) -> Result<Classified<R::Entry>, EngineError> {
        let window = draw::round_window(&self.config.chain, round)?;
        let notes = db
            .notes_in_range(window.start, window.end)?
            .iter()
            .map(|n| n.to_received_note())
            .collect::<Vec<_>>();
        Ok(self.rules.classify(
            &self.config.game,
            &RoundInfo {
                round,
                window,
                voided,
            },
            &notes,
        ))
    }

    /// The notes the payout transaction of a settled round must spend, in
    /// canonical order, as its settlement states them.
    pub fn payout_inputs(&self, row: &SettlementRow) -> Vec<InputRef> {
        row.settlement.payout_inputs.clone()
    }

    /// The public board for the current round.
    pub fn board(&self, db: &Db) -> Result<BoardState, EngineError> {
        let params = &self.config.chain;
        let tip = db.tip()?;
        let (tip_height, tip_time) = tip.map(|b| (b.height, b.time)).unwrap_or((0, 0));
        // Before genesis nothing is stored; the chain tip lightwalletd reported
        // still says how far away round 0 is.
        let chain_tip = db.chain_tip()?.unwrap_or(tip_height).max(tip_height);
        let round = self.current_round(db)?;
        let (window, status, _) = self.status(db, round)?;
        let classified = self.classified(db, round, status == RoundStatus::Voided)?;
        let blocks_remaining = window.end.saturating_sub(chain_tip.saturating_add(1));
        let estimated_start_time = match (tip_time, db.chain_tip_time()?) {
            (0, Some(chain_tip_time)) if chain_tip < params.genesis_height => Some(
                chain_tip_time
                    .checked_add(
                        u64::from(params.genesis_height - chain_tip)
                            .saturating_mul(params.target_spacing_secs),
                    )
                    .ok_or(EngineError::Overflow("estimated start time"))?,
            ),
            _ => None,
        };
        // Unknown (0) until the first game block is stored.
        let estimated_close_time = if tip_time == 0 {
            0
        } else {
            tip_time
                .checked_add(u64::from(blocks_remaining).saturating_mul(params.target_spacing_secs))
                .ok_or(EngineError::Overflow("estimated close time"))?
        };
        Ok(BoardState {
            tip_height,
            tip_time,
            chain_tip,
            round,
            status,
            window,
            blocks_remaining,
            estimated_close_time,
            estimated_start_time,
            carry: self.carry(db)?,
            entries: classified.valid.len(),
            recent_spacing_secs: mean_spacing(&db.block_times(
                tip_height.saturating_sub(SPACING_SAMPLE_BLOCKS),
                tip_height.saturating_add(1),
            )?),
            game: self.rules.board(&self.config.game, &classified),
        })
    }

    /// Closed, unsettled rounds in order.
    pub fn pending_rounds(&self, db: &Db) -> Result<Vec<PendingRound>, EngineError> {
        let params = &self.config.chain;
        let tip = db.tip()?.map(|b| b.height).unwrap_or(0);
        let first = db.last_settled_round()?.map(|r| r + 1).unwrap_or(0);
        let current = self.current_round(db)?;
        let mut pending = Vec::new();
        for round in first..=current {
            let (window, status, ctx) = self.status(db, round)?;
            if tip < window.end || status == RoundStatus::Settled {
                continue;
            }
            let Some(start_time) = ctx.start_block_time else {
                continue;
            };
            let drand_round = draw::beacon_round_for(params, start_time)?;
            pending.push(PendingRound {
                round,
                window,
                status,
                drand_round,
                beacon_time: draw::beacon_time(params, drand_round)?,
            });
        }
        Ok(pending)
    }

    /// The proof bundle of a settled round.
    pub fn proof_bundle(
        &self,
        db: &Db,
        row: &SettlementRow,
    ) -> Result<ProofBundle<R::Params>, EngineError> {
        let window = draw::round_window(&self.config.chain, row.round)?;
        let notes = db
            .notes_in_range(window.start, window.end)?
            .iter()
            .map(|n| n.to_received_note())
            .collect();
        let (beacon, randomness) = if row.voided {
            (None, None)
        } else {
            let b = db.beacon(row.drand_round)?.ok_or_else(|| {
                EngineError::Corrupt(format!(
                    "beacon {} missing for drawn round {}",
                    row.drand_round, row.round
                ))
            })?;
            (Some(b.beacon), Some(b.randomness))
        };
        Ok(ProofBundle {
            version: PROOF_VERSION,
            chain: self.config.chain.clone(),
            game: self.config.game.clone(),
            round: row.round,
            window,
            notes,
            start_block_time: row.start_block_time,
            post_close_block_times: row.post_close_block_times.clone(),
            drand_round: row.drand_round,
            beacon_time: row.beacon_time,
            beacon,
            randomness,
            timing: row.timing,
            carry_in: row.settlement.carry_in,
            settlement: row.settlement.clone(),
            payout: db.payout(row.round)?,
            payout_openings: db.payout_openings(row.round)?,
            unclaimable_payouts: db.unclaimable(row.round)?,
        })
    }

    /// Settles every round that is ready, in order. `now` is the current Unix
    /// time (beacons are only fetched once published).
    ///
    /// The beacon is fetched with no database connection held; each round's
    /// beacon and settlement are then written in one short transaction.
    pub async fn process(
        &mut self,
        store: &Store,
        fetcher: &mut dyn BeaconFetcher,
        now: u64,
    ) -> Result<Vec<Event>, EngineError> {
        let params = self.config.chain.clone();
        let mut events = Vec::new();
        let Some(tip) = store.read(|db| db.tip())? else {
            return Ok(events);
        };
        let mut round = store
            .read(|db| db.last_settled_round())?
            .map(|r| r + 1)
            .unwrap_or(0);
        loop {
            let (window, status, ctx) = store.read(|db| self.status(db, round))?;
            let required_tip = window
                .end
                .checked_add(params.mtp_window - 1)
                .and_then(|h| h.checked_add(SETTLE_CONFIRMATIONS))
                .ok_or(EngineError::Overflow("settlement height"))?;
            if tip.height < required_tip
                || ctx.post_close_block_times.len() < params.mtp_window as usize
            {
                break;
            }
            let start_time = ctx.start_block_time.ok_or_else(|| {
                EngineError::Corrupt(format!("start block of round {round} missing"))
            })?;
            let drand_round = draw::beacon_round_for(&params, start_time)?;
            let beacon_time = draw::beacon_time(&params, drand_round)?;
            let timing =
                draw::check_window_close(&params, beacon_time, &ctx.post_close_block_times)?;

            let mut new_beacon = None;
            let randomness = match (status, timing) {
                (RoundStatus::Voided, _) | (_, TimingVerdict::Unsafe { .. }) => None,
                (RoundStatus::Revealable, TimingVerdict::Safe { .. }) => {
                    let fetched = match store.read(|db| db.beacon(drand_round))? {
                        Some(row) => row,
                        None => {
                            if now < beacon_time || !self.backoff.ready(drand_round, now) {
                                break;
                            }
                            match fetcher.fetch(&params.drand, drand_round).await {
                                Ok(f) => {
                                    self.backoff.succeeded(drand_round);
                                    let row = BeaconRow {
                                        beacon: f.beacon,
                                        randomness: f.randomness,
                                        relay: f.relay,
                                    };
                                    new_beacon = Some(row.clone());
                                    row
                                }
                                Err(e) => {
                                    let delay = self.backoff.failed(drand_round, now);
                                    tracing::warn!(round, drand_round, retry_in_secs = delay, error = %e, "beacon not available yet");
                                    break;
                                }
                            }
                        }
                    };
                    events.push(Event::Reveal {
                        round,
                        drand_round,
                        randomness: hex::encode(fetched.randomness.0),
                    });
                    Some(fetched.randomness)
                }
                (other, _) => {
                    return Err(EngineError::Corrupt(format!(
                        "round {round} has status {other:?} although its blocks are confirmed"
                    )));
                }
            };

            let voided = randomness.is_none();
            let (classified, carry_in) = store.read(|db| {
                Ok::<_, EngineError>((self.classified(db, round, voided)?, self.carry(db)?))
            })?;
            let settlement = settle_checked(
                &self.rules,
                &self.config.game,
                &classified,
                carry_in,
                randomness.as_ref(),
            )?
            .erase()?;
            let entries = entry_rows(&classified);
            let row = SettlementRow {
                round,
                settled_at_height: tip.height,
                start_block_time: start_time,
                post_close_block_times: ctx.post_close_block_times[..params.mtp_window as usize]
                    .to_vec(),
                drand_round,
                beacon_time,
                timing,
                voided,
                settlement: settlement.clone(),
                status: PlanStatus::AwaitingSignatures,
            };
            store.write(|db| {
                db.transaction(|tx| {
                    if let Some(beacon) = &new_beacon {
                        tx.insert_beacon(beacon)?;
                    }
                    tx.insert_settlement(&row)?;
                    tx.insert_entries(&entries)
                })
            })?;
            tracing::info!(
                round,
                voided,
                outcome = %settlement.outcome,
                received = settlement.received,
                payouts = settlement.payouts.len(),
                carry_out = settlement.carry_out,
                "settlement plan stored (awaiting signatures)"
            );
            events.push(Event::Settled {
                round,
                voided,
                outcome: settlement.outcome.clone(),
                carry_out: settlement.carry_out,
            });
            round += 1;
        }
        Ok(events)
    }
}

/// The entries of a classified round, as the database stores them.
pub fn entry_rows<E: Payload>(classified: &Classified<E>) -> Vec<EntryRow> {
    let row = |note: &zecraft_core::rules::ReceivedNote, payload: Vec<u8>, sealed: &[u8], kind| {
        EntryRow {
            round: classified.round,
            height: note.height,
            txid: note.txid.clone(),
            action_index: note.output_index,
            value: note.value,
            payload,
            sealed_receiver: sealed.to_vec(),
            kind,
        }
    };
    let mut out: Vec<EntryRow> = classified
        .valid
        .iter()
        .map(|v| {
            row(
                &v.note,
                v.entry.to_bytes(),
                v.sealed.as_bytes(),
                EntryKind::Valid,
            )
        })
        .collect();
    // The refund's payload is the memo's, after the header's 4 + 1 + 4 bytes.
    out.extend(classified.refunds.iter().map(|r| {
        row(
            &r.note,
            r.aad.get(9..).unwrap_or_default().to_vec(),
            r.sealed.as_bytes(),
            EntryKind::Refund,
        )
    }));
    out
}

#[cfg(test)]
mod spacing_tests {
    use super::mean_spacing;

    #[test]
    fn mean_spacing_of_block_times() {
        assert_eq!(mean_spacing(&[100, 125, 150, 200]), Some(33));
        assert_eq!(mean_spacing(&[100]), None);
        assert_eq!(mean_spacing(&[]), None);
        assert_eq!(mean_spacing(&[200, 100]), None);
    }
}
