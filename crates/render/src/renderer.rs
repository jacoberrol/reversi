//! The quad batcher: one pipeline that draws every rectangle and disc as an
//! instanced unit quad.

use wgpu::util::DeviceExt;

use crate::quad::{Instance, Vertex, QUAD_INDICES, QUAD_VERTICES};

/// Upper bound on quads per frame. The board needs ~130 (1 backing + 64 cells +
/// up to 64 discs + a few hints), so 256 is comfortable and lets us use one
/// fixed-size instance buffer instead of reallocating.
const MAX_INSTANCES: usize = 256;

/// Uniform block shared by every vertex. `_pad` rounds the struct up to the
/// 16-byte alignment a uniform buffer requires.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    screen_size: [f32; 2],
    _pad: [f32; 2],
}

/// Owns the render pipeline and its buffers. Construct once per GPU device;
/// call [`prepare`](Renderer::prepare) each frame with the instances to draw,
/// then [`draw`](Renderer::draw) inside a render pass.
pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    instances: wgpu::Buffer,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_count: u32,
}

impl Renderer {
    /// Build the pipeline for a given target texture `format` (the window's
    /// surface format, or `Rgba8Unorm` offscreen).
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad pipeline layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });

        // Two vertex buffers: slot 0 is the per-vertex unit quad, slot 1 steps
        // once per instance. Bind these to variables so the attribute arrays
        // outlive the pipeline-descriptor borrow.
        let vertex_attrs = wgpu::vertex_attr_array![0 => Float32x2];
        let instance_attrs = wgpu::vertex_attr_array![
            1 => Float32x2, // center
            2 => Float32x2, // half_size
            3 => Float32x4, // color
            4 => Float32,   // circle
        ];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex_attrs,
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attrs,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[vertex_layout, instance_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad vertices"),
            contents: bytemuck::cast_slice(&QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad indices"),
            contents: bytemuck::cast_slice(&QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (MAX_INSTANCES * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals bind group"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });

        Self {
            pipeline,
            vertices,
            indices,
            instances,
            globals,
            bind_group,
            instance_count: 0,
        }
    }

    /// Upload this frame's instances and the target size. Extra instances beyond
    /// [`MAX_INSTANCES`] are dropped (the board never approaches the cap).
    pub fn prepare(&mut self, queue: &wgpu::Queue, screen_size: [f32; 2], instances: &[Instance]) {
        let count = instances.len().min(MAX_INSTANCES);
        queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals {
                screen_size,
                _pad: [0.0; 2],
            }),
        );
        queue.write_buffer(
            &self.instances,
            0,
            bytemuck::cast_slice(&instances[..count]),
        );
        self.instance_count = count as u32;
    }

    /// Record the instanced draw into an in-progress render pass.
    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..QUAD_INDICES.len() as u32, 0, 0..self.instance_count);
    }
}
