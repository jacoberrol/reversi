//! Pure game core for Reversi (Othello): board, rules, and move generation.
//!
//! This crate depends on nothing but `std`. It knows nothing about rendering,
//! windowing, or I/O, which keeps the rules fully testable with `cargo test`.
//!
//! Invalid states are made unrepresentable where practical, and anything that
//! *can* fail on bad input returns [`Option`]/[`Result`] rather than panicking:
//! a [`Square`] can only be constructed in range, and [`Board::apply`] refuses
//! an illegal move by returning `None`.

mod board;
mod cell;
mod player;
mod square;

pub mod rng;
pub mod selfplay;

pub use board::{Board, Outcome};
pub use cell::Cell;
pub use player::Player;
pub use square::Square;

/// Side length of the board in squares. Reversi is always 8x8.
pub const BOARD_SIZE: usize = 8;

/// Total number of squares on the board (`BOARD_SIZE * BOARD_SIZE`).
pub const NUM_SQUARES: usize = BOARD_SIZE * BOARD_SIZE;
