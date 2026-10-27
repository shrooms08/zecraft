//! `zecraft-engine`: the round engine.
//!
//! It follows Zcash testnet through `lightwalletd`, detects digs with the
//! vault's **external-scope incoming viewing key** (so the vault's own change
//! is invisible to it and it never holds spend authority), records the block
//! timestamps zecraft-core's timing rule needs, fetches and verifies the drand
//! beacon bound to each round, and settles every round with the game's
//! [`Rules`](zecraft_core::rules::Rules) through
//! [`settle_checked`](zecraft_core::rules::settle_checked). Settlements are
//! persisted as plans awaiting the keyholders' signatures; the engine holds
//! no key share.
//!
//! Every rule comes from `zecraft-core` and the game; this crate only moves
//! data between the chain, SQLite, the keyholders and the HTTP API. A game's
//! engine binary calls [`cli::main`].
//!
//! Privacy: sealed payout receivers are stored and served as sealed bytes
//! only. Nothing in this crate opens them.

#![forbid(unsafe_code)]

pub mod api;
pub mod beacon;
pub mod chain;
pub mod cli;
pub mod config;
pub mod coordinator;
pub mod db;
pub mod decrypt;
pub mod edge;
pub mod error;
pub mod identity;
pub mod ledger;
pub mod logging;
pub mod processor;
pub mod proof;
pub mod protocol;
pub mod relay;
pub mod store;
#[cfg(any(test, feature = "testkit"))]
pub mod synthetic;

pub use error::EngineError;
