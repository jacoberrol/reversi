//! Render the real login screen offscreen to `target/login.png`
//! (`just login-frame`), so we can inspect its look without a live window. Uses
//! `app::login` — the same theme + layout the live app draws.

use std::io::Cursor;

use app::login::{self, LoginAction, LoginForm};

const WIDTH: u32 = 720;
const HEIGHT: u32 = 900;
const PPP: f32 = 2.0;

fn main() {
    // --- headless wgpu ---
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("no GPU adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("egui offscreen"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        },
        None,
    ))
    .expect("device");

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("egui target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // --- egui frame (with a warm-up so fonts are ready) ---
    let ctx = egui::Context::default();
    app::lobby::apply_theme(&ctx);
    ctx.set_pixels_per_point(PPP);

    // The initial state: empty fields show the grey "username"/"password" hints.
    let mut form = LoginForm::default();
    let mut actions: Vec<LoginAction> = Vec::new();

    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(WIDTH as f32 / PPP, HEIGHT as f32 / PPP),
        )),
        ..Default::default()
    };
    let warm = ctx.run(raw.clone(), |ctx| login::ui(ctx, &mut form, &mut actions));
    let output = ctx.run(raw, |ctx| login::ui(ctx, &mut form, &mut actions));
    let mut texture_sets = warm.textures_delta.set;
    texture_sets.extend(output.textures_delta.set.iter().cloned());
    let primitives = ctx.tessellate(output.shapes, PPP);

    let mut egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1);
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [WIDTH, HEIGHT],
        pixels_per_point: PPP,
    };
    for (id, delta) in &texture_sets {
        egui_renderer.update_texture(&device, &queue, *id, delta);
    }

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let user_buffers =
        egui_renderer.update_buffers(&device, &queue, &mut encoder, &primitives, &screen);
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.051,
                        g: 0.063,
                        b: 0.082,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        egui_renderer.render(&mut pass, &primitives, &screen);
    }
    for id in &output.textures_delta.free {
        egui_renderer.free_texture(id);
    }

    // Copy to a padded readback buffer and encode a PNG.
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded_row = WIDTH * 4;
    let padded_row = unpadded_row.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded_row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(
        user_buffers
            .into_iter()
            .chain(std::iter::once(encoder.finish())),
    );

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    device.poll(wgpu::Maintain::Wait);
    let mapped = slice.get_mapped_range();
    let mut rgba = Vec::with_capacity((unpadded_row * HEIGHT) as usize);
    for row in 0..HEIGHT {
        let start = (row * padded_row) as usize;
        rgba.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
    }
    drop(mapped);
    readback.unmap();

    let img = image::RgbaImage::from_raw(WIDTH, HEIGHT, rgba).expect("rgba");
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("png");
    std::fs::create_dir_all("target").ok();
    std::fs::write("target/login.png", &png).expect("write");
    println!("wrote target/login.png ({} bytes)", png.len());
}
