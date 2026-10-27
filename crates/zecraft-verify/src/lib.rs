//! Independent verification of Zecraft rounds and win proofs, for any game
//! behind the [`Rules`](zecraft_core::rules::Rules) trait.
//!
//! The library does no I/O: callers fetch chain data from wherever they
//! trust (the `zecraft-verify` CLI talks gRPC to any lightwalletd; the browser
//! verifier fetches through a relay and cross-checks block hashes with a
//! second source) and hand it in. Nothing here needs anything from the
//! operator except the round's proof bundle and the game's public
//! configuration (parameters, vault address, UIVK).
//!
//! * [`checks`]: the five round checks (beacon, entries, timing, settlement,
//!   payout), each with a plain-language verdict and technical detail.
//! * [`win`]: proofs that a payout was received at an address the prover
//!   controls ("prove-a-win").

#![forbid(unsafe_code)]

pub mod chain;
pub mod checks;
#[cfg(feature = "cli")]
pub mod cli;
pub mod report;
pub mod win;

pub use report::{CheckId, CheckResult, CheckStatus, RoundReport};
