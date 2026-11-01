use anyhow::{Context as _, Result, anyhow};
use grimora_core::{Camera, Environment, Mat4, MeshData, PointLight, Transform, Vec3};
use image::RgbaImage;
use wgpu::util::DeviceExt as _;

use crate::bloom::Bloom;
use crate::material::{GpuMaterial, MaterialCache};
use crate::uniforms::{self, Globals, InstanceRaw, PostParams};
use crate::{DEPTH_FORMAT, HDR_FORMAT};

/// Startup options for the renderer.
#[derive(Debug, Clone)]
pub struct RendererSettings {
    pub vsync: bool,
    /// Requested MSAA sample count (1 or 4). Falls back to 1 when unsupported.
    pub msaa: u32,
    pub shadow_map_size: u32,
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self { vsync: true, msaa: 4, shadow_map_size: 2048 }
    }
}

/// Opaque reference to a mesh uploaded with [`Renderer::upload_mesh`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshHandle(u32);

/// One object to draw this frame.
#[derive(Debug, Clone, Copy)]
pub struct DrawItem {
    pub mesh: MeshHandle,
    pub transform: Transform,
    /// sRGB tint multiplied with the mesh's vertex colors.
    pub color: Vec3,
    /// Self-illumination: 0 = none, 1 = glows at its own albedo, higher = brighter.
    pub emissive: f32,
    /// Skinned meshes: index of this draw's first matrix in [`Frame::joint_matrices`].
    /// `None` draws the mesh in its bind pose.
    pub joints: Option<u32>,
}

/// Everything the renderer needs for one frame.
pub struct Frame<'a> {
    pub camera: &'a Camera,
    pub environment: &'a Environment,
    pub lights: &'a [PointLight],
    pub draws: &'a [DrawItem],
    /// Skinning matrices of every animated draw, one block per draw
    /// (see [`grimora_core::Animator::skinning_matrices`]).
    pub joint_matrices: &'a [Mat4],
    /// Seconds since start; drives flicker and film grain.
    pub time: f32,
}

/// Counters from the last rendered frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStats {
    pub draw_calls: u32,
    pub instances: u32,
    pub triangles: u64,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// Index range and index into `Renderer::materials`.
    submeshes: Vec<(std::ops::Range<u32>, usize)>,
    /// Some submesh uses alpha, so shadows are drawn per submesh.
    has_alpha: bool,
}

struct Targets {
    hdr_msaa: Option<wgpu::TextureView>,
    hdr: wgpu::TextureView,
    depth: wgpu::TextureView,
}

struct Batch {
    mesh: MeshHandle,
    instances: std::ops::Range<u32>,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    adapter_info: wgpu::AdapterInfo,
    msaa: u32,
    shadow_map_size: u32,

    globals_buffer: wgpu::Buffer,
    globals_layout: wgpu::BindGroupLayout,
    globals_group: wgpu::BindGroup,
    /// Skinning matrices for this frame (storage buffer in the globals group).
    joint_buffer: wgpu::Buffer,
    joint_capacity: usize,
    shadow_view: wgpu::TextureView,
    shadow_group: wgpu::BindGroup,
    post_buffer: wgpu::Buffer,
    post_layout: wgpu::BindGroupLayout,
    post_group: wgpu::BindGroup,
    linear_sampler: wgpu::Sampler,

    /// Indexed by [`mesh_pipeline_index`]: culling x alpha-to-coverage.
    mesh_pipelines: [wgpu::RenderPipeline; 4],
    sky_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    /// Shadow caster that cuts out alpha-tested texels.
    shadow_masked_pipeline: wgpu::RenderPipeline,
    tonemap_pipeline: wgpu::RenderPipeline,

    targets: Targets,
    bloom: Bloom,
    meshes: Vec<GpuMesh>,
    materials: Vec<GpuMaterial>,
    material_cache: MaterialCache,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instances: Vec<InstanceRaw>,
    batches: Vec<Batch>,
    stats: FrameStats,
}

