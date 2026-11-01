//! Grimora's wgpu renderer.
//!
//! Frame pipeline: shadow map → HDR main pass (sky, lit meshes, then sorted
//! blended particles, MSAA) → bloom mip chain → tonemap/grade onto the window surface → HUD ([`Ui`]) on top.

mod bloom;
mod font;
mod material;
mod particle_pass;
mod renderer;
mod ui;
mod ui_pass;
mod uniforms;

pub use image::RgbaImage;
pub use renderer::{DrawItem, Frame, FrameStats, MeshHandle, Renderer, RendererSettings};
pub use ui::{Anchor, Rect, TextStyle, Ui};
pub use uniforms::MAX_POINT_LIGHTS;
pub use wgpu;

pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
