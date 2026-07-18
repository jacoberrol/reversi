//! A validated board coordinate.

use crate::BOARD_SIZE;

const SIZE: u8 = BOARD_SIZE as u8;

/// A position on the board, stored as a single index in `0..64`.
///
/// The inner value is private and every constructor is range-checked, so an
/// out-of-bounds `Square` cannot be built — indexing the board with one is
/// therefore always safe.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Square(u8);

impl Square {
    /// Build a square from a `(row, col)` pair, or `None` if either is `>= 8`.
    pub fn new(row: u8, col: u8) -> Option<Square> {
        if row < SIZE && col < SIZE {
            Some(Square(row * SIZE + col))
        } else {
            None
        }
    }

    /// Build a square from a flat index, or `None` if `index >= 64`.
    pub fn from_index(index: usize) -> Option<Square> {
        if index < BOARD_SIZE * BOARD_SIZE {
            Some(Square(index as u8))
        } else {
            None
        }
    }

    /// The flat index in `0..64`. Always valid as a board array index.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Row (0 at the top), in `0..8`.
    pub fn row(self) -> u8 {
        self.0 / SIZE
    }

    /// Column (0 at the left), in `0..8`.
    pub fn col(self) -> u8 {
        self.0 % SIZE
    }

    /// Iterator over every square, in flat-index order.
    pub fn all() -> impl Iterator<Item = Square> {
        (0..(BOARD_SIZE * BOARD_SIZE) as u8).map(Square)
    }
}
