//! Application shell library: the winit event loop and window/lobby wiring.
//!
//! Exposed as a library (with a thin `main.rs`) so the offscreen lobby mockup
//! example can reuse the real lobby code.
//!
//! No args launches single-player vs the AI. `--online` connects to the public
//! relay ([`DEFAULT_RELAY_URL`]); `--server URL` overrides it with a specific
//! relay (e.g. a local `ws://` server). The display name comes from the account
//! you log in with (or register) on the title screen.

pub mod anim;
pub mod config;
pub mod egui_layer;
pub mod game;
pub mod game_msg;
pub mod gpu;
pub mod lobby;
pub mod login;
pub mod session;

use std::sync::Arc;

use netplay_client::NetEvent;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::Key;
use winit::window::{Window, WindowId};

use gpu::WindowState;

/// The public relay's client-facing URL. TLS is terminated by the exe.dev proxy,
/// which forwards to the plain-`ws://` relay on the VM. Baked in so `--online`
/// needs no address; `--server URL` overrides it (e.g. a local dev server).
pub const DEFAULT_RELAY_URL: &str = "wss://relay.netplay.oliverj.network";

/// How the app was launched.
enum Launch {
    SinglePlayer,
    Network { url: String },
}

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

        if let Launch::Network { url } = &self.launch {
            // Network mode starts on the login screen; the connection is made
            // when the player logs in or registers.
            state.begin_login(url.clone(), self.proxy.clone());
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
                state.mouse_button(button_state == ElementState::Pressed);
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if state.is_login() {
                    // On the login screen, keys drive the text fields.
                    state.login_key(&event);
                } else if event.state == ElementState::Pressed {
                    if let Key::Character(s) = &event.logical_key {
                        let changed = match s.as_str() {
                            "r" | "R" => state.restart(),
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

/// Run the app (single-player, or online if `--server` is given).
pub fn run() {
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

/// Parse the launch flags. `--online` joins the public relay; `--server URL`
/// overrides it with a specific relay. With neither, launch single-player. (The
/// name comes from the login screen now, not a flag.)
fn parse_launch() -> Launch {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut server = None;
    let mut online = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--server" => {
                server = args.get(i + 1).cloned();
                i += 2;
            }
            "--online" => {
                online = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    // An explicit `--server` wins; otherwise `--online` uses the baked-in relay.
    let url = server.or_else(|| online.then(|| DEFAULT_RELAY_URL.to_string()));
    match url {
        Some(url) => Launch::Network { url },
        None => Launch::SinglePlayer,
    }
}
