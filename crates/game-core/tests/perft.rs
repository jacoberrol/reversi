//! Property/perft-style test: play many random games to completion and assert
//! the invariants that must hold at every step of every game.
//!
//! This exercises the public API from outside the crate the way a real driver
//! (self-play, search) would.

use game_core::rng::SmallRng;
use game_core::{Board, Player, NUM_SQUARES};

#[test]
fn thousand_random_games_stay_consistent() {
    const GAMES: u64 = 1_000;
    // Reversi tops out at 60 placements; passes add a few. This is a generous
    // guard against an accidental infinite loop, not a tight bound.
    const MAX_PLIES: u32 = 200;

    for game in 0..GAMES {
        let seed = 0xD1B5_4A32_D192_ED03 ^ game.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut rng = SmallRng::new(seed);
        let mut board = Board::new();
        let mut plies = 0u32;

        loop {
            // Disc counts must always partition the 64 squares.
            let black = board.count(Player::Black);
            let white = board.count(Player::White);
            let empty = board.empty_count();
            assert_eq!(
                black + white + empty,
                NUM_SQUARES as u32,
                "counts must sum to 64 (game {game}, ply {plies})"
            );

            if board.is_terminal() {
                break;
            }

            let moves = board.legal_moves();
            board = if moves.is_empty() {
                // Not terminal but no moves => a pass is forced.
                assert!(board.must_pass(), "expected a forced pass (game {game})");
                board.pass()
            } else {
                let pick = rng.below(moves.len());
                board
                    .apply(moves[pick])
                    .expect("a move from legal_moves must apply")
            };

            plies += 1;
            assert!(plies < MAX_PLIES, "game {game} ran too long");
        }

        // A completed game always has a definite outcome.
        assert!(
            board.outcome().is_some(),
            "terminal board must have an outcome (game {game})"
        );
    }
}
