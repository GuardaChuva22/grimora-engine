//! Grids of distances, for shapes that the map only draws in coarse texels: the coast, as a
//! smooth signed distance whose zero is a curve rather than a staircase.

use glam::DVec2;

/// A grid of values over a rectangle of the world, read back with bilinear filtering.
#[derive(Debug, Clone, Default)]
pub struct Field {
    pub min: DVec2,
    pub cell: f64,
    pub nx: usize,
    pub nz: usize,
    pub values: Vec<f64>,
}

impl Field {
    /// The middle of cell `i`, in world meters.
    pub fn at(&self, i: usize) -> DVec2 {
        self.min + DVec2::new((i % self.nx) as f64 + 0.5, (i / self.nx) as f64 + 0.5) * self.cell
    }

    /// The value at a point, or `None` outside the grid.
    pub fn sample(&self, p: DVec2) -> Option<f64> {
        if self.values.is_empty() {
            return None;
        }
        let g = (p - self.min) / self.cell - 0.5;
        if g.x < -0.5 || g.y < -0.5 || g.x > self.nx as f64 - 0.5 || g.y > self.nz as f64 - 0.5 {
            return None;
        }
        let (x0, z0) = (g.x.floor(), g.y.floor());
        let (tx, tz) = (g.x - x0, g.y - z0);
        let cell = |x: f64, z: f64| {
            let x = (x.max(0.0) as usize).min(self.nx - 1);
            let z = (z.max(0.0) as usize).min(self.nz - 1);
            self.values[z * self.nx + x]
        };
        let top = cell(x0, z0) + (cell(x0 + 1.0, z0) - cell(x0, z0)) * tx;
        let bottom = cell(x0, z0 + 1.0) + (cell(x0 + 1.0, z0 + 1.0) - cell(x0, z0 + 1.0)) * tx;
        Some(top + (bottom - top) * tz)
    }
}

/// Meters from each cell to the nearest cell where `source` holds (0 there), by two chamfer
/// passes (straight steps and diagonals).
pub fn chamfer(source: &[bool], nx: usize, nz: usize, cell: f64) -> Vec<f64> {
    let mut d: Vec<f64> = source.iter().map(|&s| if s { 0.0 } else { f64::MAX }).collect();
    let diagonal = cell * std::f64::consts::SQRT_2;
    let steps = [(-1i64, 0i64, cell), (0, -1, cell), (-1, -1, diagonal), (1, -1, diagonal)];
    let mut relax = |x: i64, z: i64, sign: i64| {
        let i = z as usize * nx + x as usize;
        for &(dx, dz, cost) in &steps {
            let (px, pz) = (x + dx * sign, z + dz * sign);
            if px >= 0 && pz >= 0 && (px as usize) < nx && (pz as usize) < nz {
                let j = pz as usize * nx + px as usize;
                if d[j] + cost < d[i] {
                    d[i] = d[j] + cost;
                }
            }
        }
    };
    for z in 0..nz as i64 {
        for x in 0..nx as i64 {
            relax(x, z, 1);
        }
    }
    for z in (0..nz as i64).rev() {
        for x in (0..nx as i64).rev() {
            relax(x, z, -1);
        }
    }
    d
}

/// Signed meters to the edge between `inside` and the rest: positive inside. The edge lies
/// half a cell from the middles of the cells on each side of it.
pub fn signed_distance(inside: &[bool], nx: usize, nz: usize, cell: f64) -> Vec<f64> {
    let outside: Vec<bool> = inside.iter().map(|&i| !i).collect();
    let to_out = chamfer(&outside, nx, nz, cell);
    let to_in = chamfer(inside, nx, nz, cell);
    let half = cell * 0.5;
    inside
        .iter()
        .zip(to_out.iter().zip(&to_in))
        .map(|(&i, (&o, &n))| if i { o.min(1e7) - half } else { half - n.min(1e7) })
        .collect()
}

/// A box blur run three times, about a Gaussian `radius` cells wide; the edges clamp.
pub fn blur(values: &mut [f64], nx: usize, nz: usize, radius: usize) {
    if radius == 0 {
        return;
    }
    let mut line = Vec::new();
    for _ in 0..3 {
        for z in 0..nz {
            line.clear();
            line.extend((0..nx).map(|x| values[z * nx + x]));
            box_line(&line, radius, |x, v| values[z * nx + x] = v);
        }
        for x in 0..nx {
            line.clear();
            line.extend((0..nz).map(|z| values[z * nx + x]));
            box_line(&line, radius, |z, v| values[z * nx + x] = v);
        }
    }
}

fn box_line(line: &[f64], radius: usize, mut out: impl FnMut(usize, f64)) {
    let n = line.len() as i64;
    let r = radius as i64;
    let at = |i: i64| line[i.clamp(0, n - 1) as usize];
    let mut sum: f64 = (-r..=r).map(at).sum();
    let width = (2 * r + 1) as f64;
    for i in 0..n {
        out(i as usize, sum / width);
        sum += at(i + r + 1) - at(i - r);
    }
}
