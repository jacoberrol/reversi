//! Random self-play: drive a game to completion with random legal moves.
//!
//! Used by the `selfplay` example (`just selfplay N`) and by the perft test.
//! Kept in the library so both share one correct game loop.

use crate::rng::SmallRng;
use crate::{Board, Outcome};

/// The result of one played-out game.
pub struct Playout {
    /// Who won, or a draw.
    pub outcome: Outcome,
    /// Number of half-moves taken, counting forced passes.
    pub plies: u32,
}

/// Play one game from the opening, choosing uniformly among legal moves and
/// passing when required, until the position is terminal.
///
/// Deterministic in `seed`: the same seed always produces the same game.
pub fn random_playout(seed: u64) -> Playout {
    let mut board = Board::new();
    let mut rng = SmallRng::new(seed);
    let mut plies = 0;

    loop {
        if board.is_terminal() {
            break;
        }
        let moves = board.legal_moves();
        board = if moves.is_empty() {
            board.pass()
        } else {
            let pick = rng.below(moves.len());
            // A move drawn from `legal_moves` always applies.
            board.apply(moves[pick]).expect("legal move applies")
        };
        plies += 1;
    }

    Playout {
        outcome: board.outcome().expect("a terminal board has an outcome"),
        plies,
    }
}
