//! Human-vs-AI game flow, built on the pure `game-core` rules and `eval` search.
//!
//! Keeps no windowing or rendering state — just the board and the logic that
//! drives a turn, so it stays easy to reason about.

use eval::Heuristic;
use game_core::{search, Board, Player, Square};

/// AI search depth. Depth 6 is effectively instant on this hardware (~0.2s worst
/// case, per the Stage-3 benchmark) and far stronger than a beginner.
pub const AI_DEPTH: u32 = 6;

/// The human plays Black (which moves first); the AI plays White.
pub const HUMAN: Player = Player::Black;

/// The current game plus the AI's evaluator.
pub struct Game {
    pub board: Board,
    evaluator: Heuristic,
}

impl Game {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            evaluator: Heuristic::new(),
        }
    }

    /// True when it is the human's turn to click a move.
    pub fn awaiting_human(&self) -> bool {
        !self.board.is_terminal() && self.board.to_move() == HUMAN
    }

    /// Attempt the human's move at `sq`. Returns `true` if it was legal and
    /// applied — in which case the AI has already made its reply — and `false`
    /// for an illegal click (ignored) or when it isn't the human's turn.
    pub fn play_human(&mut self, sq: Square) -> bool {
        if !self.awaiting_human() {
            return false;
        }
        match self.board.apply(sq) {
            Some(next) => {
                self.board = next;
                self.run_until_human();
                true
            }
            None => false,
        }
    }

    /// Let the AI move (and auto-pass either side when it has no move) until it
    /// is the human's turn again or the game is over.
    fn run_until_human(&mut self) {
        loop {
            if self.board.is_terminal() {
                break;
            }
            if self.board.to_move() == HUMAN {
                if self.board.legal_moves().is_empty() {
                    // The human has no move: pass automatically and let the AI
                    // continue.
                    self.board = self.board.pass();
                    continue;
                }
                break;
            }
            // AI's turn: search, or pass if it has no move.
            match search(&self.board, AI_DEPTH, &self.evaluator).best_move {
                Some(mv) => {
                    self.board = self.board.apply(mv).expect("engine move is legal");
                }
                None => self.board = self.board.pass(),
            }
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}
