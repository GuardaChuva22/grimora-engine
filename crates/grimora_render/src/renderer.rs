use anyhow::{Context as _, Result, anyhow};
use std::sync::Arc;

use grimora_core::{
    BoundingSphere, Camera, Environment, Frustum, Mat4, MeshData, ParticleInstance, PointLight, SkyTextures,
    TextureData, Transform, Vec3, Vec4, WaterBed, WaterHull, WaterLevels,
    water::{HULL_MAP_SIZE, HullMap, MAX_HULLS},
};
use image::RgbaImage;
use wgpu::util::DeviceExt as _;

use crate::bloom::Bloom;
use crate::material::{self, GpuMaterial, MaterialCache};
use crate::noise::noise_texture;
use crate::particle_pass::ParticlePass;
use crate::ui::{ImageId, Ui};
use crate::ui_pass::UiPass;
use crate::uniforms::{self, Cascades, Globals, HullPlace, InstanceRaw, MAX_CASCADES, PostParams, SkySprites};
use crate::{DEPTH_FORMAT, HDR_FORMAT, SCENE_DEPTH_CLEAR, SCENE_DEPTH_COMPARE};

/// Startup options for the renderer.
#[derive(Debug, Clone)]
pub struct RendererSettings {
    pub vsync: bool,
    /// Requested MSAA sample count (1 or 4). Falls back to 1 when unsupported.
    pub msaa: u32,
    pub shadow_map_size: u32,
    /// Objects covering less than this fraction of the screen height are not
    /// drawn (0.002 is about 1.5 pixels at 720p). 0 draws everything in view.
    pub min_screen_size: f32,
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self { vsync: true, msaa: 4, shadow_map_size: 2048, min_screen_size: 0.002 }
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

/// A small picture of a few things, drawn before the frame into a picture the [`Ui`] shows:
/// with nothing behind them, and no shadows, water or particles.
pub struct Portrait<'a> {
    /// From [`Ui::reserve_image`], which says its size.
    pub image: ImageId,
    pub camera: Camera,
    pub environment: &'a Environment,
    pub draws: Vec<DrawItem>,
    pub joint_matrices: Vec<Mat4>,
}

/// Everything the renderer needs for one frame.
pub struct Frame<'a> {
    pub camera: &'a Camera,
    pub environment: &'a Environment,
    /// The pictures [`Environment::sky`] names.
    pub sky_textures: &'a SkyTextures,
    pub lights: &'a [PointLight],
    pub draws: &'a [DrawItem],
    /// Skinning matrices of every animated draw, one block per draw
    /// (see [`grimora_core::Animator::skinning_matrices`]).
    pub joint_matrices: &'a [Mat4],
    /// Drawn over the opaque scene, sorted by the renderer.
    pub particles: &'a [ParticleInstance],
    /// Seconds since start; drives flicker and film grain.
    pub time: f32,
    /// Drawn over the finished image.
    pub ui: &'a Ui,
    /// What lies under the sea, if the scene has one.
    pub water_bed: Option<&'a Arc<WaterBed>>,
    /// The level of every body of water around the camera (see `Waters::levels`).
    pub water_levels: Option<&'a Arc<WaterLevels>>,
    /// The hollow hulls that keep the water out.
    pub water_hulls: &'a [WaterHull],
    /// Pictures of things to draw first, for the [`Ui`].
    pub portraits: &'a [Portrait<'a>],
}

/// Counters from the last rendered frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStats {
    /// Main pass only.
    pub draw_calls: u32,
    /// Objects drawn in the main pass.
    pub instances: u32,
    /// Main pass only.
    pub triangles: u64,
    /// Objects skipped by the main pass: off screen or too small.
    pub culled: u32,
    /// Objects drawn into the shadow maps, counted once per cascade.
    pub shadow_casters: u32,
    /// Particles drawn; part of the main pass's draw calls and triangles.
    pub particles: u32,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// Index range and index into `Renderer::materials`.
    submeshes: Vec<(std::ops::Range<u32>, usize)>,
    /// Some submesh uses alpha, so shadows are drawn per submesh.
    has_alpha: bool,
    /// Some submesh is really blended, so it is also drawn among the transparent things.
    has_blend: bool,
    has_water: bool,
    /// See `MeshData::wind` and `MeshData::shadowless`.
    wind: f32,
    shadowless: bool,
    /// In model space; shared by the mesh's LODs so they switch consistently.
    bounds: BoundingSphere,
    /// `(screen_size, index into Renderer::meshes)`, most detailed first.
    lods: Vec<(f32, usize)>,
}

struct Targets {
    hdr_msaa: Option<wgpu::TextureView>,
    hdr: wgpu::TextureView,
    hdr_texture: wgpu::Texture,
    depth: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    /// The depth before the water was drawn, which the water reads while it writes the real one.
    depth_copy: wgpu::Texture,
    depth_copy_view: wgpu::TextureView,
}

/// The format of the [`Ui`]'s pictures, into which portraits are copied.
const PORTRAIT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Where the portraits of one size are drawn.
struct PortraitTarget {
    targets: Targets,
    /// Its colors tonemapped, with the alpha kept. Copied into the [`Ui`]'s pictures.
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

struct Batch {
    /// Index into `Renderer::meshes`.
    mesh: usize,
    instances: std::ops::Range<u32>,
}

/// Which submeshes a pass draws and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeshPass {
    Shadow,
    Opaque,
    Water,
    Transparent,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    adapter_info: wgpu::AdapterInfo,
    msaa: u32,
    shadow_map_size: u32,
    /// The most shadow cascades drawn, whatever the scene asks for.
    max_cascades: u32,
    min_screen_size: f32,

