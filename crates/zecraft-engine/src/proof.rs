//! Proof bundles. The format and `recompute` live in [`zecraft_core::proof`]
//! so that independent verifiers need nothing but `zecraft-core`; they are
//! re-exported here for the engine's own users.

pub use zecraft_core::proof::*;
