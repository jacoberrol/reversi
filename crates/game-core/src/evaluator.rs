//! The scoring interface that [`search`](crate::search) consumes.

use crate::{Board, Player};

/// Scores a position from `perspective`'s point of view: larger is better for
/// that player. The units are arbitrary — only the ordering of scores matters.
///
/// Implementations live in the `eval` crate (a handcrafted heuristic today, an
/// ML model later). The trait is defined here, next to the search that uses it,
/// so `game-core`'s search stays generic without depending on `eval` — which
/// would invert the `eval -> game-core` dependency direction.
///
/// For search to reason correctly the score should be *zero-sum*: swapping the
/// perspective should negate the score, i.e. `evaluate(b, p) ==
/// -evaluate(b, p.opponent())`. The handcrafted evaluator achieves this by
/// scoring "my terms minus the opponent's".
pub trait Evaluator {
    fn evaluate(&self, board: &Board, perspective: Player) -> i32;
}
