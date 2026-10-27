//! `zecraft-core`: the pure, deterministic half of the Zecraft kit for
//! provably fair private games on shielded Zcash.
//!
//! Players enter a round by sending shielded ZEC to a game's vault with a
//! memo naming the round and a game-specific payload. When the round's
//! window closes, a drand beacon published after the close decides it, and
//! a game's rules turn the round's entries into payouts.
//!
//! Everything here is **pure and deterministic**: no I/O, no clocks, no
//! networking, no randomness except what the caller passes in. The same code
//! runs in the round engine, in each keyholder node and in a browser verifier,
//! so it also compiles to `wasm32-unknown-unknown`. All money is `u64`
//! zatoshis under checked arithmetic. Nothing in this crate names a game:
//! magic bytes, text prefixes, HPKE contexts and draw domains are always the
//! caller's.
//!
//! * [`params`]: [`ChainParams`](params::ChainParams), the network, round
//!   clock, fees and beacon every game shares.
//! * [`draw`]: rounds, drand beacons, the timing rule and unbiased draws.
//! * [`entry`]: the entry memo, the sealed payout receiver and ZIP 321.
//! * [`rules`]: the [`Rules`](rules::Rules) trait a game implements, the
//!   generic [`Settlement`](rules::Settlement) and its enforced invariants.

#![forbid(unsafe_code)]

pub mod draw;
pub mod entry;
pub mod params;
pub mod rules;

/// Zatoshis per ZEC.
pub const COIN: u64 = 100_000_000;
