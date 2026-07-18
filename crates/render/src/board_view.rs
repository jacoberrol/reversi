//! Turn a `game-core` board (plus a little UI state) into quad instances, and
//! map pixels back to the board and controls.
//!
//! This is the one place that knows the on-screen layout, so drawing and mouse
//! hit-testing stay consistent. The window and `just frame` both build their
//! frame through [`scene`].

use game_core::{Board, Cell, Outcome, Player, Square, BOARD_SIZE};

use crate::quad::Instance;

// Colors are straight Unorm values (they match the Rgba8Unorm offscreen target
// byte-for-byte, and are close enough on an sRGB window).
const BOARD_LINE: [f32; 4] = [0.04, 0.18, 0.09, 1.0]; // dark green backing = grid lines
const CELL: [f32; 4] = [0.13, 0.50, 0.27, 1.0]; // felt green
const BLACK_DISC: [f32; 4] = [0.06, 0.06, 0.07, 1.0];
const WHITE_DISC: [f32; 4] = [0.93, 0.93, 0.88, 1.0];
const DRAW_DISC: [f32; 4] = [0.55, 0.55, 0.58, 1.0];
const HINT: [f32; 4] = [0.0, 0.0, 0.0, 0.20]; // translucent dot on legal squares
const BUTTON_BG: [f32; 4] = [0.16, 0.17, 0.21, 1.0];
const BUTTON_SEL_BG: [f32; 4] = [0.20, 0.42, 0.68, 1.0];
const BUTTON_BAR: [f32; 4] = [0.86, 0.88, 0.93, 1.0];
const OVERLAY: [f32; 4] = [0.0, 0.0, 0.02, 0.62]; // dims the board when the game is over

/// Number of difficulty buttons in the control strip.
pub const DIFFICULTY_COUNT: usize = 4;

/// Clear color behind everything (the window margins).
pub const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.09,
    g: 0.10,
    b: 0.12,
    a: 1.0,
};

/// What to draw this frame, beyond the board itself.
pub struct View {
    /// Mark the side-to-move's legal moves with hint dots.
    pub show_hints: bool,
    /// Which difficulty button is selected (`0..DIFFICULTY_COUNT`).
    pub selected_difficulty: usize,
    /// `Some` when the game is over: draws the end-of-game overlay.
    pub outcome: Option<Outcome>,
}

/// An axis-aligned rectangle in pixel space.
#[derive(Clone, Copy)]
struct Rect {
    center: [f32; 2],
    half_size: [f32; 2],
}

impl Rect {
    fn contains(&self, x: f32, y: f32) -> bool {
        (x - self.center[0]).abs() <= self.half_size[0]
            && (y - self.center[1]).abs() <= self.half_size[1]
    }
}

/// Screen positions of the board and the control strip, for a given target size.
pub struct Layout {
    board_origin: [f32; 2],
    cell: f32,
    board_size: f32,
    buttons: [Rect; DIFFICULTY_COUNT],
}

/// Compute the layout: a centered square board above a full-width control strip.
pub fn layout(width: f32, height: f32) -> Layout {
    let control_h = (height * 0.14).clamp(44.0, 120.0);
    let board_region = (height - control_h).max(1.0);

    let extent = width.min(board_region);
    let margin = extent * 0.05;
    let board_size = (extent - 2.0 * margin).max(1.0);
    let board_origin = [
        (width - board_size) * 0.5,
        (board_region - board_size) * 0.5,
    ];

    // Difficulty buttons, centered in the control strip.
    let bsize = (control_h * 0.6).min(width * 0.15);
    let gap = bsize * 0.45;
    let total = DIFFICULTY_COUNT as f32 * bsize + (DIFFICULTY_COUNT as f32 - 1.0) * gap;
    let first_cx = (width - total) * 0.5 + bsize * 0.5;
    let cy = board_region + control_h * 0.5;
    let buttons = std::array::from_fn(|i| Rect {
        center: [first_cx + i as f32 * (bsize + gap), cy],
        half_size: [bsize * 0.5, bsize * 0.5],
    });

    Layout {
        board_origin,
        cell: board_size / BOARD_SIZE as f32,
        board_size,
        buttons,
    }
}

