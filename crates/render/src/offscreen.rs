//! Headless rendering to a PNG, for `just frame` (we can't see the screen, so we
//! inspect a rendered image instead).
//!
//! wgpu can render without a window: we make a texture, draw into it, copy it to
//! a CPU-mappable buffer, read the bytes back, and encode a PNG. The one fiddly
//! detail is that texture-to-buffer copies require each row to be a multiple of
//! 256 bytes, so we pad the copy and strip the padding afterwards.

use std::io::Cursor;

use game_core::Board;

use crate::{board_view, Renderer};

/// Render `board` to a `width` x `height` frame and return PNG bytes.
pub fn render_board_png(board: &Board, width: u32, height: u32) -> Vec<u8> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    // No surface: ask for any adapter that can render.
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("no suitable GPU adapter found");

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("offscreen device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        },
        None,
    ))
    .expect("failed to create device");

    // Non-sRGB so the bytes we read back match our colour literals directly.
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut renderer = Renderer::new(&device, format);
    let layout = board_view::layout(width as f32, height as f32);
    let instances = board_view::instances(board, &layout, Some(board.to_move()));
    renderer.prepare(&queue, [width as f32, height as f32], &instances);

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("board pass"),
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
        renderer.draw(&mut pass);
    }

    // Copy the texture into a mappable buffer, padding rows to the 256-byte
    // alignment wgpu requires for texture-to-buffer copies.
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded_row = width * 4;
    let padded_row = unpadded_row.div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback buffer"),
        size: u64::from(padded_row) * u64::from(height),
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
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    // Map the buffer and wait for the GPU to finish (poll drives the callback).
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("buffer map failed")
    });
    device.poll(wgpu::Maintain::Wait);
    let mapped = slice.get_mapped_range();

    // Strip the per-row padding into a tight RGBA image.
    let mut rgba = Vec::with_capacity((unpadded_row * height) as usize);
    for row in 0..height {
        let start = (row * padded_row) as usize;
        let end = start + unpadded_row as usize;
        rgba.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    buffer.unmap();

    let img = image::RgbaImage::from_raw(width, height, rgba).expect("rgba buffer size mismatch");
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("PNG encode failed");
    png
}
