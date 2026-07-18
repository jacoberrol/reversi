//! Game flow, built on the pure `game-core` rules and `eval` search.
//!
//! Works for both modes: single-player applies the local move then the AI's
//! reply; network applies the local move only (the opponent's move arrives over
//! the wire via [`apply_remote_move`](Game::apply_remote_move)). Either way,
//! moves become [`Transition`]s for the animator, and forced passes are resolved
//! locally (never sent) so both networked clients stay in lockstep.

use eval::Heuristic;
use game_core::{search, Board, Outcome, Player, Square};

/// One move's board change: the position before and after. The animator diffs
/// these into per-disc animations.
pub struct Transition {
    pub before: Board,
    pub after: Board,
}

/// AI difficulty, mapped to a search depth (how many plies it looks ahead).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Expert,
}

impl Difficulty {
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ];

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

    pub fn index(self) -> usize {
        Difficulty::ALL.iter().position(|&d| d == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Option<Difficulty> {
        Difficulty::ALL.get(index).copied()
    }
}

/// The current game plus the AI evaluator and which color the local player owns.
pub struct Game {
    board: Board,
    difficulty: Difficulty,
    evaluator: Heuristic,
    /// The color the local player controls. Black in single-player; assigned by
    /// the server in network mode.
    local: Player,
}

impl Game {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            difficulty: Difficulty::Hard,
            evaluator: Heuristic::new(),
            local: Player::Black,
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn local(&self) -> Player {
        self.local
    }

    /// Set which color the local player controls (network mode, on match).
    pub fn set_local(&mut self, player: Player) {
        self.local = player;
    }

    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    pub fn set_difficulty(&mut self, difficulty: Difficulty) {
        self.difficulty = difficulty;
    }

    /// Start a new game, keeping difficulty and local color.
    pub fn restart(&mut self) {
        self.board = Board::new();
    }

    pub fn is_over(&self) -> bool {
        self.board.is_terminal()
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.board.outcome()
    }

    /// Disc counts as `(local, opponent)`.
    pub fn score(&self) -> (u32, u32) {
        (
            self.board.count(self.local),
            self.board.count(self.local.opponent()),
        )
    }

    /// True when it is the local player's turn to move.
    pub fn awaiting_local(&self) -> bool {
        !self.board.is_terminal() && self.board.to_move() == self.local
    }

    /// Single-player: apply the local move, then let the AI reply. Returns the
    /// sequence of transitions (local move, then AI move(s)); empty on an illegal
    /// click or not-your-turn.
    pub fn play_human(&mut self, sq: Square) -> Vec<Transition> {
        let mut transitions = self.play_local(sq);
        if transitions.is_empty() {
            return transitions;
        }
        self.run_ai(&mut transitions);
        transitions
    }

    /// Network: apply only the local player's move at `sq`. Returns its
    /// transition (empty on illegal / not-your-turn). The caller sends the move
    /// to the opponent.
    pub fn play_local(&mut self, sq: Square) -> Vec<Transition> {
        if !self.awaiting_local() {
            return Vec::new();
        }
        match self.apply(sq) {
            Some(transition) => vec![transition],
            None => Vec::new(),
        }
    }

    /// Network: apply the opponent's move received over the wire. Returns `None`
    /// if it isn't the opponent's turn or the move is illegal (a protocol error).
    pub fn apply_remote_move(&mut self, sq: Square) -> Option<Vec<Transition>> {
        if self.board.is_terminal() || self.board.to_move() == self.local {
            return None;
        }
        self.apply(sq).map(|transition| vec![transition])
    }

    /// Apply one move, advancing through any forced passes afterward. Returns the
    /// move's transition, or `None` if the move was illegal.
    fn apply(&mut self, sq: Square) -> Option<Transition> {
        let before = self.board.clone();
        let after = self.board.apply(sq)?;
        self.board = after.clone();
        self.resolve_passes();
        Some(Transition { before, after })
    }

    /// Skip past any side that has no legal move (both clients do this
    /// identically, so passes never need to be sent).
    fn resolve_passes(&mut self) {
        while !self.board.is_terminal() && self.board.legal_moves().is_empty() {
            self.board = self.board.pass();
        }
    }

    /// Let the AI move while it is its turn, recording each move's transition.
    fn run_ai(&mut self, transitions: &mut Vec<Transition>) {
        while !self.board.is_terminal() && self.board.to_move() != self.local {
            match search(&self.board, self.difficulty.depth(), &self.evaluator).best_move {
                Some(mv) => {
                    if let Some(transition) = self.apply(mv) {
                        transitions.push(transition);
                    }
                }
                // Not terminal but the AI has no move: `resolve_passes` already
                // handled it, so this is unreachable in practice.
                None => break,
            }
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two networked clients (one Black, one White) applying the same moves —
    /// one via `play_local`, the other via `apply_remote_move` — must stay
    /// bit-for-bit identical every ply and agree on the outcome. This is the
    /// determinism guarantee the relay relies on.
    #[test]
    fn networked_clients_stay_in_sync() {
        let mut black = Game::new();
        black.set_local(Player::Black);
        let mut white = Game::new();
        white.set_local(Player::White);

        loop {
            assert_eq!(black.board(), white.board(), "boards diverged");
            if black.board().is_terminal() {
                break;
            }
            // Deterministic choice: the first legal move for the side to move.
            let sq = black.board().legal_moves()[0];
            let (mover, receiver) = if black.board().to_move() == Player::Black {
                (&mut black, &mut white)
            } else {
                (&mut white, &mut black)
            };
            assert!(!mover.play_local(sq).is_empty(), "local move should apply");
            assert!(
                receiver.apply_remote_move(sq).is_some(),
                "remote move should apply"
            );
        }

        assert_eq!(black.outcome(), white.outcome());
        assert!(black.outcome().is_some());
    }
}
