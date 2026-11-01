//! Grimora's wgpu renderer.
//!
//! Frame pipeline: shadow map → HDR main pass (sky + lit meshes, MSAA) →
//! bloom mip chain → tonemap/grade onto the window surface.

mod bloom;
mod material;
mod renderer;
mod uniforms;

pub use image::RgbaImage;
pub use renderer::{DrawItem, Frame, FrameStats, MeshHandle, Renderer, RendererSettings};
pub use uniforms::MAX_POINT_LIGHTS;
pub use wgpu;

pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
