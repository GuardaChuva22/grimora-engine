//! Bounding volumes and frustum tests, for skipping what the camera can't see.

use glam::{Mat4, Vec3, Vec4};

use crate::{Camera, Transform};

/// A sphere enclosing a mesh or an object.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BoundingSphere {
    pub center: Vec3,
    pub radius: f32,
}

impl BoundingSphere {
    /// A sphere around `points`, centered on their bounding box. Not the
    /// smallest possible, but close for compact shapes and cheap to build.
    pub fn from_points(points: impl IntoIterator<Item = Vec3> + Clone) -> Self {
        let (min, max) = points
            .clone()
            .into_iter()
            .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(p), hi.max(p)));
        if min.x > max.x {
            return Self::default();
        }
        let center = (min + max) * 0.5;
        let radius = points.into_iter().map(|p| p.distance_squared(center)).fold(0.0, f32::max).sqrt();
        Self { center, radius }
    }

    /// The sphere after moving, rotating and scaling it with `transform`.
    /// Non-uniform scale grows it by the largest axis, so it stays enclosing.
    pub fn transformed(&self, transform: &Transform) -> Self {
        Self {
            center: transform.position + transform.rotation * (transform.scale * self.center),
            radius: self.radius * transform.scale.abs().max_element(),
        }
    }

    /// Fraction of the screen height the sphere covers as seen by `camera`
    /// (1 = fills it). Infinite when the camera is inside.
    pub fn screen_size(&self, camera: &Camera) -> f32 {
        let distance = self.center.distance(camera.position);
        if distance <= self.radius { f32::INFINITY } else { self.radius / (distance * (camera.fov_y * 0.5).tan()) }
    }
}

/// The six planes of a camera's or light's view volume, normals pointing inwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Extracts the planes of a view-projection matrix with depth in 0..1 (the
    /// wgpu/DirectX convention used throughout the engine).
    pub fn from_view_projection(m: Mat4) -> Self {
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| p / p.truncate().length().max(1e-12));
        Self { planes }
    }

    /// False only when the sphere is entirely outside; spheres near a corner
    /// may pass, which is fine for culling.
    pub fn intersects_sphere(&self, sphere: BoundingSphere) -> bool {
        self.planes.iter().all(|p| p.truncate().dot(sphere.center) + p.w >= -sphere.radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn sphere_encloses_its_points() {
        let points = [Vec3::new(-1.0, 0.0, 0.0), Vec3::new(3.0, 2.0, 0.0), Vec3::new(1.0, 1.0, 4.0)];
        let s = BoundingSphere::from_points(points);
        assert!(points.iter().all(|p| p.distance(s.center) <= s.radius + 1e-5));
        assert_eq!(BoundingSphere::from_points(std::iter::empty::<Vec3>()), BoundingSphere::default());
    }

    #[test]
    fn transformed_sphere_follows_the_object() {
        let s = BoundingSphere { center: Vec3::new(0.0, 1.0, 0.0), radius: 1.0 };
        let t = Transform {
            position: Vec3::new(10.0, 0.0, 0.0),
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: Vec3::new(2.0, 3.0, 1.0),
        };
        let moved = s.transformed(&t);
        // (0, 3, 0) after scaling, then rotated 90° about Z to (-3, 0, 0).
        assert!(moved.center.abs_diff_eq(Vec3::new(7.0, 0.0, 0.0), 1e-5), "{}", moved.center);
        assert_eq!(moved.radius, 3.0);
    }

    #[test]
    fn screen_size_shrinks_with_distance() {
        let camera = Camera::new(Vec3::ZERO, 0.0, 0.0);
        let at = |z: f32| BoundingSphere { center: Vec3::new(0.0, 0.0, -z), radius: 1.0 };
        // At the distance where the view is exactly 2 units tall, a 2-unit sphere fills it.
        let fills = 1.0 / (camera.fov_y * 0.5).tan();
        assert!((at(fills).screen_size(&camera) - 1.0).abs() < 1e-5);
        assert!((at(fills * 10.0).screen_size(&camera) - 0.1).abs() < 1e-5);
        assert_eq!(at(0.5).screen_size(&camera), f32::INFINITY);
    }

    #[test]
    fn camera_frustum_keeps_what_is_in_view() {
        // Looking down -Z from the origin.
        let camera = Camera::new(Vec3::ZERO, 0.0, 0.0);
        let frustum = Frustum::from_view_projection(camera.view_projection(16.0 / 9.0));
        let at = |x: f32, y: f32, z: f32, r: f32| BoundingSphere { center: Vec3::new(x, y, z), radius: r };

        assert!(frustum.intersects_sphere(at(0.0, 0.0, -10.0, 0.5)));
        assert!(!frustum.intersects_sphere(at(0.0, 0.0, 10.0, 0.5)), "behind");
        assert!(!frustum.intersects_sphere(at(40.0, 0.0, -10.0, 0.5)), "far to the right");
        assert!(frustum.intersects_sphere(at(40.0, 0.0, -10.0, 35.0)), "big enough to reach into view");
        assert!(!frustum.intersects_sphere(at(0.0, 0.0, -camera.far - 5.0, 1.0)), "past the far plane");
        assert!(frustum.intersects_sphere(at(0.0, 0.0, 0.5, 1.0)), "around the camera");
    }
}
