//! Draws the frame's particles at the end of the main pass: camera-facing quads,
//! farthest first, over the opaque scene. They read depth but don't write it.

use grimora_core::{BoundingSphere, Camera, Frustum, ParticleInstance, Vec3};

use crate::HDR_FORMAT;
use crate::renderer::depth_state;

const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

pub(crate) struct ParticlePass {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
    /// What survived culling, farthest first; kept to reuse its memory.
    sorted: Vec<ParticleInstance>,
}

impl ParticlePass {
    pub fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        globals_layout: &wgpu::BindGroupLayout,
        msaa: u32,
    ) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &[Some(globals_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_particle"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<ParticleInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: Some(depth_state(false, wgpu::CompareFunction::Less, Default::default())),
            multisample: wgpu::MultisampleState { count: msaa, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_particle"),
                compilation_options: Default::default(),
                // Premultiplied: alpha 0 adds light, alpha 1 covers what is behind,
                // so fire and smoke sort together in one draw call.
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let capacity = 1024;
        Self { pipeline, buffer: particle_buffer(device, capacity), capacity, count: 0, sorted: Vec::new() }
    }

    /// Culls and sorts this frame's particles and uploads them. Returns how many will be drawn.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        particles: &[ParticleInstance],
        camera: &Camera,
        aspect: f32,
    ) -> u32 {
        let view = Frustum::from_view_projection(camera.view_projection(aspect));
        self.sorted.clear();
        self.sorted.extend(particles.iter().filter(|p| view.intersects_sphere(bounds(p))));
        let forward = camera.forward();
        let depth = |p: &ParticleInstance| (Vec3::from(p.position) - camera.position).dot(forward);
        self.sorted.sort_unstable_by(|a, b| depth(b).total_cmp(&depth(a)));

        self.count = self.sorted.len() as u32;
        if self.sorted.is_empty() {
            return 0;
        }
        if self.sorted.len() > self.capacity {
            self.capacity = self.sorted.len().next_power_of_two();
            self.buffer = particle_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.sorted));
        self.count
    }

    /// Group 0 (the globals) must already be bound.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        pass.draw(0..4, 0..self.count);
    }
}

/// A sphere around the particle's quad, stretched or turned any way.
fn bounds(p: &ParticleInstance) -> BoundingSphere {
    let radius = (p.size * 0.5 + Vec3::from(p.stretch).length()) * std::f32::consts::SQRT_2;
    BoundingSphere { center: p.position.into(), radius }
}

fn particle_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particles"),
        size: (capacity * size_of::<ParticleInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
