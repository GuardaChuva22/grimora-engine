//! Grimora's wgpu renderer.
//!
//! Frame pipeline: shadow map → HDR main pass (sky, lit meshes, then sorted
//! blended particles, MSAA) → bloom mip chain → tonemap/grade onto the window surface → HUD ([`Ui`]) on top.

mod bloom;
mod font;
mod font_data;
mod material;
mod noise;
mod particle_pass;
mod renderer;
mod ui;
mod ui_pass;
mod uniforms;

pub use image::RgbaImage;
pub use renderer::{DrawItem, Frame, FrameStats, MeshHandle, Portrait, Renderer, RendererSettings};
pub use ui::{ART_PIXEL, Anchor, ImageId, Rect, Slot, SlotResponse, TextStyle, Theme, Ui, UiInput, pixel_scale};
pub use uniforms::MAX_POINT_LIGHTS;
pub use wgpu;

pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The scene's depth is reversed: 1 at the near plane, 0 at the far one. Floats are densest
/// near 0, which then holds the distance, so terrain kilometers away keeps its order against
/// the sea and the depth read back under the water stays exact to the centimeter. The shadow
/// maps keep the usual order.
pub(crate) const SCENE_DEPTH_CLEAR: f32 = 0.0;
pub(crate) const SCENE_DEPTH_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::Greater;
