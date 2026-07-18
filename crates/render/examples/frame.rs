//! `just frame` — render one board frame offscreen to target/frame.png.
//!
//! With no arguments it renders a mid-game position with the controls. Pass
//! `over` (`cargo run -p render --example frame -- over`) to render the
//! game-over overlay instead.

use game_core::Board;
use render::board_view::{self, View};

const WIDTH: u32 = 560;
const HEIGHT: u32 = 640;

fn main() {
    let show_over = std::env::args().nth(1).as_deref() == Some("over");

    // Deterministically play into a position by always taking the first legal
    // move: a handful of plies for a mid-game shot, or to the end for game-over.
    let mut board = Board::new();
    let target_plies = if show_over { 200 } else { 8 };
    for _ in 0..target_plies {
        if board.is_terminal() {
            break;
        }
        match board.legal_moves().first() {
            Some(&sq) => board = board.apply(sq).expect("legal move"),
            None => board = board.pass(),
        }
    }

    let layout = board_view::layout(WIDTH as f32, HEIGHT as f32);
    let view = View {
        show_hints: !show_over,
        selected_difficulty: 2, // "Hard"
        outcome: if show_over { board.outcome() } else { None },
    };
    let instances = board_view::scene(&board, &layout, &view);

    let png = render::offscreen::render_png(&instances, WIDTH, HEIGHT);
    std::fs::create_dir_all("target").ok();
    std::fs::write("target/frame.png", &png).expect("write target/frame.png");
    println!("wrote target/frame.png ({} bytes)", png.len());
}