    globals_buffer: wgpu::Buffer,
    globals_layout: wgpu::BindGroupLayout,
    globals_group: wgpu::BindGroup,
    /// Skinning matrices for this frame (storage buffer in the globals group).
    joint_buffer: wgpu::Buffer,
    noise: (wgpu::TextureView, wgpu::Sampler),
    joint_capacity: usize,
    /// One depth layer per shadow cascade, as render targets.
    shadow_layers: Vec<wgpu::TextureView>,
    /// All layers, for sampling in the main pass.
    shadow_group: wgpu::BindGroup,
    shadow_layout: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    /// Each cascade's light matrix, `cascade_stride` bytes apart (dynamic offset).
    cascade_buffer: wgpu::Buffer,
    cascade_group: wgpu::BindGroup,
    cascade_stride: u64,
    post_buffer: wgpu::Buffer,
    post_layout: wgpu::BindGroupLayout,
    post_group: wgpu::BindGroup,
    linear_sampler: wgpu::Sampler,

    /// Indexed by [`mesh_pipeline_index`]: culling x alpha-to-coverage.
    mesh_pipelines: [wgpu::RenderPipeline; 4],
    /// Really blended surfaces: culling on and off.
    blend_pipelines: [wgpu::RenderPipeline; 2],
    water_pipeline: wgpu::RenderPipeline,
    water_layout: wgpu::BindGroupLayout,
    /// What is behind the water: the depth and the image drawn so far.
    water_group: wgpu::BindGroup,
    /// What lies under the sea, as a texture: depth below the level, in meters.
    bed: wgpu::TextureView,
    bed_rect: Vec4,
    bed_source: Option<Arc<WaterBed>>,
    /// `WaterLevels` as a texture, and where it lies (`Globals::level_map`).
    levels: wgpu::TextureView,
    level_map: Vec4,
    levels_source: Option<Arc<WaterLevels>>,
    /// The layers of `hull_maps` hold these. It stays one texel until a hull shows up.
    hull_maps: wgpu::TextureView,
    hull_texture: Option<wgpu::Texture>,
    hull_layers: [Option<Arc<HullMap>>; MAX_HULLS],
    sky_pipeline: wgpu::RenderPipeline,
    sky_layout: wgpu::BindGroupLayout,
    sky_sampler: wgpu::Sampler,
    sky_pixel_sampler: wgpu::Sampler,
    /// Filters pixelated materials and the sky's pictures instead of showing hard texels.
    smooth_textures: bool,
    sky_group: wgpu::BindGroup,
    /// The pictures `sky_group` shows, to notice when the scene changes them.
    sky_sources: SkyTextures,
    shadow_pipeline: wgpu::RenderPipeline,
    /// Shadow caster that cuts out alpha-tested texels.
    shadow_masked_pipeline: wgpu::RenderPipeline,
    tonemap_pipeline: wgpu::RenderPipeline,
    portrait_pipeline: wgpu::RenderPipeline,
    portrait_targets: std::collections::HashMap<(u32, u32), PortraitTarget>,
    particles: ParticlePass,
    ui: UiPass,

