//! Rendering: a thin wgpu quad batcher and offscreen frame capture.
//!
//! No game logic lives here — the renderer reads `game-core` state and draws it.
//! Textures are stubbed for now; v1 is solid-colour quads and procedural discs.

pub mod board_view;
pub mod offscreen;
mod quad;
mod renderer;

pub use quad::Instance;
pub use renderer::Renderer;
