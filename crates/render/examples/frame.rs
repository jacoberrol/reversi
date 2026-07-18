//! `just frame` — render one board frame offscreen to target/frame.png.

use game_core::Board;

fn main() {
    // A few moves in, so the frame shows both colours and legal-move hints.
    let mut board = Board::new();
    for sq in board.legal_moves().into_iter().take(1) {
        board = board.apply(sq).expect("legal opening move");
    }

    let png = render::offscreen::render_board_png(&board, 512, 512);
    std::fs::create_dir_all("target").ok();
    std::fs::write("target/frame.png", &png).expect("write target/frame.png");
    println!("wrote target/frame.png ({} bytes)", png.len());
}
