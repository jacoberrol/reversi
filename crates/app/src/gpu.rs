//! Per-window GPU state: surface, device, the [`Renderer`], and the [`Session`]
//! it draws. Game/network logic lives in `session`; this file is wgpu + winit.
//!
//! wgpu concepts in brief:
//! - **surface**: the swapchain the window presents; invalid on resize/display
//!   change and reconfigured in [`WindowState::render`].
//! - **device / queue**: the logical GPU and its submission queue.
//! - **config**: format + size the surface is configured for.

use std::sync::Arc;
use std::time::Instant;

use render::{board_view, Renderer};
use winit::window::Window;

use crate::input::{Phase, PointerInput};
use crate::net::{NetEvent, NetHandle};
use crate::session::Session;

pub struct WindowState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    session: Session,
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
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, format);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            session: Session::new(),
            cursor: [0.0, 0.0],
        }
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

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

    // --- session delegation ---------------------------------------------

    pub fn enter_network(&mut self, handle: NetHandle) {
        self.session.enter_network(handle);
    }

    pub fn set_net_error(&mut self, message: String) {
        self.session.set_net_error(message);
    }

    pub fn on_net_event(&mut self, event: NetEvent) -> bool {
        self.session.on_net_event(event)
    }

    pub fn restart(&mut self) {
        self.session.restart();
    }

    pub fn set_difficulty_index(&mut self, index: usize) -> bool {
        self.session.set_difficulty_index(index)
    }

    /// Handle a pointer event. Returns `true` if something changed (redraw
    /// needed).
    pub fn handle_pointer(&mut self, input: PointerInput) -> bool {
        if input.phase != Phase::Pressed {
            return false;
        }
        let layout = board_view::layout(self.config.width as f32, self.config.height as f32);

        // Difficulty buttons exist only in single-player mode.
        if !self.session.is_network() {
            if let Some(index) = board_view::difficulty_button_at(&layout, input.x, input.y) {
                return self.session.set_difficulty_index(index);
            }
        }

        match board_view::square_at(&layout, input.x, input.y) {
            Some(sq) => self.session.click_square(sq),
            None => false,
        }
    }

    /// Draw the current state to the window.
    pub fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(_) => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Keep the title in sync with state (our text stand-in).
        self.window.set_title(&self.session.title());

        let size = [self.config.width as f32, self.config.height as f32];
        let layout = board_view::layout(size[0], size[1]);
        let (board, anims) = self.session.frame(Instant::now());
        let scene = self.session.view(!anims.is_empty());
        let instances = board_view::scene(&board, &layout, &scene, &anims);
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

        // Keep frames coming while a move animates.
        if self.session.is_animating() {
            self.request_redraw();
        }
    }
}
