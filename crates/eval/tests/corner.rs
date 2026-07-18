//! Depth-1 search with the handcrafted evaluator takes an available corner.

use eval::Heuristic;
use game_core::{search, Board, Player, Square};

#[test]
fn depth_one_takes_an_available_corner() {
    let sq = |r, c| Square::new(r, c).expect("in range");

    // Black to move with exactly two legal moves:
    //   - a1 = (0, 0): a corner, capturing the white disc at (0, 1)
    //   - h6 = (5, 5): a non-corner, capturing the white disc at (5, 4)
    // The corner is strictly better under the heuristic, so it must be chosen.
    let board = Board::from_discs(
        &[sq(0, 2), sq(5, 3)], // black discs
        &[sq(0, 1), sq(5, 4)], // white discs
        Player::Black,
    );

    let moves = board.legal_moves();
    assert_eq!(moves.len(), 2, "expected exactly the two crafted moves");
    assert!(moves.contains(&sq(0, 0)));
    assert!(moves.contains(&sq(5, 5)));

    let result = search(&board, 1, &Heuristic::new());
    assert_eq!(result.best_move, Some(sq(0, 0)), "should grab the corner");
}
