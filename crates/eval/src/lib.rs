//! Position evaluation and search.
//!
//! Handcrafted heuristics (corner control, mobility, disc parity) live behind a
//! trait so an ML evaluator can slot in later without touching callers. Depends
//! only on `game-core`.

// Stage 3 fills this in with the `Evaluator` trait and alpha-beta search.