    targets: Targets,
    bloom: Bloom,
    meshes: Vec<GpuMesh>,
    materials: Vec<GpuMaterial>,
    /// Slots of `meshes` and `materials` that [`Renderer::replace_mesh`] let go of.
    free_meshes: Vec<usize>,
    free_materials: Vec<usize>,
    material_cache: MaterialCache,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instances: Vec<InstanceRaw>,
    /// `(mesh, draw)` pairs that survived culling, per pass; kept to reuse their memory.
    visible: Vec<(usize, usize)>,
    casters: [Vec<(usize, usize)>; MAX_CASCADES],
    /// `(mesh, draw)` pairs with blended surfaces, from the farthest to the nearest.
    see_through: Vec<(usize, usize)>,
    batches: Vec<Batch>,
    transparent_batches: Vec<Batch>,
    shadow_batches: [Vec<Batch>; MAX_CASCADES],
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
                // The water's vertices read these two as well.
                wgpu::BindGroupLayoutEntry {
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ..texture_entry(2, wgpu::TextureSampleType::Float { filterable: true })
                },
                wgpu::BindGroupLayoutEntry {
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ..sampler_entry(3, wgpu::SamplerBindingType::Filtering)
                },
                wgpu::BindGroupLayoutEntry {
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ..texture_entry(4, wgpu::TextureSampleType::Float { filterable: false })
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ..texture_entry(6, wgpu::TextureSampleType::Float { filterable: false })
                },
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler_entry(1, wgpu::SamplerBindingType::Comparison),
            ],
        });
        // Same group number as `shadow_layout`, which shadow passes don't use: the
        // shadow map is their render target.
        let cascade_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow cascade"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(size_of::<Mat4>() as u64),
                },
                count: None,
            }],
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
        let noise_view = material::upload(&device, &queue, &noise_texture(), false);
        let noise_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("noise"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let noise = (noise_view, noise_sampler);
        let bed = upload_bed(&device, &queue, None);
        let levels = upload_levels(&device, &queue, None);
        let (_, hull_maps) = create_hull_maps(&device, &queue, 1, 1);
        let globals_group = create_globals_group(
            &device,
            &globals_layout,
            &globals_buffer,
            &joint_buffer,
            &noise,
            &bed,
            &hull_maps,
            &levels,
        );

        let shadow_map_size = settings.shadow_map_size.clamp(256, 8192);
        let cascade_stride =
            (size_of::<Mat4>() as u64).next_multiple_of(u64::from(device.limits().min_uniform_buffer_offset_alignment));
        let cascade_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow cascades"),
            size: cascade_stride * MAX_CASCADES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cascade_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow cascade"),
            layout: &cascade_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &cascade_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<Mat4>() as u64),
                }),
            }],
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let (shadow_layers, shadow_group) =
            create_shadow_maps(&device, &shadow_layout, &shadow_sampler, shadow_map_size);
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
            source: wgpu::ShaderSource::Wgsl(if msaa > 1 {
                include_str!("shaders/scene.wgsl").into()
            } else {
                include_str!("shaders/scene.wgsl").replace("texture_depth_multisampled_2d", "texture_depth_2d").into()
            }),
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
        let sky_textures_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky"),
            entries: &[
                texture_entry(7, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(8, wgpu::TextureSampleType::Float { filterable: true }),
                sampler_entry(9, wgpu::SamplerBindingType::Filtering),
            ],
        });
        let sky_pixel_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky pixelated"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sky_sources = SkyTextures::default();
        let sky_group = create_sky_group(&device, &queue, &sky_textures_layout, &sky_sampler, &sky_sources);
        let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&globals_layout), Some(&shadow_layout), Some(&sky_textures_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&globals_layout), Some(&cascade_layout)],
            immediate_size: 0,
        });
        let shadow_masked_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow masked"),
            bind_group_layouts: &[Some(&globals_layout), Some(&cascade_layout), Some(&material_cache.layout)],
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
                depth_stencil: Some(depth_state(true, SCENE_DEPTH_COMPARE, Default::default())),
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
        // Over everything opaque, without hiding what comes after them.
        let blended_target = [Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let blended_pipeline_with = |label, vertex, fragment, cull_mode| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&scene_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &mesh_buffers,
                },
                primitive: wgpu::PrimitiveState { cull_mode, ..Default::default() },
                depth_stencil: Some(depth_state(false, SCENE_DEPTH_COMPARE, Default::default())),
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &blended_target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let blend_pipelines = [
            blended_pipeline_with("mesh blended", "vs_main", "fs_main", Some(wgpu::Face::Back)),
            blended_pipeline_with("mesh blended double-sided", "vs_main", "fs_main", None),
        ];
        let water_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("behind the water"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: msaa > 1,
                    },
                    count: None,
                },
                texture_entry(1, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let water_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&shadow_layout),
                Some(&material_cache.layout),
                Some(&water_layout),
            ],
            immediate_size: 0,
        });
        // Writes depth and covers what is behind it, which it already shows through itself.
        // Both faces: from under the water, the surface is seen from behind.
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water"),
            layout: Some(&water_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_water"),
                compilation_options: Default::default(),
                buffers: &mesh_buffers,
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(depth_state(true, SCENE_DEPTH_COMPARE, Default::default())),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_water"),
                compilation_options: Default::default(),
                targets: &hdr_target,
            }),
            multiview_mask: None,
            cache: None,
        });
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

        let portrait_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("portrait"),
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
                entry_point: Some("fs_portrait"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: PORTRAIT_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let targets = create_targets(&device, config.width, config.height, msaa);
        let particles =
            ParticlePass::new(&device, &scene_shader, &globals_layout, msaa, (&targets.depth, &targets.hdr_texture));
        let water_group = create_water_group(&device, &water_layout, &targets, particles.copy_view());
        let ui = UiPass::new(&device, &queue, format, &linear_sampler);

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
            max_cascades: MAX_CASCADES as u32,
            min_screen_size: settings.min_screen_size.max(0.0),
            globals_buffer,
            globals_layout,
            globals_group,
            joint_buffer,
            noise,
            joint_capacity,
            shadow_layers,
            shadow_group,
            shadow_layout,
            shadow_sampler,
            cascade_buffer,
            cascade_group,
            cascade_stride,
            post_buffer,
            post_layout,
            post_group,
            linear_sampler,
            mesh_pipelines,
            blend_pipelines,
            water_pipeline,
            water_layout,
            water_group,
            bed,
            bed_rect: Vec4::ZERO,
            bed_source: None,
            levels,
            level_map: Vec4::new(0.0, 0.0, 0.0, WaterLevels::NONE),
            levels_source: None,
            hull_maps,
            hull_texture: None,
            hull_layers: Default::default(),
            sky_pipeline,
            sky_layout: sky_textures_layout,
            sky_sampler,
            sky_pixel_sampler,
            smooth_textures: true,
            sky_group,
            sky_sources,
            shadow_pipeline,
            shadow_masked_pipeline,
            tonemap_pipeline,
            portrait_pipeline,
            portrait_targets: Default::default(),
            particles,
            ui,
            targets,
            bloom,
            meshes: Vec::new(),
            materials: Vec::new(),
            free_meshes: Vec::new(),
            free_materials: Vec::new(),
            material_cache,
            instance_buffer,
            instance_capacity,
            instances: Vec::new(),
            visible: Vec::new(),
            casters: Default::default(),
            see_through: Vec::new(),
            batches: Vec::new(),
            transparent_batches: Vec::new(),
            shadow_batches: Default::default(),
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
        self.particles.resize(&self.device, &self.targets.depth, &self.targets.hdr_texture);
        self.water_group =
            create_water_group(&self.device, &self.water_layout, &self.targets, self.particles.copy_view());
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

    /// Waits for the monitor or not. Does nothing if that is how it already is.
    pub fn set_vsync(&mut self, vsync: bool) {
        let mode = if vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync };
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Shadow map size in pixels and the most cascades to draw. Does nothing if
    /// that is how it already is; a new size rebuilds the maps.
    pub fn set_shadow_quality(&mut self, map_size: u32, max_cascades: u32) {
        self.max_cascades = max_cascades.clamp(1, MAX_CASCADES as u32);
        let map_size = map_size.clamp(256, 8192);
        if map_size != self.shadow_map_size {
            self.shadow_map_size = map_size;
            (self.shadow_layers, self.shadow_group) =
                create_shadow_maps(&self.device, &self.shadow_layout, &self.shadow_sampler, map_size);
        }
    }

    /// Bilinear filtering for pixel art textures, or hard texels. Takes effect at
    /// once, without uploading the meshes again.
    pub fn set_smooth_textures(&mut self, smooth: bool) {
        if smooth != self.smooth_textures {
            self.smooth_textures = smooth;
            self.rebuild_sky_group();
        }
    }

    fn rebuild_sky_group(&mut self) {
        let sampler = if self.smooth_textures { &self.sky_sampler } else { &self.sky_pixel_sampler };
        self.sky_group = create_sky_group(&self.device, &self.queue, &self.sky_layout, sampler, &self.sky_sources);
    }

    /// Uploads a mesh and its [`MeshData::lods`]; the handle draws whichever
    /// level suits the object's size on screen.
    pub fn upload_mesh(&mut self, mesh: &MeshData) -> MeshHandle {
        let mut bounds = mesh.bounding_sphere();
        if mesh.skeleton.is_some() {
            // Animated poses reach past the bind pose; the margin keeps limbs from being culled.
            bounds.radius *= 1.5;
        }
        if mesh.boundless {
            bounds.radius = f32::MAX * 0.5;
        }
        let lods = mesh
            .lods
            .iter()
            .map(|lod| (lod.screen_size, self.upload_level(&lod.mesh, mesh, bounds, Vec::new())))
            .collect();
        MeshHandle(self.upload_level(mesh, mesh, bounds, lods) as u32)
    }

    /// `whole` is the mesh this level belongs to, which says how it behaves.
    fn upload_level(
        &mut self,
        mesh: &MeshData,
        whole: &MeshData,
        bounds: BoundingSphere,
        lods: Vec<(f32, usize)>,
    ) -> usize {
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
                let slot = match self.free_materials.pop() {
                    Some(slot) => {
                        self.materials[slot] = gpu;
                        slot
                    }
                    None => {
                        self.materials.push(gpu);
                        self.materials.len() - 1
                    }
                };
                (sub.indices, slot)
            })
            .collect();
        let has_alpha = submeshes.iter().any(|(_, m)| self.materials[*m].alpha);
        let has_blend = submeshes.iter().any(|(_, m)| self.materials[*m].blend);
        let has_water = submeshes.iter().any(|(_, m)| self.materials[*m].water);
        let index_count = mesh.indices.len() as u32;
        let gpu = GpuMesh {
            vertices,
            indices,
            index_count,
            submeshes,
            has_alpha,
            has_blend,
            has_water,
            wind: whole.wind,
            shadowless: whole.shadowless,
            bounds,
            lods,
        };
        match self.free_meshes.pop() {
            Some(slot) => {
                self.meshes[slot] = gpu;
                slot
            }
            None => {
                self.meshes.push(gpu);
                self.meshes.len() - 1
            }
        }
    }

    /// Uploads `mesh` in place of the one behind `old`, which stops being valid: for meshes
    /// that come and go while the world runs, such as the ground's tiles, so memory doesn't grow.
    pub fn replace_mesh(&mut self, old: MeshHandle, mesh: &MeshData) -> MeshHandle {
        self.release(old.0 as usize);
        self.upload_mesh(mesh)
    }

    fn release(&mut self, index: usize) {
        let Some(gone) = self.meshes.get_mut(index) else {
            return;
        };
        // Its buffers go now; the slot holds empty ones until it is used again.
        gone.index_count = 0;
        gone.vertices = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("freed mesh"),
            size: 4,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        gone.indices = gone.vertices.clone();
        let submeshes = std::mem::take(&mut gone.submeshes);
        let lods = std::mem::take(&mut gone.lods);
        self.free_materials.extend(submeshes.into_iter().map(|(_, m)| m));
        self.free_meshes.push(index);
        for (_, lod) in lods {
            self.release(lod);
        }
    }

    /// Frees every uploaded mesh. Existing [`MeshHandle`]s become invalid.
    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
        self.materials.clear();
        self.free_meshes.clear();
        self.free_materials.clear();
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

        // Before the portraits, which draw into the pictures this may write.
        self.ui.prepare(&self.device, &self.queue, frame.ui, self.config.width, self.config.height);
        for portrait in frame.portraits {
            self.render_portrait(frame, portrait);
        }

        self.update_water_bed(frame.water_bed);
        self.update_water_levels(frame.water_levels);
        let hulls = self.update_water_hulls(frame.water_hulls, frame.camera.position);
        let (globals, cascades) = uniforms::globals(
            frame.camera,
            self.aspect(),
            frame.environment,
            frame.lights,
            frame.time,
            self.shadow_map_size,
            self.max_cascades,
            SkySprites {
                sun: frame.sky_textures.sun.is_some(),
                moon: frame.sky_textures.moon.is_some(),
                smooth: self.smooth_textures,
            },
            self.bed_rect,
            self.level_map,
            &hulls,
        );
        self.queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        self.update_sky_textures(frame.sky_textures);
        let post = uniforms::post_params(frame.environment, frame.time, self.bloom.mip_count());
        self.queue.write_buffer(&self.post_buffer, 0, bytemuck::bytes_of(&post));
        let cascades = frame.environment.shadows.enabled.then_some(cascades);
        for (i, view_proj) in cascades.iter().flat_map(|c| &c.view_proj[..c.count]).enumerate() {
            let offset = i as u64 * self.cascade_stride;
            self.queue.write_buffer(&self.cascade_buffer, offset, bytemuck::bytes_of(&view_proj.to_cols_array_2d()));
        }
        self.prepare_instances(frame, cascades.as_ref());
        let aspect = self.aspect();
        let particles = self.particles.prepare(&self.device, &self.queue, frame.particles, frame.camera, aspect);
        self.stats.particles = particles;
        self.stats.draw_calls += u32::from(particles > 0);
        self.stats.triangles += u64::from(particles) * 2;
        self.upload_joints(frame.joint_matrices);

        let mut encoder = self.device.create_command_encoder(&Default::default());

        for i in 0..cascades.map_or(0, |c| c.count) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_layers[i],
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.cascade_group, &[(i as u64 * self.cascade_stride) as u32]);
            self.draw_batches(&mut pass, &self.shadow_batches[i], MeshPass::Shadow);
        }

        let (color_view, resolve_target) = match &self.targets.hdr_msaa {
            Some(msaa) => (msaa, Some(&self.targets.hdr)),
            None => (&self.targets.hdr, None),
        };
        let water_shows = self.see_through.iter().any(|&(mesh, _)| self.meshes[mesh].has_water);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    // The water reads what was drawn before it.
                    resolve_target: resolve_target.filter(|_| water_shows),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(SCENE_DEPTH_CLEAR),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(2, &self.sky_group, &[]);
            pass.draw(0..3, 0..1);
            self.draw_batches(&mut pass, &self.batches, MeshPass::Opaque);
        }
        if water_shows {
            let hdr = &self.targets.hdr_texture;
            encoder.copy_texture_to_texture(hdr.as_image_copy(), self.particles.copy().as_image_copy(), hdr.size());
            let depth = &self.targets.depth_texture;
            encoder.copy_texture_to_texture(
                depth.as_image_copy(),
                self.targets.depth_copy.as_image_copy(),
                depth.size(),
            );
        }
        if !self.transparent_batches.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("water and what is see-through"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            pass.set_bind_group(3, &self.water_group, &[]);
            self.draw_batches(&mut pass, &self.transparent_batches, MeshPass::Water);
            self.draw_batches(&mut pass, &self.transparent_batches, MeshPass::Transparent);
        }
        {
            // A pass of their own, as they read the depth the main pass wrote. It also resolves.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particles"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: if resolve_target.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: None,
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            self.particles.draw(&mut pass);
        }
        self.particles.draw_bending(&mut encoder, &self.globals_group, &self.targets.hdr_texture, &self.targets.hdr);

        self.bloom.render(&mut encoder);
        self.tonemap(&mut encoder, &surface_view);
        self.queue.submit([encoder.finish()]);

        let image = if capture { Some(self.capture()?) } else { None };
        self.queue.present(surface_texture);
        Ok(image)
    }

    /// Draws one portrait, in a submission of its own, since it writes the same buffers as the
    /// frame that comes after it.
    fn render_portrait(&mut self, frame: &Frame<'_>, portrait: &Portrait<'_>) {
        let Some(rect) = frame.ui.image_rect(portrait.image) else { return };
        let size = (rect[2], rect[3]);
        if !self.portrait_targets.contains_key(&size) {
            let targets = create_targets(&self.device, size.0, size.1, self.msaa);
            let output = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("portrait"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: PORTRAIT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let output_view = output.create_view(&Default::default());
            let group = create_post_group(
                &self.device,
                &self.post_layout,
                &targets.hdr,
                &self.linear_sampler,
                &self.bloom,
                &self.post_buffer,
            );
            self.portrait_targets.insert(size, PortraitTarget { targets, output, output_view, group });
        }

        let view = Frame {
            camera: &portrait.camera,
            environment: portrait.environment,
            sky_textures: frame.sky_textures,
            lights: &[],
            draws: &portrait.draws,
            joint_matrices: &portrait.joint_matrices,
            particles: &[],
            time: frame.time,
            ui: frame.ui,
            water_bed: None,
            water_levels: None,
            water_hulls: &[],
            portraits: &[],
        };
        let (globals, _) = uniforms::globals(
            view.camera,
            size.0 as f32 / size.1.max(1) as f32,
            view.environment,
            view.lights,
            view.time,
            self.shadow_map_size,
            self.max_cascades,
            SkySprites::default(),
            self.bed_rect,
            self.level_map,
            &[],
        );
        self.queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        let post = uniforms::post_params(view.environment, view.time, self.bloom.mip_count());
        self.queue.write_buffer(&self.post_buffer, 0, bytemuck::bytes_of(&post));
        self.prepare_instances(&view, None);
        self.upload_joints(view.joint_matrices);

        let target = &self.portrait_targets[&size];
        let (color_view, resolve_target) = match &target.targets.hdr_msaa {
            Some(msaa) => (msaa, Some(&target.targets.hdr)),
            None => (&target.targets.hdr, None),
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("portrait"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: if resolve_target.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(SCENE_DEPTH_CLEAR),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            self.draw_batches(&mut pass, &self.batches, MeshPass::Opaque);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("portrait tonemap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.portrait_pipeline);
            pass.set_bind_group(0, &target.group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_texture(
            target.output.as_image_copy(),
            wgpu::TexelCopyTextureInfo {
                texture: self.ui.images_texture(),
                mip_level: 0,
                origin: wgpu::Origin3d { x: rect[0], y: rect[1], z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            target.output.size(),
        );
        self.queue.submit([encoder.finish()]);
    }

    fn update_water_bed(&mut self, bed: Option<&Arc<WaterBed>>) {
        let same = match (bed, &self.bed_source) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.bed_source = bed.cloned();
        self.bed = upload_bed(&self.device, &self.queue, bed.map(|b| &**b));
        self.bed_rect = bed.map_or(Vec4::ZERO, |b| {
            let size = b.max - b.min;
            Vec4::new(b.min.x, b.min.y, 1.0 / size.x, 1.0 / size.y)
        });
        self.rebuild_globals_group();
    }

    fn update_water_levels(&mut self, levels: Option<&Arc<WaterLevels>>) {
        let same = match (levels, &self.levels_source) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.levels_source = levels.cloned();
        self.levels = upload_levels(&self.device, &self.queue, levels.map(|l| &**l));
        // With no map, every point is off it, and only the sea's level (none: below everything) is left.
        self.level_map = levels.map_or(Vec4::new(0.0, 0.0, 0.0, WaterLevels::NONE), |l| {
            Vec4::new(l.min.x, l.min.y, 1.0 / l.cell, l.sea.unwrap_or(WaterLevels::NONE))
        });
        self.rebuild_globals_group();
    }

    /// Places the hulls nearest the camera, and gets each one's map to a layer of the texture.
    fn update_water_hulls(&mut self, hulls: &[WaterHull], camera: Vec3) -> Vec<HullPlace> {
        if hulls.is_empty() {
            return Vec::new();
        }
        if self.hull_texture.is_none() {
            let (texture, view) = create_hull_maps(&self.device, &self.queue, HULL_MAP_SIZE, MAX_HULLS as u32);
            self.hull_texture = Some(texture);
            self.hull_maps = view;
            self.rebuild_globals_group();
        }
        let mut nearest: Vec<&WaterHull> = hulls.iter().collect();
        nearest.sort_by(|a, b| a.position.distance_squared(camera).total_cmp(&b.position.distance_squared(camera)));
        nearest.truncate(MAX_HULLS);
        let shown = |map: &Arc<HullMap>| nearest.iter().any(|hull| Arc::ptr_eq(&hull.map, map));
        let mut places = Vec::with_capacity(nearest.len());
        for hull in &nearest {
            let held =
                self.hull_layers.iter().position(|map| map.as_ref().is_some_and(|map| Arc::ptr_eq(map, &hull.map)));
            let layer = held.unwrap_or_else(|| {
                let free = self.hull_layers.iter().position(|map| !map.as_ref().is_some_and(shown));
                let layer = free.unwrap_or(0);
                self.hull_layers[layer] = Some(hull.map.clone());
                write_hull_map(&self.queue, self.hull_texture.as_ref().unwrap(), layer as u32, &hull.map);
                layer
            });
            places.push(HullPlace::new(hull, layer));
        }
        places
    }

    fn rebuild_globals_group(&mut self) {
        self.globals_group = create_globals_group(
            &self.device,
            &self.globals_layout,
            &self.globals_buffer,
            &self.joint_buffer,
            &self.noise,
            &self.bed,
            &self.hull_maps,
            &self.levels,
        );
    }

    fn update_sky_textures(&mut self, textures: &SkyTextures) {
        let same = |a: &Option<Arc<TextureData>>, b: &Option<Arc<TextureData>>| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same(&self.sky_sources.sun, &textures.sun) || !same(&self.sky_sources.moon, &textures.moon) {
            self.sky_sources = textures.clone();
            self.rebuild_sky_group();
        }
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
        self.ui.draw(&mut pass);
    }

    /// Re-runs the tonemap pass (HUD included) into an offscreen texture and reads it back.
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

    /// Culls the draws against the camera and, with shadows on, against each
    /// shadow cascade's volume; drops objects too small to see (or, for a
    /// cascade, smaller than its texels); picks each one's LOD; then groups
    /// them by mesh so each mesh is one instanced draw call per pass.
    fn prepare_instances(&mut self, frame: &Frame<'_>, cascades: Option<&Cascades>) {
        let camera = frame.camera;
        let view = Frustum::from_view_projection(camera.view_projection(self.aspect()));
        let mut light = [None; MAX_CASCADES];
        if let Some(c) = cascades {
            for (slot, (view_proj, texel)) in light.iter_mut().zip(c.view_proj.iter().zip(c.texels)).take(c.count) {
                *slot = Some((Frustum::from_view_projection(*view_proj), texel));
            }
        }

        self.visible.clear();
        self.see_through.clear();
        for casters in &mut self.casters {
            casters.clear();
        }
        let mut culled = 0;
        for (i, draw) in frame.draws.iter().enumerate() {
            let Some(mesh) = self.meshes.get(draw.mesh.0 as usize) else { continue };
            let sphere = mesh.bounds.transformed(&draw.transform);
            let screen_size = sphere.screen_size(camera);
            if screen_size < self.min_screen_size {
                culled += 1;
                continue;
            }
            let level = pick_lod(&mesh.lods, screen_size).unwrap_or(draw.mesh.0 as usize);
            if view.intersects_sphere(sphere) {
                self.visible.push((level, i));
                if self.meshes[level].has_blend {
                    self.see_through.push((level, i));
                }
            } else {
                culled += 1;
            }
            if mesh.shadowless {
                continue;
            }
            for (casters, (frustum, texel)) in self.casters.iter_mut().zip(light.iter().map_while(|l| l.as_ref())) {
                let diameter = sphere.radius * 2.0;
                if diameter >= *texel && frustum.intersects_sphere(sphere) {
                    // The LOD that fits its size in this shadow map, as it would on screen.
                    let map_size = diameter / (texel * self.shadow_map_size as f32);
                    casters.push((pick_lod(&mesh.lods, map_size).unwrap_or(draw.mesh.0 as usize), i));
                }
            }
        }
        self.visible.sort_unstable();
        let distance = |&(_, i): &(usize, usize)| frame.draws[i].transform.position.distance_squared(camera.position);
        self.see_through.sort_by(|a, b| distance(b).total_cmp(&distance(a)));

        self.instances.clear();
        let sway = uniforms::Sway::of(frame.environment);
        group(&self.visible, frame.draws, &self.meshes, &sway, &mut self.instances, &mut self.batches);
        group(&self.see_through, frame.draws, &self.meshes, &sway, &mut self.instances, &mut self.transparent_batches);
        for (casters, batches) in self.casters.iter_mut().zip(&mut self.shadow_batches) {
            casters.sort_unstable();
            group(casters, frame.draws, &self.meshes, &sway, &mut self.instances, batches);
        }

        if self.instances.len() > self.instance_capacity {
            self.instance_capacity = self.instances.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        self.queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.instances));

        self.stats = FrameStats {
            draw_calls: (self.batches.len() + self.transparent_batches.len()) as u32,
            instances: self.visible.len() as u32,
            triangles: self
                .batches
                .iter()
                .map(|b| u64::from(self.meshes[b.mesh].index_count / 3) * u64::from(b.instances.len() as u32))
                .sum(),
            culled,
            shadow_casters: self.casters.iter().map(|c| c.len() as u32).sum(),
            particles: 0,
        };
    }

    fn upload_joints(&mut self, matrices: &[Mat4]) {
        if matrices.len() > self.joint_capacity {
            self.joint_capacity = matrices.len().next_power_of_two();
            self.joint_buffer = create_joint_buffer(&self.device, self.joint_capacity);
            self.rebuild_globals_group();
        }
        if !matrices.is_empty() {
            self.queue.write_buffer(&self.joint_buffer, 0, bytemuck::cast_slice(matrices));
        }
    }

    /// Draws every batch. In the main pass, binds each submesh's material and
    /// picks its pipeline, drawing the blended ones or the rest depending on
    /// `kind`. The shadow pass draws whole meshes, splitting only those with
    /// alpha-tested submeshes.
    fn draw_batches(&self, pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], kind: MeshPass) {
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut current: Option<*const wgpu::RenderPipeline> = None;
        let mut use_pipeline = |pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline| {
            if current != Some(pipeline) {
                current = Some(pipeline);
                pass.set_pipeline(pipeline);
            }
        };
        for batch in batches {
            let mesh = &self.meshes[batch.mesh];
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            if kind == MeshPass::Shadow && !mesh.has_alpha {
                use_pipeline(pass, &self.shadow_pipeline);
                pass.draw_indexed(0..mesh.index_count, 0, batch.instances.clone());
                continue;
            }
            for (range, material) in &mesh.submeshes {
                let material = &self.materials[*material];
                match kind {
                    MeshPass::Opaque if material.blend => continue,
                    MeshPass::Opaque => use_pipeline(pass, &self.mesh_pipelines[mesh_pipeline_index(material)]),
                    MeshPass::Water if !material.water => continue,
                    MeshPass::Water => use_pipeline(pass, &self.water_pipeline),
                    MeshPass::Transparent if !material.blend || material.water => continue,
                    MeshPass::Transparent => {
                        use_pipeline(pass, &self.blend_pipelines[usize::from(material.double_sided)]);
                    }
                    MeshPass::Shadow if material.alpha => use_pipeline(pass, &self.shadow_masked_pipeline),
                    MeshPass::Shadow => {
                        use_pipeline(pass, &self.shadow_pipeline);
                        pass.draw_indexed(range.clone(), 0, batch.instances.clone());
                        continue;
                    }
                }
                pass.set_bind_group(2, material.group(self.smooth_textures), &[]);
                pass.draw_indexed(range.clone(), 0, batch.instances.clone());
            }
        }
    }
}

/// The least detailed level whose `screen_size` threshold the object is under,
/// or `None` for full detail. `lods` goes from most to least detailed.
fn pick_lod(lods: &[(f32, usize)], screen_size: f32) -> Option<usize> {
    lods.iter().rev().find(|(threshold, _)| screen_size < *threshold).map(|&(_, mesh)| mesh)
}

/// Appends one instance per `(mesh, draw)` pair and one batch per run of the same mesh.
fn group(
    pairs: &[(usize, usize)],
    draws: &[DrawItem],
    meshes: &[GpuMesh],
    sway: &uniforms::Sway,
    instances: &mut Vec<InstanceRaw>,
    batches: &mut Vec<Batch>,
) {
    batches.clear();
    for &(mesh, i) in pairs {
        let draw = &draws[i];
        let index = instances.len() as u32;
        let wind = meshes[mesh].wind;
        let phase = if wind > 0.0 { sway.phase(draw.transform.position) } else { 0.0 };
        instances.push(InstanceRaw::new(&draw.transform, draw.color, draw.emissive, draw.joints, wind, phase));
        match batches.last_mut() {
            Some(batch) if batch.mesh == mesh => batch.instances.end = index + 1,
            _ => batches.push(Batch { mesh, instances: index..index + 1 }),
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

pub(crate) fn depth_state(
    write: bool,
    compare: wgpu::CompareFunction,
    bias: wgpu::DepthBiasState,
) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias,
    }
}

/// The shadow map: one render target per cascade, and all of them to sample from.
fn create_shadow_maps(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: u32,
) -> (Vec<wgpu::TextureView>, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow cascades"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: MAX_CASCADES as u32 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layers = (0..MAX_CASCADES as u32)
        .map(|layer| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    (layers, group)
}

/// The sun and moon pictures for the sky; a missing one is a transparent pixel.
fn create_sky_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    textures: &SkyTextures,
) -> wgpu::BindGroup {
    let view = |texture: &Option<Arc<TextureData>>| match texture {
        Some(texture) => material::upload(device, queue, texture, true),
        None => {
            let empty = TextureData { name: "no sky picture".into(), width: 1, height: 1, rgba: vec![0; 4] };
            material::upload(device, queue, &empty, true)
        }
    };
    let (sun, moon) = (view(&textures.sun), view(&textures.moon));
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sky"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&sun) },
            wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&moon) },
            wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn create_targets(device: &wgpu::Device, width: u32, height: u32, msaa: u32) -> Targets {
    let texture = |label, format, samples, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
    let sampled = attachment | wgpu::TextureUsages::TEXTURE_BINDING;
    let hdr_texture = texture("hdr", HDR_FORMAT, 1, sampled | wgpu::TextureUsages::COPY_SRC);
    let depth_texture = texture("depth", DEPTH_FORMAT, msaa, sampled | wgpu::TextureUsages::COPY_SRC);
    let depth_copy = texture("depth before water", DEPTH_FORMAT, msaa, sampled | wgpu::TextureUsages::COPY_DST);
    Targets {
        hdr_msaa: (msaa > 1)
            .then(|| texture("hdr msaa", HDR_FORMAT, msaa, attachment).create_view(&Default::default())),
        hdr: hdr_texture.create_view(&Default::default()),
        hdr_texture,
        depth: depth_texture.create_view(&Default::default()),
        depth_texture,
        depth_copy_view: depth_copy.create_view(&Default::default()),
        depth_copy,
    }
}

/// The depth and the image from before the water, which are its group 3.
fn create_water_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    targets: &Targets,
    scene: wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("behind the water"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&targets.depth_copy_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&scene) },
        ],
    })
}

