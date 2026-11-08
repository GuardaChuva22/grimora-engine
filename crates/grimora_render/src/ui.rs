//! Screen-space 2D drawing for HUDs and menus: text, rectangles, panels and
//! bars. Immediate mode: the game calls [`Ui`] methods every frame (the
//! engine clears it before `Game::update`) and the renderer draws the result
//! over the finished image, in call order.
//!
//! Coordinates are logical pixels from the top-left corner of the window
//! ([`Ui::size`]); on high-DPI screens they are scaled by [`Ui::scale`].
//! Colors are sRGB 0..1 with alpha.
//!
//! Menus use the widgets ([`Ui::button`], [`Ui::slider`], [`Ui::toggle`],
//! [`Ui::choice`]), which answer to [`UiInput`]. One widget holds the focus:
//! arrows or W/S move it, the mouse takes it by moving over a widget. A widget
//! is known by its label, so two on screen shouldn't share one.

use std::ops::RangeInclusive;

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec4};
use grimora_core::environment::srgb_to_linear_vec;
use grimora_core::{Input, KeyCode, MouseButton, PadButton};

use crate::font::{FONT, GLYPH, Glyph, LINE, ROWS, TOP, UNIT_SIZE};

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
    /// Text size in logical pixels: 8 is one logical pixel per font pixel, and multiples
    /// of 8 are the crispest.
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
        self.size.max(1.0) / UNIT_SIZE
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

/// Logical pixels in one pixel of the art: dither patterns are drawn on this grid, and a
/// window scaled by [`pixel_scale`] makes it a whole number of real pixels.
pub const ART_PIXEL: f32 = 2.0;
/// The side of an item's picture, in logical pixels at one pixel of art each.
const ICON_ART: f32 = 48.0;

/// The UI scale that keeps the art's pixels whole on a window this many physical pixels tall:
/// the art is drawn for a screen 360 pixels tall, and 720 logical pixels are that screen.
pub fn pixel_scale(height: f32) -> f32 {
    ((height / 360.0).floor() * 0.5).max(1.0)
}

/// Texels on each side of the texture that holds the pictures of [`Ui::add_image`].
pub(crate) const IMAGE_ATLAS: u32 = 1024;

/// A picture that [`Ui::add_image`] took in, to show with [`Ui::image`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(u32);

/// Every picture the UI has, side by side in one texture.
#[derive(Debug, Clone, Default)]
pub(crate) struct ImageAtlas {
    /// RGBA, once there is a picture.
    pub pixels: Vec<u8>,
    /// Goes up with each picture added, so the renderer knows to send them again.
    pub version: u64,
    /// Texels of each picture: x, y, width, height.
    rects: Vec<[u32; 4]>,
    /// Where the next one goes, and the bottom of the row it's in.
    pen: (u32, u32),
    row_bottom: u32,
}

/// What a [`Ui::slot`] shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Slot<'a> {
    pub image: Option<ImageId>,
    /// Shown in place of the picture, if there is none.
    pub short: &'a str,
    /// Shown in its corner when above 1.
    pub count: u32,
    /// Drawn with a bright frame: the one in use, or being moved.
    pub marked: bool,
}

/// What happened to a [`Ui::slot`] this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotResponse {
    /// Clicked, or Enter or Space with the focus.
    pub used: bool,
    pub focused: bool,
    pub hovered: bool,
}

/// What the widgets see of the mouse and keyboard this frame. The engine fills
/// it from [`Input`]; a test can fill it by hand.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiInput {
    /// Cursor in logical pixels; `None` while it is grabbed for mouse look.
    pub cursor: Option<Vec2>,
    pub mouse_down: bool,
    pub mouse_pressed: bool,
    pub mouse_released: bool,
    /// Move the focus (arrows or W/S, repeating while held).
    pub up: bool,
    pub down: bool,
    /// Change the focused slider, toggle or choice (arrows or A/D, repeating while held).
    pub left: bool,
    pub right: bool,
    /// Use the focused widget (Enter or Space).
    pub activate: bool,
}

impl UiInput {
    /// Reads `input`, whose cursor is in physical pixels, for a UI at `scale`.
    pub fn from_input(input: &Input, scale: f32) -> Self {
        let tapped = |arrow, letter, pad| {
            [arrow, letter].into_iter().any(|key| input.just_pressed(key) || input.just_repeated(key))
                || input.pad_nav(pad)
        };
        Self {
            cursor: Some(input.cursor() / scale.max(0.01)),
            mouse_down: input.mouse_pressed(MouseButton::Left),
            mouse_pressed: input.mouse_just_pressed(MouseButton::Left),
            mouse_released: input.mouse_just_released(MouseButton::Left),
            up: tapped(KeyCode::ArrowUp, KeyCode::KeyW, PadButton::DPadUp),
            down: tapped(KeyCode::ArrowDown, KeyCode::KeyS, PadButton::DPadDown),
            left: tapped(KeyCode::ArrowLeft, KeyCode::KeyA, PadButton::DPadLeft),
            right: tapped(KeyCode::ArrowRight, KeyCode::KeyD, PadButton::DPadRight),
            activate: [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]
                .into_iter()
                .any(|key| input.just_pressed(key))
                || input.pad_just_pressed(PadButton::South),
        }
    }
}

/// How widgets look; the defaults match the sandbox's HUD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Height of widget text in logical pixels.
    pub text_size: f32,
    pub text: Vec4,
    /// Text and border of the focused widget, and the filled part of sliders and toggles.
    pub accent: Vec4,
    pub disabled: Vec4,
    pub fill: Vec4,
    /// Background under the cursor or with the focus.
    pub hover: Vec4,
    /// Background while the mouse holds the widget down.
    pub pressed: Vec4,
    pub border: Vec4,
    /// Background of slider tracks and toggle boxes.
    pub track: Vec4,
    /// Widgets have a box around them. Without it they're lines of text on the screen, and
    /// the one with the focus has a band that fades to the right and a pointer.
    pub frames: bool,
    /// Widgets write in capitals. What identifies them is still the label as given.
    pub upper: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            text_size: 24.0,
            text: Vec4::new(0.9, 0.87, 0.76, 1.0),
            accent: Vec4::new(0.98, 0.95, 0.84, 1.0),
            disabled: Vec4::new(0.6, 0.6, 0.58, 0.6),
            fill: Vec4::new(0.015, 0.018, 0.05, 0.86),
            hover: Vec4::new(0.12, 0.13, 0.22, 0.92),
            pressed: Vec4::new(0.22, 0.22, 0.3, 0.96),
            border: Vec4::new(0.62, 0.46, 0.58, 1.0),
            track: Vec4::new(0.0, 0.0, 0.03, 0.6),
            frames: true,
            upper: false,
        }
    }
}

