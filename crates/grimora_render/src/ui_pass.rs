//! Draws the frame's [`Ui`] over the tonemapped image, inside the tonemap pass.

use crate::font::FONT;
use crate::ui::{Ui, UiQuad};

const QUAD_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

pub(crate) struct UiPass {
    pipeline: wgpu::RenderPipeline,
    /// Target size, then atlas size (`UiGlobals` in `ui.wgsl`).
    globals: wgpu::Buffer,
    group: wgpu::BindGroup,
    quads: wgpu::Buffer,
    capacity: usize,
    count: u32,
}

impl UiPass {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font atlas"),
            size: wgpu::Extent3d { width: FONT.width, height: FONT.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            atlas.as_image_copy(),
            &FONT.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(FONT.width * 2),
                rows_per_image: Some(FONT.height),
            },
            atlas.size(),
        );

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let view = atlas.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ui.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_ui"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<UiQuad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &QUAD_ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ui"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let capacity = 256;
        Self { pipeline, globals, group, quads: quad_buffer(device, capacity), capacity, count: 0 }
    }

    /// Uploads this frame's quads for a target of `width` x `height` physical pixels.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, ui: &Ui, width: u32, height: u32) {
        let quads = ui.quads();
        self.count = quads.len() as u32;
        if quads.is_empty() {
            return;
        }
        if quads.len() > self.capacity {
            self.capacity = quads.len().next_power_of_two();
            self.quads = quad_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.quads, 0, bytemuck::cast_slice(quads));
        let sizes = [width as f32, height as f32, FONT.width as f32, FONT.height as f32];
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&sizes));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, self.quads.slice(..));
        pass.draw(0..4, 0..self.count);
    }
}

fn quad_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui quads"),
        size: (capacity * size_of::<UiQuad>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
