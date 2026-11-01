//! Screen-space 2D drawing for HUDs and menus: text, rectangles, panels and
//! bars. Immediate mode: the game calls [`Ui`] methods every frame (the
//! engine clears it before `Game::update`) and the renderer draws the result
//! over the finished image, in call order.
//!
//! Coordinates are logical pixels from the top-left corner of the window
//! ([`Ui::size`]); on high-DPI screens they are scaled by [`Ui::scale`].
//! Colors are sRGB 0..1 with alpha.

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec4};
use grimora_core::environment::srgb_to_linear_vec;

use crate::font::{FONT, GLYPH, Glyph, LINE};

/// A point of a box (or of the screen): corners, edge midpoints or centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// Where the point is, as a fraction of the box: `(0, 0)` top-left to `(1, 1)` bottom-right.
    pub fn fraction(self) -> Vec2 {
        let (x, y) = match self {
            Anchor::TopLeft => (0.0, 0.0),
            Anchor::Top => (0.5, 0.0),
            Anchor::TopRight => (1.0, 0.0),
            Anchor::Left => (0.0, 0.5),
            Anchor::Center => (0.5, 0.5),
            Anchor::Right => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 1.0),
            Anchor::Bottom => (0.5, 1.0),
            Anchor::BottomRight => (1.0, 1.0),
        };
        Vec2::new(x, y)
    }
}

/// An axis-aligned box in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub min: Vec2,
    pub size: Vec2,
}

impl Rect {
    pub fn new(min: Vec2, size: Vec2) -> Self {
        Self { min, size }
    }

    /// The box of `size` whose `anchor` point is at `at`.
    pub fn anchored(at: Vec2, size: Vec2, anchor: Anchor) -> Self {
        Self { min: at - size * anchor.fraction(), size }
    }

    pub fn max(&self) -> Vec2 {
        self.min + self.size
    }

    /// The point of the box at `anchor`.
    pub fn point(&self, anchor: Anchor) -> Vec2 {
        self.min + self.size * anchor.fraction()
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.cmpge(self.min).all() && p.cmplt(self.max()).all()
    }

    /// Grown by `margin` on every side (shrunk if negative).
    pub fn expand(&self, margin: f32) -> Self {
        Self { min: self.min - margin, size: (self.size + 2.0 * margin).max(Vec2::ZERO) }
    }
}

/// How [`Ui::text`] draws a string.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Glyph height in logical pixels. Multiples of 8 are the crispest.
    pub size: f32,
    pub color: Vec4,
    /// One-pixel outline (in font pixels) around each glyph, which keeps text
    /// readable over any scene. Alpha 0 turns it off.
    pub outline: Vec4,
    /// Which point of the text's box goes at the given position. Lines are
    /// aligned the same way: left, centred or right.
    pub anchor: Anchor,
    /// Wraps lines at spaces to stay within this width.
    pub max_width: Option<f32>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 16.0,
            color: Vec4::new(0.95, 0.92, 0.86, 1.0),
            outline: Vec4::new(0.0, 0.0, 0.0, 0.85),
            anchor: Anchor::TopLeft,
            max_width: None,
        }
    }
}

impl TextStyle {
    /// The default style at `size`.
    pub fn new(size: f32) -> Self {
        Self { size, ..Default::default() }
    }

    pub fn color(self, color: Vec4) -> Self {
        Self { color, ..self }
    }

    pub fn outline(self, outline: Vec4) -> Self {
        Self { outline, ..self }
    }

    pub fn anchor(self, anchor: Anchor) -> Self {
        Self { anchor, ..self }
    }

    pub fn wrap(self, max_width: f32) -> Self {
        Self { max_width: Some(max_width), ..self }
    }

    /// Logical pixels per font pixel.
    fn unit(&self) -> f32 {
        self.size.max(1.0) / GLYPH as f32
    }
}

