//! A minimal deterministic PRNG.
//!
//! `game-core` may not depend on external crates, so we can't pull in `rand`.
//! Self-play and the perft test only need cheap, seedable, reproducible random
//! numbers — an xorshift64\* generator is plenty and fits in a `u64`.

/// A small, fast, seedable pseudo-random generator (xorshift64\*).
///
/// Not cryptographically secure; intended only for reproducible self-play.
pub struct SmallRng(u64);

impl SmallRng {
    /// Seed the generator. A zero seed is nudged to 1, since xorshift is stuck
    /// at zero.
    pub fn new(seed: u64) -> Self {
        SmallRng(if seed == 0 { 1 } else { seed })
    }

    /// Next pseudo-random 64-bit value, advancing the state.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`.
    ///
    /// # Panics
    /// Panics if `bound == 0` (there is no value to return). Callers pass the
    /// length of a non-empty slice, so this is unreachable in practice.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}
