//! GPU materials: textures (with mipmaps) and their bind groups.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use grimora_core::{AlphaMode, Material, Shading, TextureData};
use wgpu::util::DeviceExt as _;

use crate::renderer::{sampler_entry, texture_entry};

/// Mirrors `MaterialParams` in scene.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct MaterialParams {
    base_color: [f32; 4],
    /// metallic, roughness, normal scale, occlusion strength
    params: [f32; 4],
    /// rgb = emitted light, w = alpha cutoff
    emissive: [f32; 4],
    /// x = has normal map, y = alpha mode (0 opaque, 1 mask, 2 blend), z = alpha output
    /// (to coverage or to blending), w = shading (1 foliage, 2 crystal, 3 ground)
    flags: [u32; 4],
    /// Water only: rgb shallow color, w clarity in meters.
    water_shallow: [f32; 4],
    /// rgb foam color, w how much foam.
    water_foam: [f32; 4],
    /// x murk, y refraction, z 1 with a picture of foam, w meters one repeat covers.
    water_look: [f32; 4],
    /// xy how fast the picture moves along its uv, per second; w 1 for a river.
    water_flow: [f32; 4],
    /// x where it starts to dither away (flat meters from the camera), y where it is gone; 0 = never.
    fade: [f32; 4],
}

pub struct GpuMaterial {
    /// Filters the textures smoothly (bilinear).
    bind_group: wgpu::BindGroup,
    /// The same textures with hard texels, for materials marked `pixelated`.
    pixel_bind_group: Option<wgpu::BindGroup>,
    pub double_sided: bool,
    /// Uses alpha (mask or blend): cut out of shadows, and masks get alpha-to-coverage.
    pub alpha: bool,
    /// Really blended: drawn in the transparent part of the main pass.
    pub blend: bool,
    pub water: bool,
    /// An aura shell (`Shading::Aura`): blended in its own pipeline.
    pub aura: bool,
}

impl GpuMaterial {
    /// `smooth` is the player's choice: it only changes materials marked
    /// `pixelated`, because the others were never meant to show hard texels.
    pub fn group(&self, smooth: bool) -> &wgpu::BindGroup {
        match &self.pixel_bind_group {
            Some(pixels) if !smooth => pixels,
            _ => &self.bind_group,
        }
    }
}

/// Uploads materials, sharing textures that several materials reference.
pub struct MaterialCache {
    pub layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pixel_sampler: wgpu::Sampler,
    white_srgb: wgpu::TextureView,
    white_linear: wgpu::TextureView,
    flat_normal: wgpu::TextureView,
    alpha_to_coverage: bool,
    /// Keyed by the texture's address and color space. The `Arc` is kept so the
    /// address can't be reused by another texture while cached.
    textures: HashMap<(usize, bool), (Arc<TextureData>, wgpu::TextureView)>,
}