/// One textured rectangle for the GPU, in physical pixels. Mirrors `Quad` in `ui.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct UiQuad {
    /// x, y, width, height.
    pub rect: [f32; 4],
    /// Atlas texels: x, y, width, height.
    pub uv: [f32; 4],
    /// Linear RGB and alpha.
    pub color: [f32; 4],
    pub outline: [f32; 4],
}

/// This frame's HUD. See the module docs.
#[derive(Debug, Clone)]
pub struct Ui {
    size: Vec2,
    scale: f32,
    quads: Vec<UiQuad>,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new(Vec2::new(1280.0, 720.0), 1.0)
    }
}

impl Ui {
    /// An empty UI for a window of `size` logical pixels at `scale` physical pixels each.
    pub fn new(size: Vec2, scale: f32) -> Self {
        Self { size, scale: scale.max(0.01), quads: Vec::new() }
    }

    /// Clears last frame's drawing and adopts the current window size. The
    /// engine calls this before `Game::update`.
    pub fn begin(&mut self, size: Vec2, scale: f32) {
        self.size = size;
        self.scale = scale.max(0.01);
        self.quads.clear();
    }

    /// Window size in logical pixels.
    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// Physical pixels per logical pixel (the window's DPI scale).
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn screen(&self) -> Rect {
        Rect::new(Vec2::ZERO, self.size)
    }

    /// The point of the screen at `anchor`, moved by `offset`.
    pub fn at(&self, anchor: Anchor, offset: Vec2) -> Vec2 {
        self.screen().point(anchor) + offset
    }

    /// Whether nothing was drawn this frame.
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    pub(crate) fn quads(&self) -> &[UiQuad] {
        &self.quads
    }

    /// A filled rectangle.
    pub fn rect(&mut self, rect: Rect, color: Vec4) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 || color.w <= 0.0 {
            return;
        }
        let [sx, sy] = FONT.solid;
        let (min, size) = (rect.min * self.scale, rect.size * self.scale);
        self.quads.push(UiQuad {
            rect: [min.x, min.y, size.x, size.y],
            uv: [sx, sy, 0.0, 0.0],
            color: linear(color),
            outline: [0.0; 4],
        });
    }

    /// A filled rectangle with a one-pixel border inside its edge.
    pub fn panel(&mut self, rect: Rect, fill: Vec4, border: Vec4) {
        let line = 1.0_f32.max(1.0 / self.scale);
        self.rect(rect.expand(-line), fill);
        let (min, max, size) = (rect.min, rect.max(), rect.size);
        self.rect(Rect::new(min, Vec2::new(size.x, line)), border);
        self.rect(Rect::new(Vec2::new(min.x, max.y - line), Vec2::new(size.x, line)), border);
        self.rect(Rect::new(Vec2::new(min.x, min.y + line), Vec2::new(line, size.y - 2.0 * line)), border);
        self.rect(Rect::new(Vec2::new(max.x - line, min.y + line), Vec2::new(line, size.y - 2.0 * line)), border);
    }

    /// A meter: `back` over the whole rectangle, then `fill` from the left
    /// for `fraction` (0..1) of it, inset by a pixel.
    pub fn bar(&mut self, rect: Rect, fraction: f32, fill: Vec4, back: Vec4) {
        self.rect(rect, back);
        let inner = rect.expand(-1.0);
        let width = inner.size.x * fraction.clamp(0.0, 1.0);
        self.rect(Rect::new(inner.min, Vec2::new(width, inner.size.y)), fill);
    }

    /// Size of `text` drawn with `style` (lines split at `\n` and wrapped
    /// at `style.max_width`).
    pub fn measure(&self, text: &str, style: &TextStyle) -> Vec2 {
        let lines = layout(text, style);
        block_size(&lines, style)
    }

    /// Draws `text` with its `style.anchor` point at `at` and returns the box
    /// it fills. Characters the font lacks show as `?`.
    pub fn text(&mut self, at: Vec2, text: &str, style: &TextStyle) -> Rect {
        let lines = layout(text, style);
        let bounds = Rect::anchored(at, block_size(&lines, style), style.anchor);
        let unit = style.unit();
        let (color, outline) = (linear(style.color), linear(style.outline));
        let scale = self.scale;
        let snap = |v: f32| (v * scale).round() / scale;
        for (n, (line, width)) in lines.iter().enumerate() {
            let mut pen = snap(bounds.min.x + (bounds.size.x - width) * style.anchor.fraction().x);
            let top = snap(bounds.min.y + n as f32 * LINE as f32 * unit);
            for c in line.chars() {
                let glyph = FONT.glyph(c);
                if glyph.width > 0 {
                    self.glyph(glyph, Vec2::new(pen, top), unit, color, outline);
                }
                pen += glyph.advance() as f32 * unit;
            }
        }
        bounds
    }

    fn glyph(&mut self, glyph: Glyph, at: Vec2, unit: f32, color: [f32; 4], outline: [f32; 4]) {
        // One font pixel of margin on every side holds the outline.
        let cols = glyph.width + 2;
        let rows = GLYPH + 2;
        let min = (at - unit) * self.scale;
        let size = Vec2::new(cols as f32, rows as f32) * unit * self.scale;
        self.quads.push(UiQuad {
            rect: [min.x, min.y, size.x, size.y],
            uv: [(glyph.x + glyph.left - 1) as f32, (glyph.y - 1) as f32, cols as f32, rows as f32],
            color,
            outline,
        });
    }
}

