//! Noise in `f64` on an integer lattice, so it is the same anywhere in a world thousands of
//! kilometers wide, in any order and on any thread.

/// A well-mixed 64-bit hash of a lattice point.
pub fn hash(x: i64, z: i64, seed: u64) -> u64 {
    let mut h = seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// A lattice point's value, -1 to 1.
fn lattice(x: i64, z: i64, seed: u64) -> f64 {
    (hash(x, z, seed) >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

fn quintic(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise with one lattice cell per unit, -1 to 1.
pub fn value(x: f64, z: f64, seed: u64) -> f64 {
    let (fx, fz) = (x.floor(), z.floor());
    let (ix, iz) = (fx as i64, fz as i64);
    let (tx, tz) = (quintic(x - fx), quintic(z - fz));
    let a = lattice(ix, iz, seed);
    let b = lattice(ix + 1, iz, seed);
    let c = lattice(ix, iz + 1, seed);
    let d = lattice(ix + 1, iz + 1, seed);
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * tz
}

/// Octaves of [`value`], each twice as fine and half as strong, scaled back to about -1 to 1.
pub fn fbm(x: f64, z: f64, octaves: u32, seed: u64) -> f64 {
    let (mut sum, mut amplitude, mut total, mut frequency) = (0.0, 1.0, 0.0, 1.0);
    for octave in 0..octaves {
        // A turn between octaves keeps their lattices from lining up.
        let (rx, rz) = (x * 0.8 - z * 0.6, x * 0.6 + z * 0.8);
        let (x, z) = if octave % 2 == 1 { (rx, rz) } else { (x, z) };
        sum += value(x * frequency, z * frequency, seed.wrapping_add(u64::from(octave) * 7919)) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    sum / total
}

/// Sharp crests: 0 in the valleys, 1 on the ridges.
pub fn ridged(x: f64, z: f64, octaves: u32, seed: u64) -> f64 {
    let (mut sum, mut amplitude, mut total, mut frequency) = (0.0, 1.0, 0.0, 1.0);
    for octave in 0..octaves {
        let n = 1.0 - value(x * frequency, z * frequency, seed.wrapping_add(u64::from(octave) * 104_729)).abs();
        sum += n * n * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.1;
    }
    sum / total
}

pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
