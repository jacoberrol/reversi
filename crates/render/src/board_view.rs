//! Turn a `game-core` board into quad instances, and map pixels back to squares.
//!
//! This is the one place that knows how the board is laid out on screen, so both
//! drawing and mouse hit-testing stay consistent (`app` inverts `square_at` to
//! translate clicks into moves).

use game_core::{Board, Cell, Player, Square, BOARD_SIZE};

use crate::quad::Instance;

// Colors are straight Unorm values (they match the Rgba8Unorm offscreen target
// byte-for-byte, and are close enough on an sRGB window).
const BOARD_LINE: [f32; 4] = [0.04, 0.18, 0.09, 1.0]; // dark green backing = grid lines
const CELL: [f32; 4] = [0.13, 0.50, 0.27, 1.0]; // felt green
const BLACK_DISC: [f32; 4] = [0.06, 0.06, 0.07, 1.0];
const WHITE_DISC: [f32; 4] = [0.93, 0.93, 0.88, 1.0];
const HINT: [f32; 4] = [0.0, 0.0, 0.0, 0.20]; // translucent dot on legal squares

/// Clear color behind the board (the window margin).
pub const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.09,
    g: 0.10,
    b: 0.12,
    a: 1.0,
};

/// Where the board sits within a `width` x `height` target: a centered square
/// with a small margin.
pub struct Layout {
    /// Top-left of the board, in pixels.
    pub origin: [f32; 2],
    /// Side length of one cell, in pixels.
    pub cell: f32,
    /// Side length of the whole board, in pixels.
    pub size: f32,
}

/// Compute the board layout for a target of the given pixel size.
pub fn layout(width: f32, height: f32) -> Layout {
    let extent = width.min(height);
    let margin = extent * 0.05;
    let size = extent - 2.0 * margin;
    let origin = [(width - size) * 0.5, (height - size) * 0.5];
    Layout {
        origin,
        cell: size / BOARD_SIZE as f32,
        size,
    }
}

/// Which square (if any) contains the pixel `(x, y)`. The inverse of the drawing
/// layout, used to turn a click into a move.
pub fn square_at(layout: &Layout, x: f32, y: f32) -> Option<Square> {
    let lx = x - layout.origin[0];
    let ly = y - layout.origin[1];
    if lx < 0.0 || ly < 0.0 || lx >= layout.size || ly >= layout.size {
        return None;
    }
    let col = (lx / layout.cell) as u8;
    let row = (ly / layout.cell) as u8;
    Square::new(row, col)
}

/// Build the quads for `board`. If `show_hints_for` is the side to move, their
/// legal moves are marked with translucent dots.
pub fn instances(board: &Board, layout: &Layout, show_hints_for: Option<Player>) -> Vec<Instance> {
    let mut out = Vec::new();

    let half_board = layout.size * 0.5;
    let board_center = [layout.origin[0] + half_board, layout.origin[1] + half_board];
    // Backing rectangle; the gaps between cells reveal it as grid lines.
    out.push(Instance::rect(
        board_center,
        [half_board, half_board],
        BOARD_LINE,
    ));

    let gap = layout.cell * 0.03;
    let cell_half = layout.cell * 0.5 - gap;
    let disc_radius = layout.cell * 0.38;
    let hint_radius = layout.cell * 0.12;

    for sq in Square::all() {
        let center = cell_center(layout, sq);
        out.push(Instance::rect(center, [cell_half, cell_half], CELL));
        match board.cell(sq) {
            Cell::Empty => {}
            Cell::Disc(Player::Black) => {
                out.push(Instance::disc(center, disc_radius, BLACK_DISC));
            }
            Cell::Disc(Player::White) => {
                out.push(Instance::disc(center, disc_radius, WHITE_DISC));
            }
        }
    }

    if let Some(player) = show_hints_for {
        if board.to_move() == player {
            for sq in board.legal_moves() {
                out.push(Instance::disc(cell_center(layout, sq), hint_radius, HINT));
            }
        }
    }

    out
}

/// Pixel center of a square.
fn cell_center(layout: &Layout, sq: Square) -> [f32; 2] {
    [
        layout.origin[0] + (f32::from(sq.col()) + 0.5) * layout.cell,
        layout.origin[1] + (f32::from(sq.row()) + 0.5) * layout.cell,
    ]
}
