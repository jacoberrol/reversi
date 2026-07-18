//! Minimax search with alpha-beta pruning.
//!
//! Written in the *negamax* form: because Reversi is zero-sum, one player's
//! score is the negation of the other's, so a single routine handles both
//! sides by flipping signs instead of duplicating "maximizing" and
//! "minimizing" code. Alpha-beta pruning skips branches that cannot affect the
//! result — `alpha` is the best score the side to move has already secured,
//! `beta` the best the opponent will allow; once `alpha >= beta` the rest of a
//! node is irrelevant.

use crate::{Board, Evaluator, Square};

/// A bound comfortably outside any real score, used to open the search window.
const INF: i32 = 1_000_000_000;

/// Value of a finished game per net disc. Large enough that any proven
/// win/loss outranks every heuristic leaf score, so search never trades a
/// certain win for a merely good-looking position.
const TERMINAL_UNIT: i32 = 1_000_000;

/// The result of a search: the chosen move (if the side to move has one) and
/// its score from that side's perspective.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SearchResult {
    pub best_move: Option<Square>,
    pub score: i32,
}

/// Search `board` to `depth` plies with `evaluator`, returning the best move
/// for the side to move.
///
/// `depth` is the difficulty knob: how many plies to look ahead. If the side to
/// move has no legal move, `best_move` is `None` and the caller should
/// [`pass`](Board::pass); the reported score still reflects the position.
pub fn search<E: Evaluator>(board: &Board, depth: u32, evaluator: &E) -> SearchResult {
    let moves = board.legal_moves();
    if moves.is_empty() {
        let score = if board.is_terminal() {
            terminal_score(board)
        } else {
            // No move here, but the game continues: value equals the negation
            // of the opponent's value after we pass.
            -negamax(&board.pass(), depth, -INF, INF, evaluator)
        };
        return SearchResult {
            best_move: None,
            score,
        };
    }

    let mut best = -INF;
    let mut best_move = None;
    let mut alpha = -INF;
    for mv in moves {
        let child = board.apply(mv).expect("a move from legal_moves applies");
        let score = -negamax(&child, depth.saturating_sub(1), -INF, -alpha, evaluator);
        if score > best {
            best = score;
            best_move = Some(mv);
        }
        if best > alpha {
            alpha = best;
        }
    }
    SearchResult {
        best_move,
        score: best,
    }
}

/// Negamax value of `board` for the side to move, within the open interval
/// `(alpha, beta)`.
fn negamax<E: Evaluator>(
    board: &Board,
    depth: u32,
    mut alpha: i32,
    beta: i32,
    evaluator: &E,
) -> i32 {
    if board.is_terminal() {
        return terminal_score(board);
    }
    if depth == 0 {
        return evaluator.evaluate(board, board.to_move());
    }

    let moves = board.legal_moves();
    if moves.is_empty() {
        // Forced pass. We don't spend a depth level on it: a pass is always
        // followed by a real move (the position isn't terminal, so the
        // opponent can move), which keeps the recursion finite.
        return -negamax(&board.pass(), depth, -beta, -alpha, evaluator);
    }

    let mut best = -INF;
    for mv in moves {
        let child = board.apply(mv).expect("a move from legal_moves applies");
        let score = -negamax(&child, depth - 1, -beta, -alpha, evaluator);
        if score > best {
            best = score;
        }
        if best > alpha {
            alpha = best;
        }
        if alpha >= beta {
            // The opponent already has a better alternative elsewhere; this
            // node can't improve their choice, so stop (beta cutoff).
            break;
        }
    }
    best
}

/// Exact score of a finished game from the side-to-move's perspective.
fn terminal_score(board: &Board) -> i32 {
    let me = board.count(board.to_move()) as i32;
    let opp = board.count(board.to_move().opponent()) as i32;
    (me - opp) * TERMINAL_UNIT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Board, Player};

    /// A trivial evaluator: just the disc-count difference. Enough to test the
    /// search machinery without pulling in the `eval` crate.
    struct DiscCount;
    impl Evaluator for DiscCount {
        fn evaluate(&self, board: &Board, perspective: Player) -> i32 {
            board.count(perspective) as i32 - board.count(perspective.opponent()) as i32
        }
    }

    #[test]
    fn depth_one_maximizes_immediate_gain() {
        // At depth 1 with a disc-count evaluator, the chosen move should be the
        // one that leaves the mover with the most discs after flipping.
        let board = Board::new();
        let result = search(&board, 1, &DiscCount);
        let mv = result.best_move.expect("opening has moves");

        // Whatever it picked must be legal and must be the best by disc count.
        let best_gain = board
            .legal_moves()
            .into_iter()
            .map(|m| {
                let after = board.apply(m).unwrap();
                after.count(Player::Black) as i32 - after.count(Player::White) as i32
            })
            .max()
            .unwrap();
        let chosen_gain = {
            let after = board.apply(mv).unwrap();
            after.count(Player::Black) as i32 - after.count(Player::White) as i32
        };
        assert_eq!(chosen_gain, best_gain);
    }

    #[test]
    fn terminal_position_reports_no_move() {
        // Only Black discs: nobody can move, so there is no best move.
        let sq = |r, c| Square::new(r, c).unwrap();
        let board = Board::from_discs(&[sq(3, 3), sq(4, 4)], &[], Player::Black);
        let result = search(&board, 4, &DiscCount);
        assert_eq!(result.best_move, None);
    }
}