/// What the widgets remember between frames.
#[derive(Debug, Clone, Default)]
struct WidgetState {
    input: UiInput,
    focus: Option<u64>,
    /// False when `focus` names no usable widget: the next enabled one drawn takes it.
    focus_settled: bool,
    /// `focus` was just set by the game, for a screen that isn't drawn yet.
    focus_requested: bool,
    /// The widget the mouse went down on, until it comes up.
    held: Option<u64>,
    /// `held` is a slider grabbed by its track, so it follows the cursor.
    dragging: bool,
    cursor_moved: bool,
    last_cursor: Option<Vec2>,
    /// Id and enabled state of every widget drawn, this frame and the last. The
    /// keyboard moves the focus through last frame's, since this frame's aren't known yet.
    drawn: Vec<(u64, bool)>,
    drawn_before: Vec<(u64, bool)>,
    /// Those drawn are a grid with this many columns, if not 0. Like `drawn`, this frame's and the last.
    columns: usize,
    columns_before: usize,
}

/// How a widget stands this frame.
#[derive(Debug, Clone, Copy)]
struct Response {
    enabled: bool,
    hovered: bool,
    focused: bool,
    /// The mouse holds it down, over it.
    pressed: bool,
    /// The mouse went down on it this frame.
    grabbed: bool,
    /// The mouse went down on it and came up over it, this frame.
    clicked: bool,
    /// Enter or Space with the focus.
    keyed: bool,
}

/// This frame's HUD. See the module docs.
#[derive(Debug, Clone)]
pub struct Ui {
    size: Vec2,
    scale: f32,
    quads: Vec<UiQuad>,
    /// How widgets are drawn; change it any time.
    pub theme: Theme,
    widgets: WidgetState,
    images: ImageAtlas,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new(Vec2::new(1280.0, 720.0), 1.0)
    }
}

impl Ui {
    /// An empty UI for a window of `size` logical pixels at `scale` physical pixels each.
    pub fn new(size: Vec2, scale: f32) -> Self {
        Self {
            size,
            scale: scale.max(0.01),
            quads: Vec::new(),
            theme: Theme::default(),
            widgets: WidgetState::default(),
            images: ImageAtlas::default(),
        }
    }

    /// Clears last frame's drawing and adopts the current window size. The
    /// engine calls this before `Game::update`, then [`Ui::set_input`].
    pub fn begin(&mut self, size: Vec2, scale: f32) {
        self.size = size;
        self.scale = scale.max(0.01);
        self.quads.clear();
        let widgets = &mut self.widgets;
        std::mem::swap(&mut widgets.drawn, &mut widgets.drawn_before);
        widgets.drawn.clear();
        widgets.columns_before = std::mem::take(&mut widgets.columns);
        widgets.input = UiInput::default();
    }

    /// Drops what was drawn this frame, and nothing else.
    pub fn clear(&mut self) {
        self.quads.clear();
    }

    /// Gives the widgets this frame's input and moves the focus for it.
    /// What was given to [Ui::set_input] this frame.
    pub fn input(&self) -> UiInput {
        self.widgets.input
    }

    pub fn set_input(&mut self, input: UiInput) {
        let widgets = &mut self.widgets;
        widgets.cursor_moved = matches!((widgets.last_cursor, input.cursor), (Some(a), Some(b)) if a != b);
        widgets.last_cursor = input.cursor;
        if !input.mouse_down && !input.mouse_released {
            widgets.held = None;
            widgets.dragging = false;
        }
        widgets.input = input;
        if std::mem::take(&mut widgets.focus_requested) {
            widgets.focus_settled = true;
            return;
        }
        let enabled = || widgets.drawn_before.iter().filter(|(_, enabled)| *enabled).map(|(id, _)| *id);
        let at = widgets.focus.and_then(|focus| enabled().position(|id| id == focus));
        widgets.focus_settled = at.is_some();
        if let Some(at) = at {
            let count = enabled().count();
            let next = match widgets.columns_before {
                0 => (at + usize::from(input.down) + (count - 1) * usize::from(input.up)) % count,
                columns => {
                    let across = i32::from(input.right) - i32::from(input.left);
                    let wanted = at as i32 + across + (i32::from(input.down) - i32::from(input.up)) * columns as i32;
                    usize::try_from(wanted).ok().filter(|&wanted| wanted < count).unwrap_or(at)
                }
            };
            widgets.focus = enabled().nth(next);
        }
    }

    /// Says that the widgets of this frame are a grid with this many columns, for the
    /// keys to move through it the next frame: in all four directions, stopping at its ends.
    pub fn grid(&mut self, columns: usize) {
        self.widgets.columns = columns;
    }

    /// Whether the widget with this label has the focus.
    pub fn is_focused(&self, label: &str) -> bool {
        self.widgets.focus == Some(widget_id(label))
    }

    /// Gives the focus to the widget with this label, in this frame or the next
    /// one (the screen you are about to switch to).
    pub fn set_focus(&mut self, label: &str) {
        self.widgets.focus = Some(widget_id(label));
        self.widgets.focus_settled = true;
        self.widgets.focus_requested = true;
    }

    /// Forgets the focus: the first enabled widget drawn next takes it. Call
    /// when switching to another screen.
    pub fn reset_focus(&mut self) {
        self.widgets.focus = None;
        self.widgets.focus_settled = false;
        self.widgets.focus_requested = false;
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

    pub(crate) fn images(&self) -> &ImageAtlas {
        &self.images
    }

    /// Takes in a picture of `width` x `height` texels (RGBA, sRGB) to show with
    /// [`Ui::image`]. `None` if there's no more room for pictures.
    pub fn add_image(&mut self, width: u32, height: u32, rgba: &[u8]) -> Option<ImageId> {
        // A clear texel between them, so that one doesn't bleed into the next.
        const GAP: u32 = 1;
        let atlas = &mut self.images;
        if rgba.len() != (width * height * 4) as usize {
            return None;
        }
        if atlas.pen.0 + width + GAP > IMAGE_ATLAS {
            atlas.pen = (0, atlas.row_bottom);
        }
        let (x, y) = atlas.pen;
        if x + width + GAP > IMAGE_ATLAS || y + height + GAP > IMAGE_ATLAS {
            return None;
        }
        atlas.pixels.resize((IMAGE_ATLAS * IMAGE_ATLAS * 4) as usize, 0);
        for (row, line) in rgba.chunks_exact((width * 4) as usize).enumerate() {
            let start = ((y + GAP + row as u32) * IMAGE_ATLAS + x + GAP) as usize * 4;
            atlas.pixels[start..start + line.len()].copy_from_slice(line);
        }
        atlas.rects.push([x + GAP, y + GAP, width, height]);
        atlas.pen.0 += width + GAP;
        atlas.row_bottom = atlas.row_bottom.max(y + height + GAP);
        atlas.version += 1;
        Some(ImageId(atlas.rects.len() as u32 - 1))
    }

    /// Room for a picture that something else draws, such as a
    /// [`Portrait`](crate::Portrait). It's clear until then.
    pub fn reserve_image(&mut self, width: u32, height: u32) -> Option<ImageId> {
        self.add_image(width, height, &vec![0; (width * height * 4) as usize])
    }

    /// Where a picture is among the pictures: x, y, width and height.
    pub(crate) fn image_rect(&self, image: ImageId) -> Option<[u32; 4]> {
        self.images.rects.get(image.0 as usize).copied()
    }

    /// A picture from [`Ui::add_image`] over `rect`, its colors multiplied by `tint`. Its
    /// texels stay sharp, whatever the size.
    pub fn image(&mut self, rect: Rect, image: ImageId, tint: Vec4) {
        let Some(&[x, y, width, height]) = self.images.rects.get(image.0 as usize) else { return };
        let (min, size) = (rect.min * self.scale, rect.size * self.scale);
        self.quads.push(UiQuad {
            rect: [min.x, min.y, size.x, size.y],
            uv: [x as f32, y as f32, width as f32, height as f32],
            color: linear(tint),
            outline: [0.0, 0.0, 0.0, -1.0],
        });
    }

    /// A filled rectangle.
    pub fn rect(&mut self, rect: Rect, color: Vec4) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 || color.w <= 0.0 {
            return;
        }
        let [sx, sy] = FONT.solid;
        // On whole pixels, or two lines of the same width come out different.
        let min = (rect.min * self.scale).round();
        let size = ((rect.max() * self.scale).round() - min).max(Vec2::ONE);
        self.quads.push(UiQuad {
            rect: [min.x, min.y, size.x, size.y],
            uv: [sx, sy, 0.0, 0.0],
            color: linear(color),
            outline: [0.0; 4],
        });
    }

