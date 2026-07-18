//! Application shell: the binary the player runs.
//!
//! Wires `game-core` (rules), `eval` (AI), and `render` (drawing) together and
//! owns the winit event loop. This is the only crate that touches windowing.

fn main() {
    // Stage 4 replaces this with the winit + wgpu window and render loop.
    println!("reversi: window comes in Stage 4. Run `just selfplay 100` for now.");
}
