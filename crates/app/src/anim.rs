//! Plays a queue of move-transitions as disc animations.
//!
//! Each move becomes a [`Step`]: the board after the move plus the discs that
//! changed (placed or flipped). Steps play one after another over a fixed
//! duration; while any is active the window redraws every frame (see
//! `WindowState::render`), which is what turns the event-driven UI into an
//! animation loop.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use game_core::{Board, Cell, Square};
use render::board_view::{AnimKind, PieceAnim};

use crate::game::Transition;

/// How long each move's animation lasts.
const STEP_DURATION: Duration = Duration::from_millis(300);

/// One move's worth of animation.
struct Step {
    /// Board after the move (drawn statically, minus the animating discs).
    board: Board,
    /// The discs that changed, and how.
    specs: Vec<(Square, AnimKind)>,
}

/// Plays queued move-transitions as animations, one after another.
#[derive(Default)]
pub struct Animator {
    queue: VecDeque<Step>,
    /// When the head step started; `None` until the next frame picks it up.
    started: Option<Instant>,
}

impl Animator {
    /// Whether any animation is still queued or playing.
    pub fn is_active(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Queue the animations for a sequence of move transitions.
    pub fn push(&mut self, transitions: Vec<Transition>) {
        for transition in transitions {
            let specs = diff(&transition.before, &transition.after);
            if !specs.is_empty() {
                self.queue.push_back(Step {
                    board: transition.after,
                    specs,
                });
            }
        }
    }

    /// Cancel everything (e.g. on restart).
    pub fn clear(&mut self) {
        self.queue.clear();
        self.started = None;
    }

    /// Advance to `now`; return the board to draw plus its live animations, or
    /// `None` when idle.
    pub fn frame(&mut self, now: Instant) -> Option<(Board, Vec<PieceAnim>)> {
        loop {
            let elapsed = {
                let start = *self.started.get_or_insert(now);
                now.saturating_duration_since(start)
            };
            if self.queue.front().is_none() {
                self.started = None;
                return None;
            }
            if elapsed >= STEP_DURATION {
                self.queue.pop_front();
                self.started = None;
                continue;
            }
            let step = self.queue.front().expect("front checked above");
            let t = elapsed.as_secs_f32() / STEP_DURATION.as_secs_f32();
            let anims = step
                .specs
                .iter()
                .map(|&(square, kind)| PieceAnim { square, kind, t })
                .collect();
            return Some((step.board.clone(), anims));
        }
    }
}

/// Diff two boards into per-disc animations for the move between them.
fn diff(before: &Board, after: &Board) -> Vec<(Square, AnimKind)> {
    Square::all()
        .filter_map(|square| {
            let kind = match (before.cell(square), after.cell(square)) {
                (Cell::Empty, Cell::Disc(_)) => AnimKind::Place,
                (Cell::Disc(from), Cell::Disc(to)) if from != to => AnimKind::Flip { from },
                _ => return None,
            };
            Some((square, kind))
        })
        .collect()
}
