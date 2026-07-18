//! Play search engines of different depths against each other.
//!
//! Shared by the strength test and the `matchup` example so both use one game
//! loop. Games are seeded and deterministic given their inputs.

use game_core::rng::SmallRng;
use game_core::{search, Board, Outcome, Player};

use crate::Heuristic;

/// Random plies played from the opening before the engines take over, so a set
/// of games with different seeds explores different lines instead of repeating
/// one deterministic game.
const OPENING_PLIES: usize = 4;

/// Aggregate result of a match, from the deeper engine's point of view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchResult {
    pub deep_wins: u32,
    pub shallow_wins: u32,
    pub draws: u32,
}

/// Play one game to completion. `deep_side` searches to `deep` plies, the other
/// side to `shallow`. Returns the winner, or `None` for a draw.
pub fn play_game(deep: u32, shallow: u32, deep_side: Player, seed: u64) -> Option<Player> {
    let heuristic = Heuristic::new();
    let mut board = Board::new();
    let mut rng = SmallRng::new(seed);

    // Diversify the opening with a few random legal moves.
    for _ in 0..OPENING_PLIES {
        if board.is_terminal() {
            break;
        }
        let moves = board.legal_moves();
        if moves.is_empty() {
            board = board.pass();
            continue;
        }
        board = board
            .apply(moves[rng.below(moves.len())])
            .expect("legal move");
    }

    // Hand over to the engines.
    while !board.is_terminal() {
        let moves = board.legal_moves();
        if moves.is_empty() {
            board = board.pass();
            continue;
        }
        let depth = if board.to_move() == deep_side {
            deep
        } else {
            shallow
        };
        let mv = search(&board, depth, &heuristic)
            .best_move
            .expect("non-terminal side to move has a move");
        board = board.apply(mv).expect("engine move is legal");
    }

    match board.outcome().expect("terminal board has an outcome") {
        Outcome::Win(player) => Some(player),
        Outcome::Draw => None,
    }
}

/// Play `games` games, alternating which colour the deeper engine plays so that
/// first-move advantage cancels out. `seed` makes the whole match reproducible.
pub fn play_match(deep: u32, shallow: u32, games: usize, seed: u64) -> MatchResult {
    let mut result = MatchResult::default();
    for i in 0..games {
        let deep_side = if i % 2 == 0 {
            Player::Black
        } else {
            Player::White
        };
        let game_seed = seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        match play_game(deep, shallow, deep_side, game_seed) {
            Some(winner) if winner == deep_side => result.deep_wins += 1,
            Some(_) => result.shallow_wins += 1,
            None => result.draws += 1,
        }
    }
    result
}
