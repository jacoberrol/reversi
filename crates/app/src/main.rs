//! Application shell: the binary the player runs.
//!
//! Owns the winit event loop (main thread on macOS) and dispatches window and
//! network events to [`WindowState`]. No args launches single-player vs the AI;
//! `--server ADDR --name NAME` connects to a relay server for online play.

mod anim;
mod game;
mod gpu;
mod input;
mod net;
mod session;

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::Key;
use winit::window::{Window, WindowId};

use gpu::WindowState;
use input::{Phase, PointerInput};
use net::NetEvent;

/// How the app was launched.
enum Launch {
    SinglePlayer,
    Network { addr: String, name: String },
}

/// The application: window state (created in `resumed`) plus a proxy for
/// injecting network events into the loop.
struct App {
    launch: Launch,
    proxy: EventLoopProxy<NetEvent>,
    state: Option<WindowState>,
}

impl ApplicationHandler<NetEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Reversi")
            .with_inner_size(LogicalSize::new(640.0, 720.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("failed to create window"),
        );
        let mut state = WindowState::new(window);

        if let Launch::Network { addr, name } = &self.launch {
            match net::connect(addr, name, self.proxy.clone()) {
                Ok(handle) => state.enter_network(handle),
                Err(e) => state.set_net_error(format!("could not connect to {addr}: {e}")),
            }
        }

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

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Character(s),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let changed = match s.as_str() {
                    "r" | "R" => {
                        state.restart();
                        true
                    }
                    "1" => state.set_difficulty_index(0),
                    "2" => state.set_difficulty_index(1),
                    "3" => state.set_difficulty_index(2),
                    "4" => state.set_difficulty_index(3),
                    _ => false,
                };
                if changed {
                    state.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => state.render(),

            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: NetEvent) {
        if let Some(state) = self.state.as_mut() {
            if state.on_net_event(event) {
                state.request_redraw();
            }
        }
    }
}

fn main() {
    let launch = parse_launch();

    let event_loop = EventLoop::<NetEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();

    let mut app = App {
        launch,
        proxy,
        state: None,
    };
    event_loop.run_app(&mut app).expect("event loop error");
}

/// Parse `--server ADDR` / `--name NAME`; absence of `--server` means
/// single-player.
fn parse_launch() -> Launch {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut server = None;
    let mut name = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--server" => {
                server = args.get(i + 1).cloned();
                i += 2;
            }
            "--name" => {
                name = args.get(i + 1).cloned();
                i += 2;
            }
            _ => i += 1,
        }
    }
    match server {
        Some(addr) => Launch::Network {
            addr,
            name: name.unwrap_or_else(|| "Player".to_string()),
        },
        None => Launch::SinglePlayer,
    }
}
