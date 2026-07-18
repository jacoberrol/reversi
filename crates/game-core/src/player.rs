//! The two sides in a game of Reversi.

/// One of the two players. Black moves first, by convention.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Player {
    Black,
    White,
}

impl Player {
    /// The other player.
    pub fn opponent(self) -> Player {
        // Exhaustive on purpose: adding a variant should force a decision here.
        match self {
            Player::Black => Player::White,
            Player::White => Player::Black,
        }
    }
}