    /// A translucent area made of an ordered dither on the art's grid, as on a PlayStation:
    /// `color`'s alpha at its start, going to `end_alpha` at its other end, across or `down`.
    pub fn dither(&mut self, rect: Rect, color: Vec4, end_alpha: f32, down: bool) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 || (color.w <= 0.0 && end_alpha <= 0.0) {
            return;
        }
        let min = (rect.min * self.scale).round();
        let size = ((rect.max() * self.scale).round() - min).max(Vec2::ONE);
        let art = (ART_PIXEL * self.scale).round().max(1.0);
        let grid = size / art;
        self.quads.push(UiQuad {
            rect: [min.x, min.y, size.x, size.y],
            uv: [0.0, 0.0, grid.x, grid.y],
            color: linear(color),
            outline: [end_alpha.clamp(0.0, 1.0), if down { grid.y } else { grid.x }, f32::from(u8::from(down)), -2.0],
        });
    }

    /// A rectangle going from `left` to `right`, in visible bands, as a low color depth would.
    /// The alpha is `left`'s.
    pub fn gradient(&mut self, rect: Rect, left: Vec4, right: Vec4) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 || left.w <= 0.0 {
            return;
        }
        let min = (rect.min * self.scale).round();
        let size = ((rect.max() * self.scale).round() - min).max(Vec2::ONE);
        let end = linear(right);
        self.quads.push(UiQuad {
            rect: [min.x, min.y, size.x, size.y],
            uv: [0.0, 0.0, 1.0, 1.0],
            color: linear(left),
            outline: [end[0], end[1], end[2], -3.0],
        });
    }

    /// A panel of the game's style: dithered `fill` from the theme with a thin line around it.
    pub fn sheet(&mut self, rect: Rect) {
        let (fill, border) = (self.theme.fill, self.theme.border);
        let line = self.line();
        self.dither(rect.expand(-line), fill, fill.w, false);
        self.panel(rect, Vec4::ZERO, border);
    }

    /// The width of the thinnest line, in logical pixels: a whole number of real pixels.
    pub fn line(&self) -> f32 {
        self.scale.ceil().max(1.0) / self.scale
    }

    /// A filled rectangle with a thin border inside its edge.
    pub fn panel(&mut self, rect: Rect, fill: Vec4, border: Vec4) {
        let line = self.line();
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
        let rows = ROWS + 2;
        let min = (at - Vec2::new(unit, (TOP + 1) as f32 * unit)) * self.scale;
        let size = Vec2::new(cols as f32, rows as f32) * unit * self.scale;
        self.quads.push(UiQuad {
            rect: [min.x, min.y, size.x, size.y],
            uv: [(glyph.x + glyph.left - 1) as f32, (glyph.y - 1) as f32, cols as f32, rows as f32],
            color,
            outline,
        });
    }

    /// A push button. Returns true on the frame it is used: clicked, or Enter
    /// or Space with the focus.
    pub fn button(&mut self, rect: Rect, label: &str) -> bool {
        self.button_if(true, rect, label)
    }

    /// A button that is greyed out and does nothing unless `enabled`.
    pub fn button_if(&mut self, enabled: bool, rect: Rect, label: &str) -> bool {
        let response = self.widget(rect, label, enabled);
        let color = self.widget_frame(rect, &response);
        let size = self.theme.text_size;
        let shift = press_shift(&response);
        let label = self.written(label);
        if self.theme.frames {
            self.text(
                rect.point(Anchor::Center) + shift,
                &label,
                &TextStyle::new(size).color(color).anchor(Anchor::Center),
            );
        } else {
            let at = rect.point(Anchor::Left) + Vec2::X * POINTER_ROOM + shift;
            self.text(at, &label, &TextStyle::new(size).color(color).anchor(Anchor::Left));
        }
        response.clicked || response.keyed
    }

    /// A square that shows an item, used like a button. `label` identifies it, and isn't drawn.
    pub fn slot(&mut self, rect: Rect, label: &str, slot: Slot<'_>) -> SlotResponse {
        let response = self.widget(rect, label, true);
        let theme = self.theme;
        let line = self.line();
        let fill = match () {
            _ if response.pressed => theme.pressed,
            _ if response.focused || response.hovered => theme.hover,
            _ => theme.fill,
        };
        // The focus is a double line in the brightest color; a slot that's marked has a single one.
        let border = if response.focused || slot.marked { theme.accent } else { theme.border };
        self.panel(rect, fill, border);
        if response.focused {
            self.panel(rect.expand(line), Vec4::ZERO, theme.accent);
        }
        let color = if response.focused { theme.accent } else { theme.text };
        self.slot_contents(rect, slot, color, press_shift(&response));
        SlotResponse { used: response.clicked || response.keyed, focused: response.focused, hovered: response.hovered }
    }

    /// What goes inside a [`Ui::slot`], for drawing it somewhere else: a slot that can't be
    /// used, or a stack being moved.
    pub fn slot_contents(&mut self, rect: Rect, slot: Slot<'_>, color: Vec4, shift: Vec2) {
        let inner = Rect::new(rect.min + shift, rect.size).expand(-rect.size.x * 0.06);
        if let Some(image) = slot.image {
            // Item pictures are drawn at 48 pixels: a whole number of them per pixel of art.
            let side =
                if inner.size.x >= ICON_ART { (inner.size.x / ICON_ART).floor() * ICON_ART } else { inner.size.x };
            let centered = Rect::anchored(inner.point(Anchor::Center), Vec2::splat(side), Anchor::Center);
            self.image(centered, image, Vec4::ONE);
        } else if !slot.short.is_empty() {
            let style = TextStyle::new(16.0).color(color).anchor(Anchor::Center);
            self.text(inner.point(Anchor::Center), slot.short, &style);
        }
        if slot.count > 1 {
            let style = TextStyle::new(16.0).color(self.theme.text).anchor(Anchor::BottomRight);
            self.text(inner.point(Anchor::BottomRight), &slot.count.to_string(), &style);
        }
    }

    /// A row with `label` on the left and `value` on the right, used like a button.
    /// The label alone identifies it, so the focus stays when the value changes.
    pub fn button_row(&mut self, rect: Rect, label: &str, value: &str) -> bool {
        self.button_row_if(true, rect, label, value)
    }

    /// A [`Ui::button_row`] that is greyed out and does nothing unless `enabled`.
    pub fn button_row_if(&mut self, enabled: bool, rect: Rect, label: &str, value: &str) -> bool {
        let response = self.widget(rect, label, enabled);
        let color = self.row_frame(rect, label, &response);
        let area = value_area(rect, self.theme.text_size);
        let at = Vec2::new(area.max().x, area.point(Anchor::Center).y) + press_shift(&response);
        let value = self.written(value);
        self.text(at, &value, &TextStyle::new(self.theme.text_size).color(color).anchor(Anchor::Right));
        response.clicked || response.keyed
    }

    /// A labelled slider stepping by `step` over `range`, showing how far along
    /// it is as a percentage. Left and Right move it; so does dragging its track.
    /// Returns true on the frames `value` changes.
    pub fn slider(&mut self, rect: Rect, label: &str, value: &mut f32, range: RangeInclusive<f32>, step: f32) -> bool {
        self.slider_showing(rect, label, value, range, step, None)
    }

    /// A [`Ui::slider`] that shows its value with this many decimals, not as a percentage.
    pub fn number_slider(
        &mut self,
        rect: Rect,
        label: &str,
        value: &mut f32,
        range: RangeInclusive<f32>,
        step: f32,
        decimals: usize,
    ) -> bool {
        self.slider_showing(rect, label, value, range, step, Some(decimals))
    }

    fn slider_showing(
        &mut self,
        rect: Rect,
        label: &str,
        value: &mut f32,
        range: RangeInclusive<f32>,
        step: f32,
        decimals: Option<usize>,
    ) -> bool {
        let response = self.widget(rect, label, true);
        let color = self.row_frame(rect, label, &response);
        let (min, max) = (*range.start(), *range.end());
        let span = (max - min).max(f32::EPSILON);
        let style = TextStyle::new(self.theme.text_size).color(color).anchor(Anchor::Right);
        let shown = |value: f32, fraction: f32| match decimals {
            Some(decimals) => format!("{value:.decimals$}"),
            None => format!("{:.0}%", fraction * 100.0),
        };
        let number = self.measure(&shown(max, 1.0), &style).x.max(self.measure(&shown(min, 1.0), &style).x);
        let area = value_area(rect, self.theme.text_size);
        let height = (self.theme.text_size * 0.4).round();
        let track = Rect::anchored(
            area.point(Anchor::Left),
            Vec2::new(area.size.x - number - self.theme.text_size, height),
            Anchor::Left,
        );

        let input = self.widgets.input;
        let cursor = input.cursor.unwrap_or_default();
        if response.grabbed {
            self.widgets.dragging = cursor.x >= track.min.x - height && cursor.x <= track.max().x + height;
        }
        let mut next = None;
        if self.widgets.dragging && input.mouse_down && self.widgets.held == Some(widget_id(label)) {
            next = Some(min + span * ((cursor.x - track.min.x) / track.size.x.max(1.0)));
        }
        if response.focused && (input.left || input.right) {
            next = Some(*value + step * (f32::from(u8::from(input.right)) - f32::from(u8::from(input.left))));
        }
        let before = *value;
        if let Some(mut next) = next {
            if step > 0.0 {
                next = min + ((next - min) / step).round() * step;
            }
            *value = next.clamp(min, max);
        }

        let fraction = ((*value - min) / span).clamp(0.0, 1.0);
        let fill = if response.focused { self.theme.accent } else { self.theme.border };
        self.bar(track, fraction, fill, self.theme.track);
        let handle = Vec2::new(track.min.x + track.size.x * fraction, track.point(Anchor::Center).y);
        let handle_size = Vec2::new((height * 0.8).round(), height * 2.0);
        self.rect(Rect::anchored(handle, handle_size, Anchor::Center), color);
        let right = Vec2::new(area.max().x, area.point(Anchor::Center).y) + press_shift(&response);
        self.text(right, &shown(*value, fraction), &style);
        *value != before
    }

    /// A labelled on/off box. Returns true on the frames `value` changes.
    pub fn toggle(&mut self, rect: Rect, label: &str, value: &mut bool) -> bool {
        let response = self.widget(rect, label, true);
        let color = self.row_frame(rect, label, &response);
        let input = self.widgets.input;
        let flip = response.clicked || response.keyed || (response.focused && (input.left || input.right));
        if flip {
            *value = !*value;
        }
        let area = value_area(rect, self.theme.text_size);
        let side = (self.theme.text_size * 0.9).round();
        let right = Vec2::new(area.max().x, area.point(Anchor::Center).y) + press_shift(&response);
        let check = Rect::anchored(right, Vec2::splat(side), Anchor::Right);
        self.panel(check, self.theme.track, color);
        if *value {
            self.rect(check.expand(-(side * 0.2).round()), color);
        }
        flip
    }

    /// A labelled row that cycles through `options`, wrapping around: Left and
    /// Right go back and forward, Enter, Space and clicks go forward (a click
    /// on the left half of the value goes back). Returns true on the frames
    /// `selected` changes.
    pub fn choice(&mut self, rect: Rect, label: &str, options: &[&str], selected: &mut usize) -> bool {
        let response = self.widget(rect, label, true);
        let color = self.row_frame(rect, label, &response);
        let input = self.widgets.input;
        let area = value_area(rect, self.theme.text_size);
        let center = area.point(Anchor::Center);
        let clicked_left = response.clicked && input.cursor.is_some_and(|c| area.contains(c) && c.x < center.x);
        let back = clicked_left || (response.focused && input.left);
        let forward = (response.clicked && !clicked_left) || response.keyed || (response.focused && input.right);
        let count = options.len();
        let mut changed = false;
        if count > 0 && back != forward {
            *selected = (*selected + if back { count - 1 } else { 1 }) % count;
            changed = true;
        }

        let shift = press_shift(&response);
        let size = self.theme.text_size;
        let at = |anchor| TextStyle::new(size).color(color).anchor(anchor);
        self.text(area.point(Anchor::Left) + shift, "<", &at(Anchor::Left));
        self.text(area.point(Anchor::Right) + shift, ">", &at(Anchor::Right));
        if let Some(option) = options.get(*selected) {
            let option = self.written(option);
            self.text(center + shift, &option, &at(Anchor::Center));
        }
        changed
    }

    /// Registers a widget for this frame and works out how it stands.
    fn widget(&mut self, rect: Rect, label: &str, enabled: bool) -> Response {
        let id = widget_id(label);
        let widgets = &mut self.widgets;
        if enabled && !widgets.focus_settled {
            widgets.focus = Some(id);
            widgets.focus_settled = true;
        }
        widgets.drawn.push((id, enabled));

        let input = widgets.input;
        let free = widgets.held.is_none_or(|held| held == id);
        let hovered = enabled && free && input.cursor.is_some_and(|c| rect.contains(c));
        let grabbed = hovered && input.mouse_pressed;
        if hovered && (widgets.cursor_moved || grabbed) {
            widgets.focus = Some(id);
        }
        if grabbed {
            widgets.held = Some(id);
        }
        let held = widgets.held == Some(id);
        let focused = enabled && widgets.focus == Some(id);
        Response {
            enabled,
            hovered,
            focused,
            pressed: held && hovered && input.mouse_down,
            grabbed,
            clicked: held && hovered && input.mouse_released,
            keyed: focused && input.activate,
        }
    }

    /// The widget's background and border. Returns the color for what goes on it.
    fn widget_frame(&mut self, rect: Rect, response: &Response) -> Vec4 {
        let theme = self.theme;
        if !theme.frames {
            return self.bare_frame(rect, response);
        }
        let faded = |color: Vec4, alpha: f32| Vec4::new(color.x, color.y, color.z, color.w * alpha);
        let (fill, border, color) = if !response.enabled {
            (faded(theme.fill, 0.6), faded(theme.border, 0.35), theme.disabled)
        } else if response.pressed {
            (theme.pressed, theme.accent, theme.accent)
        } else if response.focused {
            (theme.hover, theme.accent, theme.accent)
        } else if response.hovered {
            (theme.hover, theme.border, theme.text)
        } else {
            (theme.fill, theme.border, theme.text)
        };
        self.panel(rect, fill, border);
        if response.focused {
            let marker = (theme.text_size / 6.0).round();
            self.rect(Rect::new(rect.min, Vec2::new(marker, rect.size.y)), theme.accent);
        }
        color
    }

    /// [`Ui::widget_frame`] with no box: a band and a pointer where the focus is.
    fn bare_frame(&mut self, rect: Rect, response: &Response) -> Vec4 {
        let theme = self.theme;
        if response.enabled && (response.focused || response.hovered) {
            self.dither(rect, if response.pressed { theme.pressed } else { theme.hover }, 0.0, false);
        }
        if response.focused {
            let middle = rect.point(Anchor::Center).y;
            for column in 0..POINTER_COLUMNS {
                let tall = ART_PIXEL * (2 * (POINTER_COLUMNS - column) - 1) as f32;
                let at = Vec2::new(rect.min.x + ART_PIXEL * (2 + column) as f32, middle - tall * 0.5);
                self.rect(Rect::new(at, Vec2::new(ART_PIXEL, tall)), theme.accent);
            }
        }
        match () {
            _ if !response.enabled => theme.disabled,
            _ if response.focused => theme.accent,
            _ => theme.text,
        }
    }

    /// The frame of a widget with its label on the left and its value on the right.
    fn row_frame(&mut self, rect: Rect, label: &str, response: &Response) -> Vec4 {
        let color = self.widget_frame(rect, response);
        let size = self.theme.text_size;
        let pad = if self.theme.frames { size * 0.75 } else { POINTER_ROOM };
        let at = rect.point(Anchor::Left) + Vec2::X * pad + press_shift(response);
        let label = self.written(label);
        self.text(at, &label, &TextStyle::new(size).color(color).anchor(Anchor::Left));
        color
    }

    /// `text` as the widgets write it.
    fn written(&self, text: &str) -> String {
        if self.theme.upper { text.to_uppercase() } else { text.to_owned() }
    }
}

