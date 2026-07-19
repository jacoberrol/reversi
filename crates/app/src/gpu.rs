//! Per-window GPU state: surface, device, the board [`Renderer`], the
//! [`EguiLayer`] for the lobby, and the [`Session`] they draw. Game/lobby logic
//! lives in `session`; this file is wgpu + winit + screen routing.

use std::sync::Arc;
use std::time::Instant;

use render::{board_view, Renderer};
use winit::event::{ElementState, KeyEvent};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;

use netplay_client::NetEvent;

use crate::egui_layer::EguiLayer;
use crate::session::Session;

/// What the login screen needs to actually connect: the relay URL and the proxy
/// to deliver network events back to the event loop.
struct NetContext {
    proxy: EventLoopProxy<NetEvent>,
    url: String,
}

pub struct WindowState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    egui: EguiLayer,
    session: Session,
    /// Set in network mode; how the login screen connects.
    net: Option<NetContext>,
    /// Last cursor position (physical pixels).
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
        let egui = EguiLayer::new(&device, &queue, format);

        Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            egui,
            session: Session::new(),
            net: None,
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

    // --- input ----------------------------------------------------------

    pub fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = [x, y];
        if self.session.uses_egui() {
            let ppp = self.window.scale_factor() as f32;
            self.egui.pointer_moved([x / ppp, y / ppp]);
            self.request_redraw();
        }
    }

    pub fn mouse_button(&mut self, pressed: bool) {
        if self.session.uses_egui() {
            self.egui.pointer_button(pressed);
            self.request_redraw();
        } else if pressed {
            let [x, y] = self.cursor;
            if self.handle_game_click(x, y) {
                self.request_redraw();
            }
        }
    }

    /// True while the login screen is up (routes keyboard to the text fields).
    pub fn is_login(&self) -> bool {
        self.session.is_login()
    }

    /// Feed a keyboard event into the login form's text fields.
    pub fn login_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if let Some(key) = to_egui_key(&event.logical_key) {
            self.egui.key(key);
        }
        if let Some(text) = &event.text {
            let printable: String = text.chars().filter(|c| !c.is_control()).collect();
            if !printable.is_empty() {
                self.egui.text(printable);
            }
        }
        self.request_redraw();
    }

    fn handle_game_click(&mut self, x: f32, y: f32) -> bool {
        let layout = board_view::layout(self.config.width as f32, self.config.height as f32);
        if !self.session.is_network() {
            if let Some(index) = board_view::difficulty_button_at(&layout, x, y) {
                return self.session.set_difficulty_index(index);
            }
        }
        match board_view::square_at(&layout, x, y) {
            Some(sq) => self.session.click_square(sq),
            None => false,
        }
    }

    pub fn restart(&mut self) -> bool {
        if self.session.is_lobby() {
            return false;
        }
        self.session.restart();
        true
    }

    pub fn set_difficulty_index(&mut self, index: usize) -> bool {
        self.session.set_difficulty_index(index)
    }

    // --- network --------------------------------------------------------

    /// Enter network mode on the login screen, pre-filling the remembered name.
    pub fn begin_login(&mut self, url: String, proxy: EventLoopProxy<NetEvent>) {
        self.net = Some(NetContext { proxy, url });
        self.session.begin_login(crate::config::load_username());
        self.request_redraw();
    }

    /// Handle a login/register submission: validate, connect, remember the name.
    fn submit_login(&mut self, register: bool) {
        if self.session.login_connecting() {
            return;
        }
        let (name, password) = {
            let form = self.session.login_form();
            (form.name.trim().to_string(), form.password.clone())
        };
        if name.is_empty() {
            self.session.login_error("enter a username".to_string());
            return;
        }
        if register && password.len() < 8 {
            self.session
                .login_error("password too short (min 8 characters)".to_string());
            return;
        }
        let Some(net) = &self.net else { return };
        let credential =
            serde_json::json!({ "name": name, "password": password, "register": register });
        let handle = netplay_client::connect(&net.url, &name, credential, net.proxy.clone());
        crate::config::save_username(&name);
        self.session.start_connecting(handle);
    }

    pub fn on_net_event(&mut self, event: NetEvent) -> bool {
        self.session.on_net_event(event)
    }

    // --- rendering ------------------------------------------------------

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
        self.window.set_title(&self.session.title());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        if self.session.is_login() {
            self.render_login(&mut encoder, &view);
            self.queue.submit(std::iter::once(encoder.finish()));
            frame.present();
        } else if self.session.is_lobby() {
            self.render_lobby(&mut encoder, &view);
            self.queue.submit(std::iter::once(encoder.finish()));
            frame.present();
        } else {
            self.render_game(&mut encoder, &view);
            self.queue.submit(std::iter::once(encoder.finish()));
            frame.present();
            if self.session.is_animating() {
                self.request_redraw();
            }
        }
    }

    fn render_login(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let ppp = self.window.scale_factor() as f32;
        let size = [self.config.width, self.config.height];
        let mut actions = Vec::new();
        {
            let egui = &mut self.egui;
            let form = self.session.login_form_mut();
            egui.render(&self.device, &self.queue, encoder, view, size, ppp, |ctx| {
                crate::login::ui(ctx, form, &mut actions);
            });
        }
        for action in actions {
            let crate::login::LoginAction::Submit { register } = action;
            self.submit_login(register);
        }
    }

    fn render_lobby(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let ppp = self.window.scale_factor() as f32;
        let size = [self.config.width, self.config.height];
        let mut actions = Vec::new();
        {
            let egui = &mut self.egui;
            let session = &self.session;
            egui.render(&self.device, &self.queue, encoder, view, size, ppp, |ctx| {
                crate::lobby::ui(ctx, session.lobby_state(), &mut actions);
            });
        }
        let had_actions = !actions.is_empty();
        for action in actions {
            self.session.lobby_action(action);
        }
        if had_actions {
            self.request_redraw();
        }
    }

    fn render_game(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let size = [self.config.width as f32, self.config.height as f32];
        let layout = board_view::layout(size[0], size[1]);
        let (board, anims) = self.session.frame(Instant::now());
        let scene = self.session.view(!anims.is_empty());
        let instances = board_view::scene(&board, &layout, &scene, &anims);
        self.renderer.prepare(&self.queue, size, &instances);

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("board pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
}

/// Map the editing keys a text field needs from winit to egui (typed characters
/// come through `KeyEvent::text` instead).
fn to_egui_key(key: &Key) -> Option<egui::Key> {
    Some(match key {
        Key::Named(NamedKey::Backspace) => egui::Key::Backspace,
        Key::Named(NamedKey::Delete) => egui::Key::Delete,
        Key::Named(NamedKey::Enter) => egui::Key::Enter,
        Key::Named(NamedKey::Tab) => egui::Key::Tab,
        Key::Named(NamedKey::ArrowLeft) => egui::Key::ArrowLeft,
        Key::Named(NamedKey::ArrowRight) => egui::Key::ArrowRight,
        Key::Named(NamedKey::ArrowUp) => egui::Key::ArrowUp,
        Key::Named(NamedKey::ArrowDown) => egui::Key::ArrowDown,
        Key::Named(NamedKey::Home) => egui::Key::Home,
        Key::Named(NamedKey::End) => egui::Key::End,
        Key::Named(NamedKey::Escape) => egui::Key::Escape,
        _ => return None,
    })
}
