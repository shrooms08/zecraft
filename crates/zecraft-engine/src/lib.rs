//! `zecraft-engine`: the round engine.
//!
//! It follows Zcash testnet through `lightwalletd`, detects digs with the
//! vault's **external-scope incoming viewing key** (so the vault's own change
//! is invisible to it and it never holds spend authority), records the block
//! timestamps zecraft-core's timing rule needs, fetches and verifies the drand
//! beacon bound to each round, and settles every round with
//! [`zecraft_core::settle::settle`]. Settlements are persisted as plans awaiting
//! the keyholders' signatures; this engine never sends funds.
//!
//! Every rule comes from `zecraft-core`; this crate only moves data between the
//! chain, SQLite and the HTTP API.
//!
//! Privacy: sealed payout receivers are stored and served as sealed bytes
//! only. Nothing in this crate opens them.

#![forbid(unsafe_code)]

pub mod api;
pub mod beacon;
pub mod chain;
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