fn linear(color: Vec4) -> [f32; 4] {
    srgb_to_linear_vec(color.truncate()).extend(color.w.clamp(0.0, 1.0)).to_array()
}

/// Width of one line, without the spacing after its last glyph.
fn line_width(line: &str, unit: f32) -> f32 {
    let units: u32 = line.chars().map(|c| FONT.glyph(c).advance()).sum();
    units.saturating_sub(1) as f32 * unit
}

/// Splits `text` into lines (at `\n`, and at spaces to fit `max_width`), each
/// with its width.
fn layout<'t>(text: &'t str, style: &TextStyle) -> Vec<(&'t str, f32)> {
    let unit = style.unit();
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let Some(max_width) = style.max_width else {
            lines.push((paragraph, line_width(paragraph, unit)));
            continue;
        };
        let mut start = 0;
        let mut end = 0;
        let mut offset = 0;
        for word in paragraph.split(' ') {
            let word_end = offset + word.len();
            if end > start && line_width(&paragraph[start..word_end], unit) > max_width {
                lines.push((&paragraph[start..end], line_width(&paragraph[start..end], unit)));
                start = offset;
            }
            end = word_end;
            offset = word_end + 1;
        }
        lines.push((&paragraph[start..end], line_width(&paragraph[start..end], unit)));
    }
    lines
}