/// The map of what lies under the sea; a single deep point without one.
fn upload_levels(device: &wgpu::Device, queue: &wgpu::Queue, levels: Option<&WaterLevels>) -> wgpu::TextureView {
    let none = [[WaterLevels::NONE, 1.0, 0.0, 0.0]];
    let (side, cells) = levels.map_or((1, &none[..]), |l| (l.size as u32, &l.cells[..]));
    let size = wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("water levels"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(cells),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side * 16), rows_per_image: Some(side) },
        size,
    );
    texture.create_view(&Default::default())
}

fn upload_bed(device: &wgpu::Device, queue: &wgpu::Queue, bed: Option<&WaterBed>) -> wgpu::TextureView {
    let none = [1000.0f32];
    let (width, height, depths) = bed.map_or((1, 1, &none[..]), |b| (b.width, b.height, &b.depths[..]));
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("water bed"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(depths),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
        size,
    );
    texture.create_view(&Default::default())
}

/// An array of maps with nothing on them.
fn create_hull_maps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    side: u32,
    layers: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let size = wgpu::Extent3d { width: side, height: side, depth_or_array_layers: layers };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hull maps"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let off = vec![HullMap::OFF; (side * side * layers) as usize];
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(&off),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side * 4), rows_per_image: Some(side) },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    (texture, view)
}

fn write_hull_map(queue: &wgpu::Queue, texture: &wgpu::Texture, layer: u32, map: &HullMap) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&map.low),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(HULL_MAP_SIZE * 4),
            rows_per_image: Some(HULL_MAP_SIZE),
        },
        wgpu::Extent3d { width: HULL_MAP_SIZE, height: HULL_MAP_SIZE, depth_or_array_layers: 1 },
    );
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

#[allow(clippy::too_many_arguments)]
fn create_globals_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    joints: &wgpu::Buffer,
    noise: &(wgpu::TextureView, wgpu::Sampler),
    bed: &wgpu::TextureView,
    hull_maps: &wgpu::TextureView,
    levels: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: joints.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&noise.0) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&noise.1) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(bed) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(hull_maps) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(levels) },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_coarsest_level_that_fits() {
        let lods = [(0.1, 7), (0.02, 8)];
        assert_eq!(pick_lod(&lods, 0.5), None);
        assert_eq!(pick_lod(&lods, 0.05), Some(7));
        assert_eq!(pick_lod(&lods, 0.01), Some(8));
        assert_eq!(pick_lod(&[], 0.01), None);
    }
}