/// Which square (if any) contains the pixel `(x, y)`. The inverse of the drawing
/// layout, used to turn a click into a move.
pub fn square_at(layout: &Layout, x: f32, y: f32) -> Option<Square> {
    let lx = x - layout.board_origin[0];
    let ly = y - layout.board_origin[1];
    if lx < 0.0 || ly < 0.0 || lx >= layout.board_size || ly >= layout.board_size {
        return None;
    }
    let col = (lx / layout.cell) as u8;
    let row = (ly / layout.cell) as u8;
    Square::new(row, col)
}

/// Which difficulty button (if any) contains the pixel `(x, y)`.
pub fn difficulty_button_at(layout: &Layout, x: f32, y: f32) -> Option<usize> {
    layout.buttons.iter().position(|b| b.contains(x, y))
}

/// Build every quad for this frame: board, controls, and (if over) the overlay.
pub fn scene(board: &Board, layout: &Layout, view: &View) -> Vec<Instance> {
    let mut out = Vec::new();
    push_board(&mut out, board, layout, view.show_hints);
    push_controls(&mut out, layout, view.selected_difficulty);
    if let Some(outcome) = view.outcome {
        push_overlay(&mut out, layout, outcome);
    }
    out
}

fn push_board(out: &mut Vec<Instance>, board: &Board, layout: &Layout, show_hints: bool) {
    let half = layout.board_size * 0.5;
    let center = [layout.board_origin[0] + half, layout.board_origin[1] + half];
    // Backing rectangle; the gaps between cells reveal it as grid lines.
    out.push(Instance::rect(center, [half, half], BOARD_LINE));

    let gap = layout.cell * 0.03;
    let cell_half = layout.cell * 0.5 - gap;
    let disc_radius = layout.cell * 0.38;
    let hint_radius = layout.cell * 0.12;

    for sq in Square::all() {
        let c = cell_center(layout, sq);
        out.push(Instance::rect(c, [cell_half, cell_half], CELL));
        match board.cell(sq) {
            Cell::Empty => {}
            Cell::Disc(Player::Black) => out.push(Instance::disc(c, disc_radius, BLACK_DISC)),
            Cell::Disc(Player::White) => out.push(Instance::disc(c, disc_radius, WHITE_DISC)),
        }
    }

    if show_hints {
        for sq in board.legal_moves() {
            out.push(Instance::disc(cell_center(layout, sq), hint_radius, HINT));
        }
    }
}

fn push_controls(out: &mut Vec<Instance>, layout: &Layout, selected: usize) {
    for (i, button) in layout.buttons.iter().enumerate() {
        let bg = if i == selected {
            BUTTON_SEL_BG
        } else {
            BUTTON_BG
        };
        out.push(Instance::rect(button.center, button.half_size, bg));

        // A bottom-aligned bar whose height grows with difficulty (1..=4 steps),
        // so the four buttons read as an increasing staircase.
        let pad = button.half_size[1] * 0.28;
        let max_h = button.half_size[1] * 2.0 - 2.0 * pad;
        let bar_h = max_h * (i as f32 + 1.0) / DIFFICULTY_COUNT as f32;
        let bar_bottom = button.center[1] + button.half_size[1] - pad;
        out.push(Instance::rect(
            [button.center[0], bar_bottom - bar_h * 0.5],
            [button.half_size[0] * 0.26, bar_h * 0.5],
            BUTTON_BAR,
        ));
    }
}

fn push_overlay(out: &mut Vec<Instance>, layout: &Layout, outcome: Outcome) {
    let half = layout.board_size * 0.5;
    let center = [layout.board_origin[0] + half, layout.board_origin[1] + half];
    out.push(Instance::rect(center, [half, half], OVERLAY));

    // A large disc in the winner's color (gray for a draw) makes the result
    // obvious at a glance; the exact score goes in the window title.
    let color = match outcome {
        Outcome::Win(Player::Black) => BLACK_DISC,
        Outcome::Win(Player::White) => WHITE_DISC,
        Outcome::Draw => DRAW_DISC,
    };
    out.push(Instance::disc(center, layout.board_size * 0.18, color));
}

/// Pixel center of a square.
fn cell_center(layout: &Layout, sq: Square) -> [f32; 2] {
    [
        layout.board_origin[0] + (f32::from(sq.col()) + 0.5) * layout.cell,
        layout.board_origin[1] + (f32::from(sq.row()) + 0.5) * layout.cell,
    ]
}
