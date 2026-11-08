//! The built-in pixel font: Departure Mono (see `CREDITS.md`), baked into `font_data.rs` by
//! `tools/gen_ui_font.py`, for printable ASCII and Latin-1 (accents for Portuguese, Spanish,
//! French...), packed into one small atlas with an outline channel. Each glyph is trimmed to
//! its ink, so the text comes out proportional.

use std::sync::LazyLock;

use crate::font_data::{BASIC, COLUMNS as GLYPH_COLUMNS, LATIN, ROWS as GLYPH_ROWS};

/// Rows of every glyph bitmap, including the accents above and the descenders below.
pub(crate) const ROWS: u32 = GLYPH_ROWS as u32;
/// The rows above the text's box, which only accents reach.
pub(crate) const TOP: u32 = 3;
/// Height of the text's box in font units: what layout counts, from the top of the capitals
/// to just under the baseline. Accents and descenders stick out of it.
pub(crate) const GLYPH: u32 = 10;
/// Distance between baselines, in font units.
pub(crate) const LINE: u32 = 13;
/// Font units in a [`crate::ui::TextStyle::size`] of 8: sizes in multiples of 8 are a whole
/// number of pixels per font unit.
pub(crate) const UNIT_SIZE: f32 = 8.0;
/// Advance of a blank glyph (space), in font units.
const SPACE: u32 = 4;
/// Each glyph sits in a cell with this much empty border, so the outline and
/// linear filtering never reach the neighbouring cell.
const PAD: u32 = 2;
const CELL_WIDTH: u32 = GLYPH_COLUMNS as u32 + 2 * PAD;
const CELL_HEIGHT: u32 = ROWS + 2 * PAD;
const COLUMNS: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Glyph {
    /// Atlas texel of the bitmap's top-left pixel.
    pub x: u32,
    pub y: u32,
    /// Leftmost lit column of the bitmap.
    pub left: u32,
    /// Lit columns, from `left`; 0 for blank glyphs.
    pub width: u32,
}

impl Glyph {
    /// Horizontal advance in font units, including one unit of spacing.
    pub fn advance(&self) -> u32 {
        if self.width == 0 { SPACE } else { self.width + 1 }
    }
}

pub(crate) struct Font {
    /// By code point; `None` for characters the font doesn't have.
    glyphs: Box<[Option<Glyph>; 256]>,
    fallback: Glyph,
    /// Centre of a fully lit texel, for solid rectangles.
    pub solid: [f32; 2],
    pub width: u32,
    pub height: u32,
    /// Two bytes per texel: glyph coverage, then the outline (the glyph grown
    /// by one texel in every direction, the glyph included).
    pub pixels: Vec<u8>,
}

pub(crate) static FONT: LazyLock<Font> = LazyLock::new(Font::build);

impl Font {
    pub fn glyph(&self, c: char) -> Glyph {
        let code = c as usize;
        if c.is_whitespace() && code < 256 {
            return self.glyphs[' ' as usize].unwrap_or(self.fallback);
        }
        self.glyphs.get(code).copied().flatten().unwrap_or(self.fallback)
    }

    #[cfg(test)]
    pub fn has(&self, c: char) -> bool {
        self.glyphs.get(c as usize).is_some_and(Option::is_some)
    }

    fn build() -> Self {
        let codes: Vec<u32> = (0x20..0x7F).chain(0xA0..0x100).collect();
        let slots = codes.len() as u32 + 1;
        let width = COLUMNS * CELL_WIDTH;
        let height = slots.div_ceil(COLUMNS) * CELL_HEIGHT;
        let mut coverage = vec![0u8; (width * height) as usize];
        let cell = |slot: u32| ((slot % COLUMNS) * CELL_WIDTH, (slot / COLUMNS) * CELL_HEIGHT);

        let mut glyphs = Box::new([None; 256]);
        for (slot, &code) in codes.iter().enumerate() {
            let rows = if code < 0x80 { BASIC[code as usize - 0x20] } else { LATIN[(code - 0xA0) as usize] };
            let (cx, cy) = cell(slot as u32);
            let (x, y) = (cx + PAD, cy + PAD);
            let lit = rows.iter().fold(0u16, |acc, row| acc | row);
            for (r, row) in rows.iter().enumerate() {
                for c in 0..GLYPH_COLUMNS as u32 {
                    if row & (1 << c) != 0 {
                        coverage[((y + r as u32) * width + x + c) as usize] = 255;
                    }
                }
            }
            let (left, width) = if lit == 0 {
                (0, 0)
            } else {
                (lit.trailing_zeros(), u16::BITS - lit.leading_zeros() - lit.trailing_zeros())
            };
            glyphs[code as usize] = Some(Glyph { x, y, left, width });
        }
        let (sx, sy) = cell(slots - 1);
        for y in sy..sy + CELL_HEIGHT {
            coverage[(y * width + sx) as usize..(y * width + sx + CELL_WIDTH) as usize].fill(255);
        }

        let mut pixels = vec![0u8; coverage.len() * 2];
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) as usize;
                let near = (y.saturating_sub(1)..=(y + 1).min(height - 1))
                    .flat_map(|ny| (x.saturating_sub(1)..=(x + 1).min(width - 1)).map(move |nx| (nx, ny)))
                    .any(|(nx, ny)| coverage[(ny * width + nx) as usize] != 0);
                pixels[i * 2] = coverage[i];
                pixels[i * 2 + 1] = if near { 255 } else { 0 };
            }
        }

        let fallback = glyphs['?' as usize].expect("'?' is printable ASCII");
        let solid = [(sx + CELL_WIDTH / 2) as f32 + 0.5, (sy + CELL_HEIGHT / 2) as f32 + 0.5];
        Self { glyphs, fallback, solid, width, height, pixels }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_and_portuguese_accents_have_glyphs() {
        for c in "AZaz09!?.,:;-+/()[]ãõçéêáíóúâôàÇÃ·".chars() {
            assert!(FONT.has(c), "{c:?}");
            assert!(FONT.glyph(c).width > 0, "{c:?} is blank");
        }
        assert_eq!(FONT.glyph(' ').width, 0);
        assert_eq!(FONT.glyph('\t'), FONT.glyph(' '));
        assert!(!FONT.has('€'));
        assert_eq!(FONT.glyph('€'), FONT.glyph('?'), "missing characters show as '?'");
    }

    #[test]
    fn glyphs_are_trimmed_to_their_ink() {
        let dot = FONT.glyph('.');
        let m = FONT.glyph('m');
        assert!(dot.width < m.width, "proportional: . {} vs m {}", dot.width, m.width);
        // The trimmed columns are lit at both edges.
        let lit = |x: u32| (0..ROWS).any(|r| FONT.pixels[(((m.y + r) * FONT.width + x) * 2) as usize] != 0);
        assert!(lit(m.x + m.left) && lit(m.x + m.left + m.width - 1));
    }

    #[test]
    fn outline_surrounds_the_glyph() {
        let g = FONT.glyph('I');
        let texel = |x: u32, y: u32| {
            let i = ((y * FONT.width + x) * 2) as usize;
            (FONT.pixels[i], FONT.pixels[i + 1])
        };
        let cx = g.x + g.left;
        let cy = (g.y..g.y + ROWS).find(|&y| texel(cx, y).0 == 255).expect("the top-left of 'I' is lit");
        assert_eq!(texel(cx - 1, cy - 1), (0, 255), "outline only, diagonally outside");
        assert_eq!(texel(cx - 2, cy - 2), (0, 0));
        let [sx, sy] = FONT.solid;
        assert_eq!(texel(sx as u32, sy as u32), (255, 255));
    }
}
