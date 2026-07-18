//! Application shell: the binary the player runs (`just run`).
//!
//! Owns the winit event loop (which must live on the main thread on macOS) and
//! dispatches window events to [`WindowState`]. Wires `game-core` (rules),
//! `eval` (AI), and `render` (drawing) together.

mod game;
mod gpu;
mod input;

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use gpu::WindowState;
use input::{Phase, PointerInput};

/// The application: holds the window state once the event loop is running.
///
/// winit creates the window in `resumed` (not `main`), so the GPU state is
/// `None` until then — the standard winit 0.30 lifecycle.
#[derive(Default)]
struct App {
    state: Option<WindowState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return; // already have a window (e.g. after a suspend/resume)
        }
        let attributes = Window::default_attributes()
            .with_title("Reversi")
            .with_inner_size(LogicalSize::new(640.0, 640.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("failed to create window"),
        );
        let state = WindowState::new(window);
        state.request_redraw();
        self.state = Some(state);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                state.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.set_cursor(position.x as f32, position.y as f32);
            }

            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                let phase = match button_state {
                    ElementState::Pressed => Phase::Pressed,
                    ElementState::Released => Phase::Released,
                };
                let [x, y] = state.cursor();
                if state.handle_pointer(PointerInput { x, y, phase }) {
                    state.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => state.render(),

            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    // Wait for events rather than spinning; this is a turn-based game, so we
    // only redraw when something changes.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("event loop error");
}
