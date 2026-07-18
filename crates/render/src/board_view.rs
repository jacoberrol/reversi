//! Turn a `game-core` board (plus a little UI/animation state) into quad
//! instances, and map pixels back to the board and controls.
//!
//! This is the one place that knows the on-screen layout, so drawing and mouse
//! hit-testing stay consistent. The window and `just frame` both build their
//! frame through [`scene`].

use std::f32::consts::PI;

use game_core::{Board, Cell, Outcome, Player, Square, BOARD_SIZE};

use crate::quad::Instance;

// Colors are straight Unorm values (they match the Rgba8Unorm offscreen target
// byte-for-byte, and are close enough on an sRGB window).
const FRAME: [f32; 4] = [0.07, 0.09, 0.08, 1.0]; // dark tray around the board
const BOARD_LINE: [f32; 4] = [0.04, 0.18, 0.09, 1.0]; // backing = grid lines
const CELL: [f32; 4] = [0.13, 0.50, 0.27, 1.0]; // felt green
const STAR: [f32; 4] = [0.03, 0.11, 0.06, 1.0]; // Othello star points
const SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.28]; // soft disc shadow
const BLACK_DISC: [f32; 4] = [0.07, 0.07, 0.08, 1.0];
const WHITE_DISC: [f32; 4] = [0.92, 0.92, 0.88, 1.0];
const DRAW_DISC: [f32; 4] = [0.55, 0.55, 0.58, 1.0];
const HINT: [f32; 4] = [0.0, 0.0, 0.0, 0.20];
const BUTTON_BG: [f32; 4] = [0.16, 0.17, 0.21, 1.0];
const BUTTON_SEL_BG: [f32; 4] = [0.20, 0.42, 0.68, 1.0];
const BUTTON_BAR: [f32; 4] = [0.86, 0.88, 0.93, 1.0];
const OVERLAY: [f32; 4] = [0.0, 0.0, 0.02, 0.62];

const CELL_ROUND: f32 = 0.18;
const BUTTON_ROUND: f32 = 0.28;
const FRAME_ROUND: f32 = 0.05;

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
    /// Draw the difficulty control strip (hidden in network mode).
    pub show_controls: bool,
    /// Which difficulty button is selected (`0..DIFFICULTY_COUNT`).
    pub selected_difficulty: usize,
    /// `Some` when the game is over: draws the end-of-game overlay.
    pub outcome: Option<Outcome>,
}

/// How a single disc is animating this frame.
#[derive(Clone, Copy)]
pub enum AnimKind {
    /// A newly placed disc popping in.
    Place,
    /// A disc flipping from `from`'s color to the board's current color.
    Flip { from: Player },
}

