//! The noise texture the scene shader paints clouds and stains with. Sampling
//! it costs a fraction of computing noise per pixel, and blown up it has the
//! soft blocks of a low resolution texture, which is the look we want anyway.

use grimora_core::{Rng, TextureData};

const SIZE: usize = 256;
const OCTAVES: usize = 5;
/// Lattice cells across the texture in the first octave: the size of the largest blotches.
const FIRST_CELLS: usize = 4;

/// Four unrelated channels of fractal noise, stretched to the whole 0..=1 range,
/// that repeat seamlessly in both directions.
pub fn noise_texture() -> TextureData {
    let mut rng = Rng::new(0x6e6f_6973);
    let channels: [Vec<f32>; 4] = std::array::from_fn(|_| fractal(&mut rng));
    let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
    for i in 0..SIZE * SIZE {
        for channel in &channels {
            rgba.push((channel[i] * 255.0).round() as u8);
        }
    }
    TextureData { name: "noise".to_string(), width: SIZE as u32, height: SIZE as u32, rgba }
}

fn fractal(rng: &mut Rng) -> Vec<f32> {
    let mut sum = vec![0.0; SIZE * SIZE];
    let mut amplitude = 1.0;
    for octave in 0..OCTAVES {
        add_octave(&mut sum, FIRST_CELLS << octave, amplitude, rng);
        amplitude *= 0.55;
    }
    let (low, high) = sum.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    sum.iter().map(|v| (v - low) / (high - low).max(1e-6)).collect()
}

/// Smooth value noise on a lattice of `cells` that wraps around.
fn add_octave(sum: &mut [f32], cells: usize, amplitude: f32, rng: &mut Rng) {
    let lattice: Vec<f32> = (0..cells * cells).map(|_| rng.range(0.0, 1.0)).collect();
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    for y in 0..SIZE {
        let fy = y as f32 / SIZE as f32 * cells as f32;
        let (y0, ty) = (fy as usize % cells, smooth(fy.fract()));
        let y1 = (y0 + 1) % cells;
        for x in 0..SIZE {
            let fx = x as f32 / SIZE as f32 * cells as f32;
            let (x0, tx) = (fx as usize % cells, smooth(fx.fract()));
            let x1 = (x0 + 1) % cells;
            let top = lattice[y0 * cells + x0] * (1.0 - tx) + lattice[y0 * cells + x1] * tx;
            let bottom = lattice[y1 * cells + x0] * (1.0 - tx) + lattice[y1 * cells + x1] * tx;
            sum[y * SIZE + x] += (top * (1.0 - ty) + bottom * ty) * amplitude;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_repeats_seamlessly_and_uses_the_whole_range() {
        let noise = noise_texture();
        let at = |x: usize, y: usize, c: usize| noise.rgba[(y * SIZE + x) * 4 + c] as i32;
        for c in 0..4 {
            let (mut edge, mut inside) = (0, 0);
            for i in 0..SIZE {
                edge += (at(SIZE - 1, i, c) - at(0, i, c)).abs() + (at(i, SIZE - 1, c) - at(i, 0, c)).abs();
                inside += (at(100, i, c) - at(101, i, c)).abs() + (at(i, 100, c) - at(i, 101, c)).abs();
            }
            // Crossing the seam is no bigger a step than any other.
            assert!(edge < inside * 2, "channel {c}: {edge} at the seam, {inside} inside");
            let values: Vec<i32> = (0..SIZE * SIZE).map(|i| noise.rgba[i * 4 + c] as i32).collect();
            assert_eq!((values.iter().min(), values.iter().max()), (Some(&0), Some(&255)));
        }
        // The channels are unrelated.
        let same = (0..SIZE * SIZE).filter(|i| noise.rgba[i * 4] == noise.rgba[i * 4 + 1]).count();
        assert!(same < SIZE * SIZE / 10);
    }
}
