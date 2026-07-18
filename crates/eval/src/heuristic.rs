//! A handcrafted Reversi evaluator: corner control, mobility, and disc parity.

use game_core::{Board, Cell, Evaluator, Player, Square};

/// The four corners, which can never be flipped once taken.
const CORNERS: [(u8, u8); 4] = [(0, 0), (0, 7), (7, 0), (7, 7)];

/// A weighted sum of simple positional terms. Weights are hand-tuned, not
/// learned; an ML evaluator will later implement the same [`Evaluator`] trait.
///
/// The relative weights encode Reversi wisdom: corners are worth far more than
/// raw discs (a corner anchors stable discs and can't be retaken), mobility
/// matters (having moves, and denying the opponent theirs), and the disc count
/// itself is only a mild factor until the endgame — maximizing discs early is
/// often a trap.
pub struct Heuristic {
    pub corner: i32,
    pub mobility: i32,
    pub parity: i32,
}

impl Heuristic {
    /// The default weights.
    pub fn new() -> Self {
        Heuristic {
            corner: 25,
            mobility: 5,
            parity: 1,
        }
    }
}

impl Default for Heuristic {
    fn default() -> Self {
        Heuristic::new()
    }
}

impl Evaluator for Heuristic {
    fn evaluate(&self, board: &Board, perspective: Player) -> i32 {
        let me = perspective;
        let opp = perspective.opponent();

        // Each term is "mine minus the opponent's", which makes the whole score
        // zero-sum: evaluate(b, p) == -evaluate(b, p.opponent()), as search
        // requires.
        let corner = self.corner * (corners_held(board, me) - corners_held(board, opp));
        let mobility = self.mobility * (board.mobility(me) as i32 - board.mobility(opp) as i32);
        let parity = self.parity * (board.count(me) as i32 - board.count(opp) as i32);

        corner + mobility + parity
    }
}

/// How many corners `player` currently holds.
fn corners_held(board: &Board, player: Player) -> i32 {
    CORNERS
        .iter()
        .filter(|&&(row, col)| {
            let sq = Square::new(row, col).expect("corner coordinates are in range");
            board.cell(sq) == Cell::Disc(player)
        })
        .count() as i32
}