impl MaterialCache {
    /// `alpha_to_coverage` tells the shader whether alpha pipelines have it (MSAA only).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, alpha_to_coverage: bool) -> Self {
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // Aura shells read their look in the vertex shader too.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1, float),
                texture_entry(2, float),
                texture_entry(3, float),
                texture_entry(4, float),
                sampler_entry(5, wgpu::SamplerBindingType::Filtering),
                texture_entry(6, float),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        // Sharp texels up close; far away, mipmaps still keep them from shimmering.
        let pixel_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material pixelated"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let pixel = |name: &str, rgba: [u8; 4], srgb| {
            upload(device, queue, &TextureData { name: name.into(), width: 1, height: 1, rgba: rgba.into() }, srgb)
        };
        Self {
            layout,
            sampler,
            pixel_sampler,
            white_srgb: pixel("white srgb", [255; 4], true),
            white_linear: pixel("white linear", [255; 4], false),
            flat_normal: pixel("flat normal", [128, 128, 255, 255], false),
            alpha_to_coverage,
            textures: HashMap::new(),
        }
    }

    pub fn create(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, material: &Material) -> GpuMaterial {
        let (alpha_mode, cutoff) = match material.alpha_mode {
            AlphaMode::Opaque => (0, 0.0),
            AlphaMode::Mask { cutoff } => (1, cutoff.clamp(0.0, 1.0)),
            AlphaMode::Blend => (2, 0.0),
        };
        let look = material.water.unwrap_or_default();
        let params = MaterialParams {
            base_color: material.base_color.to_array(),
            params: [
                material.metallic.clamp(0.0, 1.0),
                material.roughness.clamp(0.0, 1.0),
                material.normal_scale,
                if material.occlusion_texture.is_some() { material.occlusion_strength } else { 0.0 },
            ],
            emissive: material.emissive.max(glam::Vec3::ZERO).extend(cutoff).to_array(),
            flags: [
                // The ground keeps other pictures in the normal and metal slots.
                u32::from(material.normal_texture.is_some() && material.shading != Shading::Ground),
                alpha_mode,
                u32::from(self.alpha_to_coverage || alpha_mode == 2),
                match material.shading {
                    Shading::Foliage => 1,
                    Shading::Crystal => 2,
                    Shading::Ground => 3,
                    Shading::Aura => 4,
                    Shading::Standard | Shading::Water => 0,
                },
            ],
            water_shallow: look.shallow_linear().extend(look.clarity.max(0.01)).to_array(),
            water_foam: look.foam_linear().extend(look.foam.max(0.0)).to_array(),
            water_look: [
                look.murk.clamp(0.0, 1.0),
                look.refraction.max(0.0),
                f32::from(u8::from(material.emissive_texture.is_some())),
                material.water_tile.max(0.01),
            ],
            water_flow: [material.water_flow.x, material.water_flow.y, 0.0, material.water_river],
            fade: [material.fade.x.max(0.0), material.fade.y.max(0.0), 0.0, 0.0],
        };
        // An aura keeps its look in the water's slots, which it has no use for: see
        // `fs_aura` in scene.wgsl.
        let mut params = params;
        if let (Shading::Aura, Some(a)) = (material.shading, material.aura) {
            let linear = |c: glam::Vec3| {
                c.max(glam::Vec3::ZERO)
                    .to_array()
                    .map(|v| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) })
            };
            let [hr, hg, hb] = linear(a.heart);
            let [rr, rg, rb] = linear(a.rim);
            params.base_color = [hr, hg, hb, a.glow.max(0.0)];
            params.emissive = [rr, rg, rb, 0.0];
            params.water_shallow = [a.thickness, 0.0, a.scale.max(0.01), a.speed];
            params.water_foam = [a.outline.max(0.0), a.edge.max(0.1), a.cover.clamp(0.0, 1.0), a.bands.max(0.0)];
            let pattern = a.pattern as u8 as f32;
            params.water_look = [a.pixels.max(0.0), a.warp.max(0.0), pattern, a.body.max(0.0)];
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let base = self.view(device, queue, &material.base_color_texture, true);
        let normal = self.view(device, queue, &material.normal_texture, false);
        let mr = self.view(device, queue, &material.metallic_roughness_texture, false);
        let occlusion = self.view(device, queue, &material.occlusion_texture, false);
        let emissive = self.view(device, queue, &material.emissive_texture, true);
        let views = [
            base.unwrap_or_else(|| self.white_srgb.clone()),
            normal.unwrap_or_else(|| self.flat_normal.clone()),
            mr.unwrap_or_else(|| self.white_linear.clone()),
            occlusion.unwrap_or_else(|| self.white_linear.clone()),
            emissive.unwrap_or_else(|| self.white_srgb.clone()),
        ];
        let group = |sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: material.name.as_deref(),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[2]) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&views[3]) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&views[4]) },
                ],
            })
        };
        GpuMaterial {
            bind_group: group(&self.sampler),
            pixel_bind_group: material.pixelated.then(|| group(&self.pixel_sampler)),
            double_sided: material.double_sided,
            alpha: alpha_mode != 0,
            blend: alpha_mode == 2,
            water: material.shading == Shading::Water,
            aura: material.shading == Shading::Aura,
        }
    }

    fn view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &Option<Arc<TextureData>>,
        srgb: bool,
    ) -> Option<wgpu::TextureView> {
        let texture = texture.as_ref()?;
        let key = (Arc::as_ptr(texture) as usize, srgb);
        if let Some((_, view)) = self.textures.get(&key) {
            return Some(view.clone());
        }
        if texture.rgba.len() != texture.width as usize * texture.height as usize * 4 || texture.width == 0 {
            log::warn!("texture {:?} has the wrong size, ignoring it", texture.name);
            return None;
        }
        let view = upload(device, queue, texture, srgb);
        self.textures.insert(key, (texture.clone(), view.clone()));
        Some(view)
    }

    /// Drops cached textures (call when every mesh is cleared).
    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

/// Uploads an image with a full CPU-generated mip chain.
pub(crate) fn upload(device: &wgpu::Device, queue: &wgpu::Queue, image: &TextureData, srgb: bool) -> wgpu::TextureView {
    let (width, height) = (image.width, image.height);
    let mip_count = width.max(height).max(1).ilog2() + 1;
    let format = if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&image.name),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: mip_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let mut level = image.clone();
    for mip in 0..mip_count {
        if mip > 0 {
            level = level.half();
        }
        level.fill_under_alpha();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(level.width * 4),
                rows_per_image: Some(level.height),
            },
            wgpu::Extent3d { width: level.width, height: level.height, depth_or_array_layers: 1 },
        );
    }
    texture.create_view(&Default::default())
}
