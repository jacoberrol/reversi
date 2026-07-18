//! Rendering: a thin wgpu sprite/quad batcher and atlas loading.
//!
//! No game logic lives here — the renderer reads `game-core` state and draws it.
//! Keeping this crate free of rules keeps both sides independently testable.

// Stage 4 fills this in with wgpu setup and the instanced-quad batcher.
