//! Draws the frame's particles after the main pass: camera-facing quads, farthest first,
//! over the scene. They read depth but don't write it, and can read it as a texture too.

use grimora_core::{BoundingSphere, Camera, Frustum, ParticleInstance, Vec3};

use crate::HDR_FORMAT;
use crate::renderer::depth_state;

const ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4,
    5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
];

pub(crate) struct ParticlePass {
    pipeline: wgpu::RenderPipeline,
    bend_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    /// What was drawn before the particles that bend it.
    copy: wgpu::Texture,
    buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
    bending: u32,
    /// What survived culling, farthest first, then what bends; kept to reuse its memory.
    sorted: Vec<ParticleInstance>,
}

impl ParticlePass {
    pub fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        globals_layout: &wgpu::BindGroupLayout,
        msaa: u32,
        targets: (&wgpu::TextureView, &wgpu::Texture),
    ) -> Self {
        let fragment_texture = |binding, sample_type, multisampled| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        };
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle scene"),
            entries: &[
                fragment_texture(3, wgpu::TextureSampleType::Depth, msaa > 1),
                fragment_texture(4, wgpu::TextureSampleType::Float { filterable: false }, false),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &[Some(globals_layout), Some(&group_layout)],
            immediate_size: 0,
        });
        let vertex = wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_particle"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<ParticleInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ATTRIBUTES,
            })],
        };
        let primitive = wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particles"),
            layout: Some(&layout),
            vertex: vertex.clone(),
            primitive,
            depth_stencil: Some(depth_state(false, crate::SCENE_DEPTH_COMPARE, Default::default())),
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
        // On the resolved image, so it reads scene depth itself instead of testing against it.
        let bend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particles that bend"),
            layout: Some(&layout),
            vertex,
            primitive,
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_particle_bend"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let capacity = 1024;
        let (copy, group) = scene_group(device, &group_layout, targets.0, targets.1);
        Self {
            pipeline,
            bend_pipeline,
            layout: group_layout,
            group,
            copy,
            buffer: particle_buffer(device, capacity),
            capacity,
            count: 0,
            bending: 0,
            sorted: Vec::new(),
        }
    }

    /// The texture that holds a copy of the scene, which the water reads as well.
    pub fn copy(&self) -> &wgpu::Texture {
        &self.copy
    }

    pub fn copy_view(&self) -> wgpu::TextureView {
        self.copy.create_view(&Default::default())
    }

    /// The screen's textures changed.
    pub fn resize(&mut self, device: &wgpu::Device, depth: &wgpu::TextureView, hdr: &wgpu::Texture) {
        (self.copy, self.group) = scene_group(device, &self.layout, depth, hdr);
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
        // Those that bend go last, as they show everything else through them.
        self.sorted.sort_unstable_by(|a, b| a.bends().cmp(&b.bends()).then(depth(b).total_cmp(&depth(a))));
        self.bending = self.sorted.iter().filter(|p| p.bends()).count() as u32;
        self.count = self.sorted.len() as u32 - self.bending;

        if self.sorted.is_empty() {
            return 0;
        }
        if self.sorted.len() > self.capacity {
            self.capacity = self.sorted.len().next_power_of_two();
            self.buffer = particle_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.sorted));
        self.count + self.bending
    }

    /// Group 0 (the globals) must already be bound. The depth must be attached read only.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(1, &self.group, &[]);
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        pass.draw(0..4, 0..self.count);
    }

    /// Draws what bends what is behind it onto `hdr`, the resolved image, if there is any.
    pub fn draw_bending(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        globals: &wgpu::BindGroup,
        hdr: &wgpu::Texture,
        hdr_view: &wgpu::TextureView,
    ) {
        if self.bending == 0 {
            return;
        }
        encoder.copy_texture_to_texture(hdr.as_image_copy(), self.copy.as_image_copy(), hdr.size());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("particles that bend"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: hdr_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.bend_pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, &self.group, &[]);
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        pass.draw(0..4, self.count..self.count + self.bending);
    }
}

fn scene_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    depth: &wgpu::TextureView,
    hdr: &wgpu::Texture,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let copy = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene copy"),
        size: hdr.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("particle scene"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(depth) },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&copy.create_view(&Default::default())),
            },
        ],
    });
    (copy, group)
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