/// One animating disc, with progress `t` in `0.0..=1.0`.
#[derive(Clone, Copy)]
pub struct PieceAnim {
    pub square: Square,
    pub kind: AnimKind,
    pub t: f32,
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
    let margin = extent * 0.06;
    let board_size = (extent - 2.0 * margin).max(1.0);
    let board_origin = [
        (width - board_size) * 0.5,
        (board_region - board_size) * 0.5,
    ];

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

/// Which square (if any) contains the pixel `(x, y)`.
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

/// Build every quad for this frame: frame, board, controls, animated discs, and
/// (if over) the overlay. `anims` are drawn in place of their squares' static
/// discs.
pub fn scene(board: &Board, layout: &Layout, view: &View, anims: &[PieceAnim]) -> Vec<Instance> {
    let mut out = Vec::new();

    push_frame_and_board(&mut out, layout);

    // Static discs, skipping any square that is currently animating.
    let mut animating = [false; BOARD_SIZE * BOARD_SIZE];
    for a in anims {
        animating[a.square.index()] = true;
    }
    for sq in Square::all() {
        if animating[sq.index()] {
            continue;
        }
        if let Cell::Disc(player) = board.cell(sq) {
            push_piece(
                &mut out,
                cell_center(layout, sq),
                layout.cell * 0.38,
                disc_color(player),
            );
        }
    }

    if view.show_hints {
        let hint_radius = layout.cell * 0.12;
        for sq in board.legal_moves() {
            out.push(Instance::disc(cell_center(layout, sq), hint_radius, HINT));
        }
    }

    for a in anims {
        push_anim(&mut out, board, layout, a);
    }

    if view.show_controls {
        push_controls(&mut out, layout, view.selected_difficulty);
    }

    if let Some(outcome) = view.outcome {
        push_overlay(&mut out, layout, outcome);
    }

    out
}

fn push_frame_and_board(out: &mut Vec<Instance>, layout: &Layout) {
    let half = layout.board_size * 0.5;
    let center = [layout.board_origin[0] + half, layout.board_origin[1] + half];

    // Tray frame, slightly larger than the board, with gently rounded corners.
    let frame_pad = layout.cell * 0.35;
    out.push(Instance::rounded(
        center,
        [half + frame_pad, half + frame_pad],
        FRAME,
        FRAME_ROUND,
    ));
    // Backing whose gaps between cells read as grid lines.
    out.push(Instance::rect(center, [half, half], BOARD_LINE));

    let gap = layout.cell * 0.03;
    let cell_half = layout.cell * 0.5 - gap;
    for sq in Square::all() {
        out.push(Instance::rounded(
            cell_center(layout, sq),
            [cell_half, cell_half],
            CELL,
            CELL_ROUND,
        ));
    }

    // Star points at the standard 2nd/6th grid-line intersections.
    let star_r = layout.cell * 0.07;
    for &line_y in &[2u8, 6] {
        for &line_x in &[2u8, 6] {
            let p = [
                layout.board_origin[0] + f32::from(line_x) * layout.cell,
                layout.board_origin[1] + f32::from(line_y) * layout.cell,
            ];
            out.push(Instance::disc(p, star_r, STAR));
        }
    }
}

/// A glossy piece with a soft drop shadow beneath it.
fn push_piece(out: &mut Vec<Instance>, center: [f32; 2], radius: f32, color: [f32; 4]) {
    let shadow_center = [center[0], center[1] + radius * 0.14];
    out.push(Instance::disc(shadow_center, radius * 1.03, SHADOW));
    out.push(Instance::piece(center, radius, color));
}

fn push_anim(out: &mut Vec<Instance>, board: &Board, layout: &Layout, anim: &PieceAnim) {
    let center = cell_center(layout, anim.square);
    let radius = layout.cell * 0.38;
    let final_color = match board.cell(anim.square) {
        Cell::Disc(player) => disc_color(player),
        Cell::Empty => return,
    };

    match anim.kind {
        AnimKind::Place => {
            // Pop in with a slight overshoot.
            let scale = ease_out_back(anim.t);
            let r = radius * scale;
            out.push(Instance::disc(
                [center[0], center[1] + r * 0.14],
                r * 1.03,
                SHADOW,
            ));
            out.push(Instance::piece(center, r, final_color));
        }
        AnimKind::Flip { from } => {
            // Edge-on flip: squash horizontally to a line at the midpoint and
            // swap the color as the far face comes around.
            let squash = (PI * anim.t).cos().abs().max(0.04);
            let color = if anim.t < 0.5 {
                disc_color(from)
            } else {
                final_color
            };
            out.push(Instance::disc(
                [center[0], center[1] + radius * 0.14],
                radius * 1.03,
                SHADOW,
            ));
            out.push(Instance::piece_xy(center, [radius * squash, radius], color));
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
        out.push(Instance::rounded(
            button.center,
            button.half_size,
            bg,
            BUTTON_ROUND,
        ));

        // A bottom-aligned bar whose height grows with difficulty (1..=4 steps).
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

    let color = match outcome {
        Outcome::Win(Player::Black) => BLACK_DISC,
        Outcome::Win(Player::White) => WHITE_DISC,
        Outcome::Draw => DRAW_DISC,
    };
    push_piece(out, center, layout.board_size * 0.18, color);
}

fn disc_color(player: Player) -> [f32; 4] {
    match player {
        Player::Black => BLACK_DISC,
        Player::White => WHITE_DISC,
    }
}

/// Ease-out-back: overshoots slightly past 1.0 before settling, for a lively pop.
fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let x = t - 1.0;
    1.0 + c3 * x * x * x + c1 * x * x
}

/// Pixel center of a square.
fn cell_center(layout: &Layout, sq: Square) -> [f32; 2] {
    [
        layout.board_origin[0] + (f32::from(sq.col()) + 0.5) * layout.cell,
        layout.board_origin[1] + (f32::from(sq.row()) + 0.5) * layout.cell,
    ]
}
