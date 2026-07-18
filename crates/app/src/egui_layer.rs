//! Live egui integration.
//!
//! We deliberately don't use `egui-winit` (it pins a conflicting winit version),
//! so winit input is hand-translated into egui events — clicks are all the lobby
//! needs. Rendering goes through `egui-wgpu` onto our surface.

use egui::{Event, PointerButton, Pos2};

/// Background the lobby clears to (matches the theme).
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.051,
    g: 0.063,
    b: 0.082,
    a: 1.0,
};

pub struct EguiLayer {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    events: Vec<Event>,
    pointer: Pos2,
}

impl EguiLayer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let ctx = egui::Context::default();
        crate::lobby::apply_theme(&ctx);
        let mut renderer = egui_wgpu::Renderer::new(device, format, None, 1);

        // Warm-up: the first `run` builds the font atlas. Upload it now so the
        // first real frame has fonts both for layout and in the GPU texture
        // (otherwise the first frame renders blank).
        let warm = ctx.run(warm_input(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(" ");
            });
        });
        for (id, delta) in &warm.textures_delta.set {
            renderer.update_texture(device, queue, *id, delta);
        }

        Self {
            ctx,
            renderer,
            events: Vec::new(),
            pointer: Pos2::ZERO,
        }
    }

    /// Feed a cursor position (in points = physical pixels / scale factor).
    pub fn pointer_moved(&mut self, pos_points: [f32; 2]) {
        self.pointer = Pos2::new(pos_points[0], pos_points[1]);
        self.events.push(Event::PointerMoved(self.pointer));
    }

    /// Feed a left mouse button press/release at the current pointer position.
    pub fn pointer_button(&mut self, pressed: bool) {
        self.events.push(Event::PointerButton {
            pos: self.pointer,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }

    /// Run `run_ui` and draw it to `view`, clearing to the lobby background.
    #[allow(clippy::too_many_arguments)] // a wgpu render entry point; grouping adds noise
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size_in_pixels: [u32; 2],
        pixels_per_point: f32,
        run_ui: impl FnOnce(&egui::Context),
    ) {
        self.ctx.set_pixels_per_point(pixels_per_point);
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(
                    size_in_pixels[0] as f32 / pixels_per_point,
                    size_in_pixels[1] as f32 / pixels_per_point,
                ),
            )),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };

        let output = self.ctx.run(raw_input, run_ui);
        let primitives = self.ctx.tessellate(output.shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels,
            pixels_per_point,
        };
        for (id, delta) in &output.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        self.renderer
            .update_buffers(device, queue, encoder, &primitives, &screen);

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer.render(&mut pass, &primitives, &screen);
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}

fn warm_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(100.0, 100.0),
        )),
        ..Default::default()
    }
}
