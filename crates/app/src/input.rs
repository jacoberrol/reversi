//! Platform-agnostic pointer input.
//!
//! winit mouse events (macOS today) and touch events (iOS later) both normalize
//! into a [`PointerInput`], so the gameplay/hit-testing code never branches on
//! the platform. Porting to touch means adding one mapping here — nothing below
//! `app` changes.

/// A pointer interaction at a point in the window, in physical pixels with the
/// origin at the top-left.
#[derive(Clone, Copy, Debug)]
pub struct PointerInput {
    pub x: f32,
    pub y: f32,
    pub phase: Phase,
}

/// The kind of pointer interaction. (Motion/drag phases can be added when touch
/// gestures need them; press/release is all v1 gameplay uses.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Pressed,
    Released,
}
