//! Unbiased draws from verified beacon randomness.
//!
//! Every draw is `SHA-256(domain || round u32 BE || randomness || counter u32 BE)`
//! read as big-endian `u32` words, accepted only when the word is below the
//! largest multiple of `n` that fits in 32 bits (rejection sampling, so no
//! modulo bias), and extended with a fresh counter when a whole digest is
//! rejected. The domain is the caller's: each game, and each independent
//! draw within a game, uses its own, so draws from the same beacon are
//! independent of each other.

use sha2::{Digest, Sha256};

use crate::draw::beacon::Randomness;

/// Errors from drawing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RandomError {
    #[error("cannot draw from an empty range")]
    EmptyRange,
    #[error("rejection sampling did not terminate")]
    Exhausted,
}

/// Draws a uniformly distributed value in `0..n` for `round` under `domain`.
pub fn uniform_below(
    domain: &[u8],
    round: u32,
    randomness: &Randomness,
    n: u32,
) -> Result<u32, RandomError> {
    if n == 0 {
        return Err(RandomError::EmptyRange);
    }
    // Largest multiple of `n` that is at most 2^32.
    let limit = (1u64 << 32) / u64::from(n) * u64::from(n);
    for counter in 0..u32::MAX {
        let digest = Sha256::new()
            .chain_update(domain)
            .chain_update(round.to_be_bytes())
            .chain_update(randomness.0)
            .chain_update(counter.to_be_bytes())
            .finalize();
        for word in digest.chunks_exact(4) {
            let value = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
            if u64::from(value) < limit {
                return Ok(value % n);
            }
        }
    }
    Err(RandomError::Exhausted)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &[u8] = b"test/a/v1";
    const B: &[u8] = b"test/b/v1";

    fn randomness(seed: u8) -> Randomness {
        Randomness([seed; 32])
    }

    #[test]
    fn draws_are_deterministic_and_in_range() {
        let r = randomness(7);
        let a = uniform_below(A, 3, &r, 25).expect("draw");
        assert_eq!(a, uniform_below(A, 3, &r, 25).expect("draw"));
        assert!(a < 25);
        assert_ne!(
            uniform_below(A, 3, &r, 1_000_000).expect("v"),
            uniform_below(B, 3, &r, 1_000_000).expect("v"),
            "domains must separate the draws"
        );
        assert_ne!(
            uniform_below(A, 3, &r, 1_000_000).expect("v"),
            uniform_below(A, 4, &r, 1_000_000).expect("v"),
            "rounds must separate the draws"
        );
        assert_eq!(uniform_below(A, 0, &r, 1).expect("v"), 0);
        assert_eq!(uniform_below(A, 0, &r, 0), Err(RandomError::EmptyRange));
    }

    #[test]
    fn known_answer_is_stable_under_the_original_zeek_domains() {
        // Zeek's regression pin of 2026-10-13, now with its domains passed in
        // by the caller: engine, keyholders and verifiers can never silently
        // disagree on a draw.
        let r = Randomness(
            hex::decode("7f7f0a0e0e2c1c84a4c9f5fcd5b06b8f45b6b9a3a2d8c0e1f2a3b4c5d6e7f809")
                .expect("hex")
                .try_into()
                .expect("32 bytes"),
        );
        let tile = uniform_below(b"zeek/treasure/v1", 42, &r, 25).expect("tile");
        let cache = uniform_below(b"zeek/cache/v1", 42, &r, 625).expect("cache");
        assert_eq!((tile, cache), (20, 283));
    }
}
