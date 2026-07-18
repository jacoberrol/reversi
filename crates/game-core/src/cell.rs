//! The contents of a single board square.

use crate::Player;

/// What occupies one square: nothing, or a disc belonging to a player.
///
/// Kept as a plain enum (rather than `Option<Player>`) so callers must match
/// exhaustively and the intent reads clearly at each use site.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Empty,
    Disc(Player),
}
