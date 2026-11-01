//! Physically-based-ish bloom: a chain of half-resolution mips, filled by
//! successive downsamples of the HDR image and then blurred back up additively.

use crate::HDR_FORMAT;

const MAX_MIPS: u32 = 6;

pub struct Bloom {
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    mips: Vec<wgpu::TextureView>,
    down_groups: Vec<wgpu::BindGroup>,
    up_groups: Vec<wgpu::BindGroup>,
}

impl Bloom {
    pub fn new(device: &wgpu::Device, hdr: &wgpu::TextureView, width: u32, height: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bloom.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
            entries: &[
                crate::renderer::texture_entry(0, wgpu::TextureSampleType::Float { filterable: true }),
                crate::renderer::sampler_entry(1, wgpu::SamplerBindingType::Filtering),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let mut bloom = Self {
            downsample: pipeline("fs_downsample", None),
            upsample: pipeline("fs_upsample", Some(additive)),
            layout,
            sampler,
            mips: Vec::new(),
            down_groups: Vec::new(),
            up_groups: Vec::new(),
        };
        bloom.resize(device, hdr, width, height);
        bloom
    }

    /// Recreates the mip chain for a new HDR target size.
    pub fn resize(&mut self, device: &wgpu::Device, hdr: &wgpu::TextureView, width: u32, height: u32) {
        let w = (width / 2).max(1);
        let h = (height / 2).max(1);
        let mip_count = (w.min(h).max(1).ilog2().saturating_sub(2)).clamp(1, MAX_MIPS);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bloom"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.mips = (0..mip_count)
            .map(|mip| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("bloom mip"),
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();

        let group = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        };
        // down_groups[i] reads the source of mip i; up_groups[i] reads mip i + 1.
        self.down_groups = (0..self.mips.len()).map(|i| group(if i == 0 { hdr } else { &self.mips[i - 1] })).collect();
        self.up_groups = self.mips.iter().skip(1).map(group).collect();
    }

    pub fn mip_count(&self) -> u32 {
        self.mips.len() as u32
    }

    /// The final blurred image (mip 0), sampled by the tonemap pass.
    pub fn output(&self) -> &wgpu::TextureView {
        &self.mips[0]
    }

    pub fn render(&self, encoder: &mut wgpu::CommandEncoder) {
        for (target, group) in self.mips.iter().zip(&self.down_groups) {
            fullscreen_pass(
                encoder,
                "bloom down",
                target,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                &self.downsample,
                group,
            );
        }
        for (i, group) in self.up_groups.iter().enumerate().rev() {
            fullscreen_pass(encoder, "bloom up", &self.mips[i], wgpu::LoadOp::Load, &self.upsample, group);
        }
    }
}

fn fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}