fn block_size(lines: &[(&str, f32)], style: &TextStyle) -> Vec2 {
    let unit = style.unit();
    let width = lines.iter().map(|(_, w)| *w).fold(0.0, f32::max);
    let height = ((lines.len() - 1) as u32 * LINE + GLYPH) as f32 * unit;
    Vec2::new(width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> TextStyle {
        TextStyle::new(16.0)
    }

    #[test]
    fn text_is_measured_in_font_pixels() {
        let ui = Ui::default();
        let one = ui.measure("I", &style());
        assert_eq!(one.y, 16.0, "one line is the glyph height");
        assert_eq!(one.x, FONT.glyph('I').width as f32 * 2.0, "no trailing spacing");
        let two = ui.measure("I\nI", &style());
        assert_eq!(two, Vec2::new(one.x, (LINE + GLYPH) as f32 * 2.0));
        assert!(ui.measure("mmm", &style()).x > ui.measure("iii", &style()).x, "proportional");
        assert_eq!(ui.measure("", &style()), Vec2::new(0.0, 16.0));
    }

    #[test]
    fn anchors_place_the_box() {
        let mut ui = Ui::new(Vec2::new(800.0, 600.0), 1.0);
        let style = style().anchor(Anchor::BottomRight);
        let corner = ui.at(Anchor::BottomRight, Vec2::new(-10.0, -10.0));
        assert_eq!(corner, Vec2::new(790.0, 590.0));
        let bounds = ui.text(corner, "Olá, mundo", &style);
        assert_eq!(bounds.max(), corner);
        assert_eq!(bounds.size, ui.measure("Olá, mundo", &style));

        let centered = ui.text(Vec2::new(400.0, 300.0), "centro", &TextStyle::new(8.0).anchor(Anchor::Center));
        assert_eq!(centered.point(Anchor::Center), Vec2::new(400.0, 300.0));
    }

    #[test]
    fn long_text_wraps_at_spaces() {
        let ui = Ui::default();
        let text = "a lanterna treme na névoa enquanto o andarilho segue";
        let wrapped = style().wrap(200.0);
        let size = ui.measure(text, &wrapped);
        assert!(size.x <= 200.0, "{size}");
        assert!(size.y > 16.0 * 3.0, "several lines: {size}");
        let lines = layout(text, &wrapped);
        assert_eq!(lines.iter().map(|(l, _)| *l).collect::<Vec<_>>().join(" "), text, "no words lost");
        // A word longer than the width gets a line of its own instead of vanishing.
        let narrow = layout("um extraordinariamente longo", &style().wrap(40.0));
        assert_eq!(narrow.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["um", "extraordinariamente", "longo"]);
    }

    #[test]
    fn glyphs_become_quads_in_physical_pixels() {
        let mut ui = Ui::new(Vec2::new(640.0, 360.0), 2.0);
        ui.text(Vec2::new(10.0, 20.0), "a b", &style());
        assert_eq!(ui.quads().len(), 2, "spaces draw nothing");
        let a = ui.quads()[0];
        let unit = 2.0 * 2.0; // 16 px text is 2 logical px per font pixel, at scale 2
        assert_eq!(a.rect[0], 10.0 * 2.0 - unit, "one font pixel of outline margin");
        assert_eq!(a.rect[1], 20.0 * 2.0 - unit);
        assert_eq!(a.rect[3], (GLYPH + 2) as f32 * unit);
        assert_eq!(a.uv[2] * unit, a.rect[2], "atlas texels map to whole font pixels");

        ui.begin(Vec2::new(640.0, 360.0), 1.0);
        assert!(ui.is_empty(), "begin clears the frame");
    }

    #[test]
    fn rects_bars_and_panels() {
        let mut ui = Ui::default();
        ui.rect(Rect::new(Vec2::ZERO, Vec2::new(10.0, 0.0)), Vec4::ONE);
        ui.rect(Rect::new(Vec2::ZERO, Vec2::splat(10.0)), Vec4::ZERO);
        assert!(ui.is_empty(), "empty or invisible rectangles are skipped");

        ui.bar(Rect::new(Vec2::ZERO, Vec2::new(102.0, 10.0)), 0.25, Vec4::ONE, Vec4::new(0.0, 0.0, 0.0, 0.5));
        assert_eq!(ui.quads().len(), 2);
        assert_eq!(ui.quads()[1].rect[2], 25.0, "a quarter of the inner width");
        ui.bar(Rect::new(Vec2::ZERO, Vec2::new(102.0, 10.0)), 7.0, Vec4::ONE, Vec4::ONE);
        assert_eq!(ui.quads()[3].rect[2], 100.0, "clamped to full");

        let before = ui.quads().len();
        ui.panel(Rect::new(Vec2::ZERO, Vec2::splat(50.0)), Vec4::new(0.1, 0.1, 0.1, 0.8), Vec4::ONE);
        assert_eq!(ui.quads().len() - before, 5, "fill and four border lines");
        assert_eq!(ui.quads()[before].color[3], 0.8);
        let white = linear(Vec4::ONE);
        assert_eq!(white, [1.0; 4]);
    }
}
