//! Human-vs-AI game flow, built on the pure `game-core` rules and `eval` search.
//!
//! Keeps no windowing or rendering state — just the board, the difficulty, and
//! the logic that drives a turn.

use eval::Heuristic;
use game_core::{search, Board, Outcome, Player, Square};

/// The human plays Black (which moves first); the AI plays White.
pub const HUMAN: Player = Player::Black;

/// AI difficulty, mapped to a search depth (how many plies it looks ahead).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Expert,
}

impl Difficulty {
    /// In selector order (also the order of the on-screen buttons).
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ];

    /// Search depth for this difficulty. Depths verified interactive on this
    /// hardware in the Stage-3 benchmark (Expert is the slowest, ~a few seconds
    /// worst case).
    pub fn depth(self) -> u32 {
        match self {
            Difficulty::Easy => 2,
            Difficulty::Medium => 4,
            Difficulty::Hard => 6,
            Difficulty::Expert => 8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
            Difficulty::Expert => "Expert",
        }
    }

    /// Index into [`Difficulty::ALL`] (matches the button order).
    pub fn index(self) -> usize {
        Difficulty::ALL.iter().position(|&d| d == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Option<Difficulty> {
        Difficulty::ALL.get(index).copied()
    }
}

/// The current game, its difficulty, and the AI's evaluator.
pub struct Game {
    board: Board,
    difficulty: Difficulty,
    evaluator: Heuristic,
}

impl Game {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            difficulty: Difficulty::Hard,
            evaluator: Heuristic::new(),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    /// Change the difficulty. Applies to the AI's next move; no restart needed.
    pub fn set_difficulty(&mut self, difficulty: Difficulty) {
        self.difficulty = difficulty;
    }

    /// Start a new game, keeping the current difficulty.
    pub fn restart(&mut self) {
        self.board = Board::new();
    }

    pub fn is_over(&self) -> bool {
        self.board.is_terminal()
    }

    /// The result once the game is over.
    pub fn outcome(&self) -> Option<Outcome> {
        self.board.outcome()
    }

    /// Disc counts as `(human, ai)` = `(black, white)`.
    pub fn score(&self) -> (u32, u32) {
        (self.board.count(HUMAN), self.board.count(HUMAN.opponent()))
    }

    /// True when it is the human's turn to click a move.
    pub fn awaiting_human(&self) -> bool {
        !self.board.is_terminal() && self.board.to_move() == HUMAN
    }

    /// Attempt the human's move at `sq`. Returns `true` if it was legal and
    /// applied — in which case the AI has already replied — and `false` for an
    /// illegal click (ignored) or when it isn't the human's turn.
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
                    self.board = self.board.pass();
                    continue;
                }
                break;
            }
            match search(&self.board, self.difficulty.depth(), &self.evaluator).best_move {
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