impl Renderer {
    /// Creates a renderer drawing into `target` (usually an `Arc<winit::window::Window>`).
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        settings: &RendererSettings,
    ) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(target).context("creating window surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .context("no compatible GPU adapter found")?;
        let adapter_info = adapter.get_info();
        log::info!("GPU: {} ({:?}, {:?})", adapter_info.name, adapter_info.backend, adapter_info.device_type);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("grimora"),
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            })
            .await
            .context("creating GPU device")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(wgpu::TextureFormat::is_srgb).unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: if settings.vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync },
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let msaa_ok = |f: wgpu::TextureFormat| adapter.get_texture_format_features(f).flags.sample_count_supported(4);
        let msaa = if settings.msaa >= 4 && msaa_ok(HDR_FORMAT) && msaa_ok(DEPTH_FORMAT) { 4 } else { 1 };
        log::info!("Surface {format:?}, MSAA x{msaa}");

        // ---- bind group layouts
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
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
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow"),
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Depth),
                sampler_entry(1, wgpu::SamplerBindingType::Comparison),
            ],
        });
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Float { filterable: true }),
                sampler_entry(1, wgpu::SamplerBindingType::Filtering),
                texture_entry(2, wgpu::TextureSampleType::Float { filterable: true }),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // ---- resources
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let joint_capacity = 256;
        let joint_buffer = create_joint_buffer(&device, joint_capacity);
        let globals_group = create_globals_group(&device, &globals_layout, &globals_buffer, &joint_buffer);

        let shadow_map_size = settings.shadow_map_size.clamp(256, 8192);
        let shadow_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d { width: shadow_map_size, height: shadow_map_size, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow"),
            layout: &shadow_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&shadow_sampler) },
            ],
        });
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let post_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post params"),
            size: size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ---- pipelines
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/scene.wgsl").into()),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post.wgsl").into()),
        });
        let material_cache = MaterialCache::new(&device, &queue, msaa > 1);
        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&globals_layout), Some(&shadow_layout), Some(&material_cache.layout)],
            immediate_size: 0,
        });
        let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&globals_layout), Some(&shadow_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        // Group 1 (the shadow map itself) is left out: it is the render target here.
        let shadow_masked_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow masked"),
            bind_group_layouts: &[Some(&globals_layout), None, Some(&material_cache.layout)],
            immediate_size: 0,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });

        let mesh_buffers = [Some(uniforms::vertex_layout()), Some(InstanceRaw::layout())];
        let multisample = wgpu::MultisampleState { count: msaa, ..Default::default() };
        let hdr_target =
            [Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })];

        let mesh_pipeline_with = |label, cull_mode, alpha_to_coverage: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&scene_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &mesh_buffers,
                },
                primitive: wgpu::PrimitiveState { cull_mode, ..Default::default() },
                depth_stencil: Some(depth_state(true, wgpu::CompareFunction::Less, Default::default())),
                multisample: wgpu::MultisampleState {
                    alpha_to_coverage_enabled: alpha_to_coverage && msaa > 1,
                    ..multisample
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &hdr_target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_pipelines = [
            mesh_pipeline_with("mesh", Some(wgpu::Face::Back), false),
            mesh_pipeline_with("mesh double-sided", None, false),
            mesh_pipeline_with("mesh alpha", Some(wgpu::Face::Back), true),
            mesh_pipeline_with("mesh alpha double-sided", None, true),
        ];
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&sky_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_sky"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(false, wgpu::CompareFunction::Always, Default::default())),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_sky"),
                compilation_options: Default::default(),
                targets: &hdr_target,
            }),
            multiview_mask: None,
            cache: None,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &mesh_buffers,
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(
                true,
                wgpu::CompareFunction::LessEqual,
                wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            )),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let shadow_masked_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow masked"),
            layout: Some(&shadow_masked_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_shadow_masked"),
                compilation_options: Default::default(),
                buffers: &mesh_buffers,
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(
                true,
                wgpu::CompareFunction::LessEqual,
                wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            )),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_shadow_masked"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let tonemap_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tonemap"),
            layout: Some(&post_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &post_shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_tonemap"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let targets = create_targets(&device, config.width, config.height, msaa);
        let bloom = Bloom::new(&device, &targets.hdr, config.width, config.height);
        let post_group = create_post_group(&device, &post_layout, &targets.hdr, &linear_sampler, &bloom, &post_buffer);

        let instance_capacity = 1024;
        let instance_buffer = create_instance_buffer(&device, instance_capacity);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            adapter_info,
            msaa,
            shadow_map_size,
            globals_buffer,
            globals_layout,
            globals_group,
            joint_buffer,
            joint_capacity,
            shadow_view,
            shadow_group,
            post_buffer,
            post_layout,
            post_group,
            linear_sampler,
            mesh_pipelines,
            sky_pipeline,
            shadow_pipeline,
            shadow_masked_pipeline,
            tonemap_pipeline,
            targets,
            bloom,
            meshes: Vec::new(),
            materials: Vec::new(),
            material_cache,
            instance_buffer,
            instance_capacity,
            instances: Vec::new(),
            batches: Vec::new(),
            stats: FrameStats::default(),
        })
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == self.size() {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.targets = create_targets(&self.device, width, height, self.msaa);
        self.bloom.resize(&self.device, &self.targets.hdr, width, height);
        self.post_group = create_post_group(
            &self.device,
            &self.post_layout,
            &self.targets.hdr,
            &self.linear_sampler,
            &self.bloom,
            &self.post_buffer,
        );
    }

    pub fn upload_mesh(&mut self, mesh: &MeshData) -> MeshHandle {
        let vertices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let submeshes: Vec<_> = mesh
            .draw_ranges()
            .into_iter()
            .map(|sub| {
                let gpu = self.material_cache.create(&self.device, &self.queue, &mesh.material(sub.material));
                self.materials.push(gpu);
                (sub.indices, self.materials.len() - 1)
            })
            .collect();
        let has_alpha = submeshes.iter().any(|(_, m)| self.materials[*m].alpha);
        self.meshes.push(GpuMesh { vertices, indices, index_count: mesh.indices.len() as u32, submeshes, has_alpha });
        MeshHandle(self.meshes.len() as u32 - 1)
    }

    /// Frees every uploaded mesh. Existing [`MeshHandle`]s become invalid.
    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
        self.materials.clear();
        self.material_cache.clear();
    }

    /// Renders and presents a frame. With `capture`, also returns the final image
    /// exactly as shown on screen (useful for automated visual checks).
    pub fn render(&mut self, frame: &Frame<'_>, capture: bool) -> Result<Option<RgbaImage>> {
        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(None);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(None);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(anyhow!("surface validation error"));
            }
        };
        let surface_view = surface_texture.texture.create_view(&Default::default());

        let (globals, _) = uniforms::globals(
            frame.camera,
            self.aspect(),
            frame.environment,
            frame.lights,
            frame.time,
            self.shadow_map_size,
        );
        self.queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        let post = uniforms::post_params(frame.environment, frame.time, self.bloom.mip_count());
        self.queue.write_buffer(&self.post_buffer, 0, bytemuck::bytes_of(&post));
        self.prepare_instances(frame.draws);
        self.upload_joints(frame.joint_matrices);

        let mut encoder = self.device.create_command_encoder(&Default::default());

        if frame.environment.shadows.enabled {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            self.draw_batches(&mut pass, false);
        }

        {
            let (view, resolve_target) = match &self.targets.hdr_msaa {
                Some(msaa) => (msaa, Some(&self.targets.hdr)),
                None => (&self.targets.hdr, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: if resolve_target.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            self.draw_batches(&mut pass, true);
        }

        self.bloom.render(&mut encoder);
        self.tonemap(&mut encoder, &surface_view);
        self.queue.submit([encoder.finish()]);

        let image = if capture { Some(self.capture()?) } else { None };
        self.queue.present(surface_texture);
        Ok(image)
    }

    fn tonemap(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tonemap"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.tonemap_pipeline);
        pass.set_bind_group(0, &self.post_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Re-runs the tonemap pass into an offscreen texture and reads it back.
    fn capture(&self) -> Result<RgbaImage> {
        let (width, height) = self.size();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let padded_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.tonemap(&mut encoder, &texture.create_view(&Default::default()));
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).context("waiting for capture")?;
        rx.recv().context("capture callback dropped")?.context("mapping capture buffer")?;

        let data = slice.get_mapped_range().context("reading capture buffer")?;
        let bgra = matches!(self.config.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in data.chunks(padded_row as usize) {
            for px in row[..(width * 4) as usize].as_chunks::<4>().0 {
                if bgra {
                    pixels.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    pixels.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        drop(data);
        buffer.unmap();
        RgbaImage::from_raw(width, height, pixels).context("building capture image")
    }

    /// Sorts draws by mesh so each mesh is one instanced draw call.
    fn prepare_instances(&mut self, draws: &[DrawItem]) {
        let mut order: Vec<usize> =
            (0..draws.len()).filter(|&i| (draws[i].mesh.0 as usize) < self.meshes.len()).collect();
        order.sort_unstable_by_key(|&i| draws[i].mesh);

        self.instances.clear();
        self.batches.clear();
        for i in order {
            let draw = &draws[i];
            let index = self.instances.len() as u32;
            self.instances.push(InstanceRaw::new(&draw.transform, draw.color, draw.emissive, draw.joints));
            match self.batches.last_mut() {
                Some(batch) if batch.mesh == draw.mesh => batch.instances.end = index + 1,
                _ => self.batches.push(Batch { mesh: draw.mesh, instances: index..index + 1 }),
            }
        }

        if self.instances.len() > self.instance_capacity {
            self.instance_capacity = self.instances.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        self.queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.instances));

        self.stats = FrameStats {
            draw_calls: self.batches.len() as u32,
            instances: self.instances.len() as u32,
            triangles: self
                .batches
                .iter()
                .map(|b| {
                    u64::from(self.meshes[b.mesh.0 as usize].index_count / 3) * u64::from(b.instances.len() as u32)
                })
                .sum(),
        };
    }

    fn upload_joints(&mut self, matrices: &[Mat4]) {
        if matrices.len() > self.joint_capacity {
            self.joint_capacity = matrices.len().next_power_of_two();
            self.joint_buffer = create_joint_buffer(&self.device, self.joint_capacity);
            self.globals_group =
                create_globals_group(&self.device, &self.globals_layout, &self.globals_buffer, &self.joint_buffer);
        }
        if !matrices.is_empty() {
            self.queue.write_buffer(&self.joint_buffer, 0, bytemuck::cast_slice(matrices));
        }
    }

    /// Draws every batch. With `materials`, binds each submesh's material and
    /// picks culling and alpha-to-coverage; without (the shadow pass), draws
    /// whole meshes, splitting only those with alpha-tested submeshes.
    fn draw_batches(&self, pass: &mut wgpu::RenderPass<'_>, materials: bool) {
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut current: Option<*const wgpu::RenderPipeline> = None;
        let mut use_pipeline = |pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline| {
            if current != Some(pipeline) {
                current = Some(pipeline);
                pass.set_pipeline(pipeline);
            }
        };
        for batch in &self.batches {
            let mesh = &self.meshes[batch.mesh.0 as usize];
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            if !materials && !mesh.has_alpha {
                use_pipeline(pass, &self.shadow_pipeline);
                pass.draw_indexed(0..mesh.index_count, 0, batch.instances.clone());
                continue;
            }
            for (range, material) in &mesh.submeshes {
                let material = &self.materials[*material];
                if materials {
                    use_pipeline(pass, &self.mesh_pipelines[mesh_pipeline_index(material)]);
                } else if material.alpha {
                    use_pipeline(pass, &self.shadow_masked_pipeline);
                } else {
                    use_pipeline(pass, &self.shadow_pipeline);
                    pass.draw_indexed(range.clone(), 0, batch.instances.clone());
                    continue;
                }
                pass.set_bind_group(2, &material.bind_group, &[]);
                pass.draw_indexed(range.clone(), 0, batch.instances.clone());
            }
        }
    }
}

fn mesh_pipeline_index(material: &GpuMaterial) -> usize {
    usize::from(material.double_sided) | (usize::from(material.alpha) << 1)
}

pub(crate) fn texture_entry(binding: u32, sample_type: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub(crate) fn sampler_entry(binding: u32, ty: wgpu::SamplerBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(ty),
        count: None,
    }
}

fn depth_state(write: bool, compare: wgpu::CompareFunction, bias: wgpu::DepthBiasState) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias,
    }
}

fn create_targets(device: &wgpu::Device, width: u32, height: u32, msaa: u32) -> Targets {
    let texture = |label, format, samples, usage| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
    Targets {
        hdr_msaa: (msaa > 1).then(|| texture("hdr msaa", HDR_FORMAT, msaa, attachment)),
        hdr: texture("hdr", HDR_FORMAT, 1, attachment | wgpu::TextureUsages::TEXTURE_BINDING),
        depth: texture("depth", DEPTH_FORMAT, msaa, attachment),
    }
}

fn create_post_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    hdr: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    bloom: &Bloom,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("post"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(hdr) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(bloom.output()) },
            wgpu::BindGroupEntry { binding: 3, resource: params.as_entire_binding() },
        ],
    })
}

fn create_joint_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("joints"),
        size: (capacity * size_of::<Mat4>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_globals_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    joints: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: joints.as_entire_binding() },
        ],
    })
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size: (capacity * size_of::<InstanceRaw>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
