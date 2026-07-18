//! Position evaluation for Reversi.
//!
//! A handcrafted [`Heuristic`] implements `game-core`'s `Evaluator` trait today;
//! an ML evaluator can slot in later behind the same trait. The search that
//! consumes evaluators lives in `game-core` (see its `search` function).

mod heuristic;
pub mod matchup;

pub use heuristic::Heuristic;
