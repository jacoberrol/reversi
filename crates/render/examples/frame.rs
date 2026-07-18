//! `just frame` — render one board frame offscreen to target/frame.png.
//!
//! Modes (first arg): none = mid-game with controls; `over` = game-over overlay;
//! `flip` = a move mid-animation (to check the disc squash and pop-in).

use game_core::{Board, Cell, Square};
use render::board_view::{self, AnimKind, PieceAnim, View};

const WIDTH: u32 = 560;
const HEIGHT: u32 = 640;

fn main() {
    let mode = std::env::args().nth(1);
    let mode = mode.as_deref().unwrap_or("play");

    let mut board = Board::new();
    let plies = if mode == "over" { 200 } else { 6 };
    for _ in 0..plies {
        if board.is_terminal() {
            break;
        }
        match board.legal_moves().first() {
            Some(&sq) => board = board.apply(sq).expect("legal move"),
            None => board = board.pass(),
        }
    }

    // For `flip`, take one more move and animate its discs mid-flight.
    let mut anims = Vec::new();
    if mode == "flip" {
        if let Some(&mv) = board.legal_moves().first() {
            let before = board.clone();
            board = board.apply(mv).expect("legal move");
            anims = anims_between(&before, &board, 0.4);
        }
    }

    let layout = board_view::layout(WIDTH as f32, HEIGHT as f32);
    let view = View {
        show_hints: mode == "play",
        selected_difficulty: 2,
        outcome: if mode == "over" {
            board.outcome()
        } else {
            None
        },
    };
    let instances = board_view::scene(&board, &layout, &view, &anims);

    let png = render::offscreen::render_png(&instances, WIDTH, HEIGHT);
    std::fs::create_dir_all("target").ok();
    std::fs::write("target/frame.png", &png).expect("write target/frame.png");
    println!("wrote target/frame.png ({} bytes, mode={mode})", png.len());
}

/// Diff two boards into the disc animations for the move between them.
fn anims_between(before: &Board, after: &Board, t: f32) -> Vec<PieceAnim> {
    Square::all()
        .filter_map(|square| {
            let kind = match (before.cell(square), after.cell(square)) {
                (Cell::Empty, Cell::Disc(_)) => AnimKind::Place,
                (Cell::Disc(from), Cell::Disc(to)) if from != to => AnimKind::Flip { from },
                _ => return None,
            };
            Some(PieceAnim { square, kind, t })
        })
        .collect()
}