/// How far a widget with no box keeps its text from the edge, for the pointer.
const POINTER_ROOM: f32 = 20.0;
/// How wide the pointer is, in pixels of art.
const POINTER_COLUMNS: u32 = 4;

/// A widget's value part: the right of its row, inside the padding. The rest is the label's.
fn value_area(rect: Rect, text_size: f32) -> Rect {
    let width = rect.size.x * 0.4 - text_size * 0.75;
    Rect::anchored(rect.point(Anchor::Right) - Vec2::X * text_size * 0.75, Vec2::new(width, rect.size.y), Anchor::Right)
}

/// Content sinks a pixel while the mouse holds a widget down.
fn press_shift(response: &Response) -> Vec2 {
    if response.pressed { Vec2::Y } else { Vec2::ZERO }
}

/// FNV-1a of the label.
fn widget_id(label: &str) -> u64 {
    label.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
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

    const ROW: Vec2 = Vec2::new(400.0, 40.0);

    fn row(n: u32) -> Rect {
        Rect::new(Vec2::new(100.0, 100.0 + 50.0 * n as f32), ROW)
    }

    /// A point on row `n`, as a fraction of its width.
    fn on_row(n: u32, x: f32) -> Option<Vec2> {
        Some(row(n).min + ROW * Vec2::new(x, 0.5))
    }

    /// One frame: `input`, then `widgets`. Returns what they returned.
    fn frame<T>(ui: &mut Ui, input: UiInput, widgets: impl FnOnce(&mut Ui) -> T) -> T {
        ui.begin(Vec2::new(1280.0, 720.0), 1.0);
        ui.set_input(input);
        widgets(ui)
    }

    /// Three buttons, the second disabled unless `second`. Returns which were used.
    fn three(ui: &mut Ui, input: UiInput, second: bool) -> [bool; 3] {
        frame(ui, input, |ui| {
            [ui.button(row(0), "um"), ui.button_if(second, row(1), "dois"), ui.button(row(2), "tres")]
        })
    }

    /// The background color the widget got, the first thing it draws.
    fn background(ui: &Ui) -> [f32; 4] {
        ui.quads()[0].color
    }

    #[test]
    fn pictures_take_their_own_place_in_the_atlas() {
        let mut ui = Ui::new(Vec2::new(1280.0, 720.0), 1.0);
        let version = ui.images().version;
        assert_eq!(ui.add_image(2, 2, &[255; 3]), None, "too few bytes for that size");
        let a = ui.add_image(2, 2, &[255; 16]).unwrap();
        let b = ui.add_image(3, 1, &[128; 12]).unwrap();
        assert!(ui.images().version > version, "the renderer has to know there's more to upload");
        let [ax, ay, aw, _] = ui.images().rects[a.0 as usize];
        let [bx, by, _, _] = ui.images().rects[b.0 as usize];
        assert_eq!(ay, by);
        assert!(bx > ax + aw, "a clear texel between them");
        let at = |x: u32, y: u32| ui.images().pixels[((y * IMAGE_ATLAS + x) * 4) as usize];
        assert_eq!((at(ax, ay), at(bx, by)), (255, 128));
        assert_eq!(at(ax + aw, ay), 0);

        let image_count = |ui: &Ui| ui.quads().iter().filter(|quad| quad.outline[3] < 0.0).count();
        frame(&mut ui, UiInput::default(), |ui| ui.image(row(0), b, Vec4::ONE));
        assert_eq!(image_count(&ui), 1);
        assert_eq!(ui.quads()[0].uv, [bx as f32, by as f32, 3.0, 1.0]);
        frame(&mut ui, UiInput::default(), |ui| ui.image(row(0), ImageId(99), Vec4::ONE));
        assert!(ui.is_empty(), "an id that doesn't exist draws nothing");
    }

    #[test]
    fn a_picture_that_does_not_fit_is_refused() {
        let mut ui = Ui::new(Vec2::new(1280.0, 720.0), 1.0);
        let big = vec![0; (IMAGE_ATLAS * IMAGE_ATLAS * 4) as usize];
        assert_eq!(ui.add_image(IMAGE_ATLAS, IMAGE_ATLAS, &big), None);
        assert!(ui.add_image(1, 1, &[0; 4]).is_some());
    }

    /// A grid of 3 by 2. Returns which of its slots has the focus.
    fn grid_frame(ui: &mut Ui, input: UiInput) -> Option<usize> {
        frame(ui, input, |ui| {
            let focused = (0..6).map(|at| ui.slot(row(at), &format!("slot {at}"), Slot::default()).focused);
            let focused = focused.collect::<Vec<_>>().iter().position(|&focused| focused);
            ui.grid(3);
            focused
        })
    }

    #[test]
    fn the_keys_move_through_a_grid_and_stop_at_its_ends() {
        let mut ui = Ui::new(Vec2::new(1280.0, 720.0), 1.0);
        assert_eq!(grid_frame(&mut ui, UiInput::default()), Some(0));
        assert_eq!(grid_frame(&mut ui, UiInput { up: true, ..Default::default() }), Some(0));
        assert_eq!(grid_frame(&mut ui, UiInput { left: true, ..Default::default() }), Some(0));
        assert_eq!(grid_frame(&mut ui, UiInput { down: true, ..Default::default() }), Some(3));
        assert_eq!(grid_frame(&mut ui, UiInput { right: true, ..Default::default() }), Some(4));
        assert_eq!(grid_frame(&mut ui, UiInput { down: true, ..Default::default() }), Some(4));
        assert_eq!(grid_frame(&mut ui, UiInput { up: true, ..Default::default() }), Some(1));
    }

    #[test]
    fn a_slot_is_used_like_a_button_and_shows_what_it_holds() {
        let mut ui = Ui::new(Vec2::new(1280.0, 720.0), 1.0);
        let draw = |ui: &mut Ui, input, count| {
            frame(ui, input, |ui| ui.slot(row(0), "slot", Slot { short: "mac", count, ..Default::default() }))
        };
        assert!(!draw(&mut ui, UiInput::default(), 1).used);
        let one = ui.quads().len();
        assert!(draw(&mut ui, UiInput { activate: true, ..Default::default() }, 1).used);
        assert!(draw(&mut ui, UiInput::default(), 12).focused);
        assert!(ui.quads().len() > one, "a stack of many says how many");
    }

    #[test]
    fn the_first_enabled_widget_takes_the_focus() {
        let mut ui = Ui::default();
        frame(&mut ui, UiInput::default(), |ui| {
            ui.button_if(false, row(0), "off");
            ui.button(row(1), "on");
        });
        assert!(ui.is_focused("on") && !ui.is_focused("off"));
    }

    #[test]
    fn hovering_moves_the_focus_and_each_state_has_its_look() {
        let mut ui = Ui::default();
        let away = UiInput { cursor: on_row(5, 0.5), ..Default::default() };
        let over = UiInput { cursor: on_row(2, 0.5), ..Default::default() };
        three(&mut ui, away, true);
        assert!(ui.is_focused("um"));

        // Plain: neither under the cursor nor focused.
        frame(&mut ui, away, |ui| {
            ui.button(row(2), "tres");
        });
        assert_eq!(background(&ui), linear(ui.theme.fill));

        // Moving over a widget takes the focus.
        three(&mut ui, away, true);
        three(&mut ui, over, true);
        assert!(ui.is_focused("tres"));
        let quads_focused = frame(&mut ui, over, |ui| {
            ui.button(row(2), "tres");
            ui.quads().to_vec()
        });
        assert_eq!(quads_focused[0].color, linear(ui.theme.hover));
        assert!(quads_focused.iter().any(|q| q.color == linear(ui.theme.accent)), "focus shows in the accent color");

        // A cursor resting on a widget doesn't fight the keyboard, and the widget
        // keeps its hover look without the accent.
        three(&mut ui, over, true);
        three(&mut ui, UiInput { up: true, ..over }, true);
        assert!(ui.is_focused("dois"));
        three(&mut ui, over, true);
        assert!(ui.is_focused("dois"));
        let quads_hovered = frame(&mut ui, over, |ui| {
            ui.button(row(2), "tres");
            ui.quads().to_vec()
        });
        assert_eq!(quads_hovered[0].color, linear(ui.theme.hover));
        assert!(quads_hovered.iter().all(|q| q.color != linear(ui.theme.accent)));

        // Disabled looks like neither, under the cursor or not.
        frame(&mut ui, over, |ui| {
            ui.button_if(false, row(2), "tres");
        });
        assert_ne!(background(&ui), linear(ui.theme.hover));
        assert_ne!(background(&ui), linear(ui.theme.fill));

        // While the cursor is grabbed the mouse isn't anywhere.
        let mut ui = Ui::default();
        three(&mut ui, away, true);
        three(&mut ui, over, true);
        let grabbed = UiInput { cursor: None, mouse_down: true, mouse_pressed: true, ..Default::default() };
        three(&mut ui, grabbed, true);
        assert!(ui.is_focused("tres"), "focus stays");
        let released = UiInput { cursor: None, mouse_released: true, ..Default::default() };
        assert_eq!(three(&mut ui, released, true), [false; 3]);
    }

    #[test]
    fn a_click_needs_press_and_release_on_the_same_widget() {
        let mut ui = Ui::default();
        let over = |n| UiInput { cursor: on_row(n, 0.5), ..Default::default() };
        three(&mut ui, over(5), true);
        three(&mut ui, over(2), true);

        let down = UiInput { mouse_down: true, mouse_pressed: true, ..over(2) };
        assert_eq!(three(&mut ui, down, true), [false; 3], "nothing until the release");
        assert!(ui.is_focused("tres"));
        frame(&mut ui, UiInput { mouse_down: true, ..over(2) }, |ui| {
            ui.button(row(2), "tres");
        });
        assert_eq!(background(&ui), linear(ui.theme.pressed), "held down looks pressed");
        let up = UiInput { mouse_released: true, ..over(2) };
        assert_eq!(three(&mut ui, up, true), [false, false, true]);
        assert_eq!(three(&mut ui, over(2), true), [false; 3], "once");

        // Dragged off before letting go: no click, and nothing else takes it.
        three(&mut ui, UiInput { mouse_down: true, mouse_pressed: true, ..over(2) }, true);
        three(&mut ui, UiInput { mouse_down: true, ..over(0) }, true);
        assert!(ui.is_focused("tres"), "a held widget keeps the mouse");
        assert_eq!(three(&mut ui, UiInput { mouse_released: true, ..over(0) }, true), [false; 3]);

        // Pressed off the widget and released on it: no click either.
        three(&mut ui, UiInput { mouse_down: true, mouse_pressed: true, ..over(5) }, true);
        assert_eq!(three(&mut ui, UiInput { mouse_released: true, ..over(0) }, true), [false; 3]);

        // A press and release between two frames still counts.
        three(&mut ui, over(0), true);
        let quick = UiInput { mouse_pressed: true, mouse_released: true, ..over(0) };
        assert_eq!(three(&mut ui, quick, true), [true, false, false]);
    }

    #[test]
    fn disabled_buttons_do_nothing() {
        let mut ui = Ui::default();
        let over = UiInput { cursor: on_row(1, 0.5), ..Default::default() };
        three(&mut ui, over, false);
        three(&mut ui, UiInput { cursor: on_row(1, 0.4), ..Default::default() }, false);
        assert!(!ui.is_focused("dois"), "the mouse can't focus it");
        three(&mut ui, UiInput { mouse_down: true, mouse_pressed: true, ..over }, false);
        assert_eq!(three(&mut ui, UiInput { mouse_released: true, ..over }, false), [false; 3]);
        assert_eq!(three(&mut ui, UiInput { activate: true, ..over }, false), [true, false, false], "Enter goes to um");

        // The keyboard skips it, and it can take the focus again once enabled.
        let mut ui = Ui::default();
        three(&mut ui, UiInput::default(), false);
        three(&mut ui, UiInput { down: true, ..Default::default() }, false);
        assert!(ui.is_focused("tres"));
        three(&mut ui, UiInput { down: true, ..Default::default() }, true);
        assert!(ui.is_focused("um"));
        three(&mut ui, UiInput { down: true, ..Default::default() }, true);
        assert!(ui.is_focused("dois"), "enabled last frame, so the keyboard sees it");

        // Disabled while focused: the focus moves on instead of getting lost.
        three(&mut ui, UiInput::default(), false);
        three(&mut ui, UiInput::default(), false);
        assert!(ui.is_focused("um"));
    }

    #[test]
    fn the_keyboard_moves_the_focus_and_activates() {
        let mut ui = Ui::default();
        let idle = UiInput::default();
        three(&mut ui, idle, true);
        assert!(ui.is_focused("um"));
        three(&mut ui, UiInput { up: true, ..idle }, true);
        assert!(ui.is_focused("tres"), "wraps upwards");
        three(&mut ui, UiInput { down: true, ..idle }, true);
        assert!(ui.is_focused("um"), "wraps downwards");
        three(&mut ui, UiInput { down: true, ..idle }, true);
        assert!(ui.is_focused("dois"));
        three(&mut ui, UiInput { up: true, down: true, ..idle }, true);
        assert!(ui.is_focused("dois"), "both cancel out");
        assert_eq!(three(&mut ui, UiInput { activate: true, ..idle }, true), [false, true, false], "only the focused");
    }

    #[test]
    fn input_comes_from_the_engines_input_in_logical_pixels() {
        let mut input = Input::default();
        input.set_cursor(Vec2::new(300.0, 200.0));
        input.key_down(KeyCode::KeyS);
        input.key_down(KeyCode::Enter);
        input.button_down(MouseButton::Left);
        let ui = UiInput::from_input(&input, 2.0);
        assert_eq!(ui.cursor, Some(Vec2::new(150.0, 100.0)));
        assert!(ui.down && ui.activate && ui.mouse_down && ui.mouse_pressed);
        assert!(!ui.up && !ui.mouse_released);

        input.end_frame();
        let held = UiInput::from_input(&input, 2.0);
        assert!(!held.down && !held.activate && !held.mouse_pressed, "held keys don't act again");
        input.key_repeat(KeyCode::KeyS);
        input.key_repeat(KeyCode::Enter);
        let repeated = UiInput::from_input(&input, 2.0);
        assert!(repeated.down, "holding a direction keeps moving");
        assert!(!repeated.activate, "holding Enter doesn't keep activating");
        input.button_up(MouseButton::Left);
        assert!(UiInput::from_input(&input, 2.0).mouse_released);
    }

    #[test]
    fn screens_change_the_focus() {
        let mut ui = Ui::default();
        three(&mut ui, UiInput::default(), true);
        three(&mut ui, UiInput { down: true, ..Default::default() }, true);
        assert!(ui.is_focused("dois"));

        // Another screen: its first widget takes over at once.
        ui.reset_focus();
        frame(&mut ui, UiInput::default(), |ui| ui.button(row(0), "outra"));
        assert!(ui.is_focused("outra"));

        // Coming back, the game can say where.
        ui.set_focus("tres");
        three(&mut ui, UiInput::default(), true);
        assert!(ui.is_focused("tres"));
        three(&mut ui, UiInput::default(), true);
        assert!(ui.is_focused("tres"));

        // A focus nobody draws anymore heals itself.
        ui.set_focus("nada");
        three(&mut ui, UiInput::default(), true);
        three(&mut ui, UiInput::default(), true);
        assert!(ui.is_focused("um"));
    }

    #[test]
    fn sliders_follow_the_keyboard_and_the_mouse() {
        let mut ui = Ui::default();
        let mut value = 0.5;
        let idle = UiInput::default();
        let mut slide = |ui: &mut Ui, input| {
            frame(ui, input, |ui| {
                let changed = ui.slider(row(0), "volume", &mut value, 0.0..=1.0, 0.25);
                (changed, ui.button(row(1), "outro"), value)
            })
        };
        assert_eq!(slide(&mut ui, idle), (false, false, 0.5));
        assert_eq!(slide(&mut ui, UiInput { right: true, ..idle }), (true, false, 0.75));
        assert_eq!(slide(&mut ui, UiInput { left: true, ..idle }), (true, false, 0.5));
        assert_eq!(slide(&mut ui, UiInput { left: true, right: true, ..idle }).2, 0.5);
        assert_eq!(slide(&mut ui, UiInput { right: true, ..idle }).2, 0.75);
        assert_eq!(slide(&mut ui, UiInput { right: true, ..idle }).2, 1.0);
        assert_eq!(slide(&mut ui, UiInput { right: true, ..idle }), (false, false, 1.0), "stops at the end");

        // Not focused: arrows leave it alone.
        slide(&mut ui, UiInput { down: true, ..idle });
        assert_eq!(slide(&mut ui, UiInput { left: true, ..idle }), (false, false, 1.0));

        // The track starts where the value part of the row does.
        let track_start = value_area(row(0), ui.theme.text_size).min.x;
        let at = |x: f32| Some(Vec2::new(x, row(0).point(Anchor::Center).y));
        // Pressing on the label doesn't jump, even though it takes the focus.
        slide(&mut ui, UiInput { cursor: at(120.0), ..idle });
        let press = UiInput { cursor: at(120.0), mouse_down: true, mouse_pressed: true, ..idle };
        assert_eq!(slide(&mut ui, press), (false, false, 1.0));
        assert_eq!(slide(&mut ui, UiInput { cursor: at(120.0), mouse_released: true, ..idle }).2, 1.0);
        assert!(ui.is_focused("volume"));

        // Pressing on the track grabs it; it follows the cursor anywhere, in steps, until released.
        slide(&mut ui, UiInput { cursor: at(track_start), ..idle });
        let press = UiInput { cursor: at(track_start), mouse_down: true, mouse_pressed: true, ..idle };
        assert_eq!(slide(&mut ui, press), (true, false, 0.0));
        let drag = |x, y: f32| UiInput { cursor: Some(Vec2::new(x, y)), mouse_down: true, ..idle };
        assert_eq!(slide(&mut ui, drag(1000.0, 400.0)).2, 1.0, "past the end, away from the row");
        assert_eq!(slide(&mut ui, drag(0.0, 400.0)).2, 0.0);
        let (_, other, _) = slide(&mut ui, drag(300.0, row(1).point(Anchor::Center).y));
        assert!(!other, "other widgets ignore the mouse meanwhile");
        let released = UiInput { cursor: at(track_start), mouse_released: true, ..idle };
        let value_there = slide(&mut ui, released).2;
        assert_eq!(slide(&mut ui, UiInput { cursor: at(1000.0), ..idle }).2, value_there, "let go");
    }

    #[test]
    fn toggles_and_choices_answer_both_ways() {
        let mut ui = Ui::default();
        let idle = UiInput::default();
        let mut on = false;
        let mut toggle = |ui: &mut Ui, input| frame(ui, input, |ui| ui.toggle(row(0), "cheia", &mut on));
        assert!(!toggle(&mut ui, idle));
        assert!(toggle(&mut ui, UiInput { activate: true, ..idle }));
        assert!(toggle(&mut ui, UiInput { right: true, ..idle }));
        let over = UiInput { cursor: on_row(0, 0.5), ..idle };
        toggle(&mut ui, over);
        assert!(!toggle(&mut ui, UiInput { mouse_down: true, mouse_pressed: true, ..over }));
        assert!(toggle(&mut ui, UiInput { mouse_released: true, ..over }));
        assert!(on, "flipped three times from off");

        let mut ui = Ui::default();
        let options = ["a", "b", "c"];
        let mut selected = 0;
        let mut choose = |ui: &mut Ui, input| {
            frame(ui, input, |ui| (ui.choice(row(0), "escala", &options, &mut selected), selected))
        };
        assert_eq!(choose(&mut ui, idle), (false, 0));
        assert_eq!(choose(&mut ui, UiInput { left: true, ..idle }), (true, 2), "wraps");
        assert_eq!(choose(&mut ui, UiInput { right: true, ..idle }), (true, 0));
        assert_eq!(choose(&mut ui, UiInput { activate: true, ..idle }), (true, 1));
        // The left part of the value goes back, the rest forward.
        let left = UiInput { cursor: on_row(0, 0.65), ..idle };
        choose(&mut ui, left);
        choose(&mut ui, UiInput { mouse_down: true, mouse_pressed: true, ..left });
        assert_eq!(choose(&mut ui, UiInput { mouse_released: true, ..left }), (true, 0));
        let right = UiInput { cursor: on_row(0, 0.9), ..idle };
        choose(&mut ui, right);
        choose(&mut ui, UiInput { mouse_down: true, mouse_pressed: true, ..right });
        assert_eq!(choose(&mut ui, UiInput { mouse_released: true, ..right }), (true, 1));
    }

    #[test]
    fn text_is_measured_in_font_pixels() {
        let ui = Ui::default();
        let one = ui.measure("I", &style());
        assert_eq!(one.y, GLYPH as f32 * 2.0, "one line is the height of the text's box");
        assert_eq!(one.x, FONT.glyph('I').width as f32 * 2.0, "no trailing spacing");
        let two = ui.measure("I\nI", &style());
        assert_eq!(two, Vec2::new(one.x, (LINE + GLYPH) as f32 * 2.0));
        assert!(ui.measure("mmm", &style()).x > ui.measure("...", &style()).x, "proportional");
        assert_eq!(ui.measure("", &style()), Vec2::new(0.0, GLYPH as f32 * 2.0));
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
        assert_eq!(a.rect[1], 20.0 * 2.0 - (TOP + 1) as f32 * unit, "the rows above the box, and the margin");
        assert_eq!(a.rect[3], (ROWS + 2) as f32 * unit);
        assert_eq!(a.uv[2] * unit, a.rect[2], "atlas texels map to whole font pixels");

        ui.begin(Vec2::new(640.0, 360.0), 1.0);
        assert!(ui.is_empty(), "begin clears the frame");
    }

    #[test]
    fn the_art_keeps_whole_pixels_on_any_window() {
        let art = |height: f32| ART_PIXEL * pixel_scale(height);
        assert_eq!((art(720.0), art(1080.0), art(1440.0), art(2160.0)), (2.0, 3.0, 4.0, 6.0));
        assert_eq!(pixel_scale(400.0), 1.0, "small windows don't shrink it further");
    }

    #[test]
    fn rectangles_land_on_whole_pixels_and_a_line_stays_visible() {
        let mut ui = Ui::new(Vec2::new(1280.0, 720.0), 1.5);
        assert!((ui.line() - 2.0 / 1.5).abs() < 1e-6, "two real pixels at this scale");
        ui.rect(Rect::new(Vec2::new(10.3, 0.0), Vec2::new(0.1, 10.0)), Vec4::ONE);
        let [x, _, width, _] = ui.quads()[0].rect;
        assert_eq!(x.fract(), 0.0);
        assert_eq!(width, 1.0, "never thinner than a pixel");
    }

    #[test]
    fn a_dither_counts_the_art_pixels_and_says_how_it_fades() {
        let mut ui = Ui::new(Vec2::new(1280.0, 720.0), 1.0);
        ui.dither(Rect::new(Vec2::ZERO, Vec2::new(40.0, 10.0)), Vec4::new(0.0, 0.0, 0.0, 0.8), 0.2, true);
        let quad = ui.quads()[0];
        assert_eq!(quad.uv, [0.0, 0.0, 20.0, 5.0]);
        assert_eq!(quad.outline, [0.2, 5.0, 1.0, -2.0]);
    }

    #[test]
    fn a_gradient_carries_both_ends() {
        let mut ui = Ui::default();
        ui.gradient(Rect::new(Vec2::ZERO, Vec2::new(40.0, 10.0)), Vec4::new(0.0, 0.0, 0.0, 0.5), Vec4::ONE);
        let quad = ui.quads()[0];
        assert_eq!(quad.color[3], 0.5);
        assert_eq!(quad.outline, [1.0, 1.0, 1.0, -3.0]);
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
