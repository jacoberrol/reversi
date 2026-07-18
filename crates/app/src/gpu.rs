//! Per-window GPU state: surface, device, the [`Renderer`], and the game it draws.
//!
//! wgpu concepts in brief:
//! - **surface**: the swapchain of images the window presents. It becomes
//!   invalid on resize / display change and must be reconfigured (handled in
//!   [`WindowState::render`]).
//! - **device / queue**: the logical GPU and its command submission queue.
//! - **config**: format + size the surface is currently configured for.

use std::sync::Arc;

use game_core::{Outcome, Player};
use render::{board_view, Renderer};
use winit::window::Window;

use crate::game::{Difficulty, Game};
use crate::input::{Phase, PointerInput};

pub struct WindowState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    game: Game,
    /// Last known cursor position (physical pixels); winit's click event doesn't
    /// carry a position, so we track it from cursor-moved events.
    cursor: [f32; 2],
}

impl WindowState {
    pub fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        // Arc<Window> gives the surface a 'static lifetime tied to the window.
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no suitable GPU adapter found");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("window device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .expect("failed to create device");

        let caps = surface.get_capabilities(&adapter);
        // Prefer a non-sRGB format so on-screen colours match the offscreen
        // frame; fall back to whatever the surface prefers.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo, // vsync; universally supported
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, format);

        let state = Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            game: Game::new(),
            cursor: [0.0, 0.0],
        };
        state.update_title();
        state
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Reconfigure the surface for a new window size.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = [x, y];
    }

    pub fn cursor(&self) -> [f32; 2] {
        self.cursor
    }

    /// Start a new game.
    pub fn restart(&mut self) {
        self.game.restart();
        self.update_title();
    }

    /// Select difficulty by button index (`0..4`). Returns whether it changed.
    pub fn set_difficulty_index(&mut self, index: usize) -> bool {
        match Difficulty::from_index(index) {
            Some(difficulty) => {
                self.game.set_difficulty(difficulty);
                self.update_title();
                true
            }
            None => false,
        }
    }

    /// Handle a pointer event. Returns `true` if something changed (redraw
    /// needed). Clicks route to the difficulty buttons, then to restart (when the
    /// game is over), then to placing a move.
    pub fn handle_pointer(&mut self, input: PointerInput) -> bool {
        if input.phase != Phase::Pressed {
            return false;
        }
        let layout = board_view::layout(self.config.width as f32, self.config.height as f32);

        if let Some(index) = board_view::difficulty_button_at(&layout, input.x, input.y) {
            return self.set_difficulty_index(index);
        }

        if self.game.is_over() {
            self.restart();
            return true;
        }

        let changed = match board_view::square_at(&layout, input.x, input.y) {
            Some(sq) => self.game.play_human(sq),
            None => false,
        };
        if changed {
            self.update_title();
        }
        changed
    }

    /// Reflect the current state in the window title (our stand-in for on-screen
    /// text until a glyph renderer exists).
    fn update_title(&self) {
        let (human, ai) = self.game.score();
        let status = match self.game.outcome() {
            Some(Outcome::Win(Player::Black)) => {
                format!("You win {human}\u{2013}{ai} \u{00b7} click board for a new game")
            }
            Some(Outcome::Win(Player::White)) => {
                format!("AI wins {ai}\u{2013}{human} \u{00b7} click board for a new game")
            }
            Some(Outcome::Draw) => {
                format!("Draw {human}\u{2013}{ai} \u{00b7} click board for a new game")
            }
            None => "Your move".to_string(),
        };
        self.window.set_title(&format!(
            "Reversi \u{2014} {status} \u{00b7} {}",
            self.game.difficulty().name()
        ));
    }

    /// Draw the current board to the window.
    pub fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            // Surface lost/outdated (resize, display change): reconfigure and
            // skip this frame rather than panicking.
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            // Timeout / out-of-memory: skip this frame.
            Err(_) => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let size = [self.config.width as f32, self.config.height as f32];
        let layout = board_view::layout(size[0], size[1]);
        let scene = board_view::View {
            show_hints: self.game.awaiting_human(),
            selected_difficulty: self.game.difficulty().index(),
            outcome: self.game.outcome(),
        };
        let instances = board_view::scene(self.game.board(), &layout, &scene);
        self.renderer.prepare(&self.queue, size, &instances);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(board_view::BACKGROUND),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer.draw(&mut pass);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}
