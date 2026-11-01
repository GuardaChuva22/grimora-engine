//! Collision shapes as plain data. `grimora_physics` turns them into Rapier colliders.

use grimora_core::{MeshData, Transform, Vec3};
use serde::{Deserialize, Serialize};

use crate::MeshDesc;

/// How an object collides, as written in scene files.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum ColliderDesc {
    /// A sensible shape picked from the mesh type (trunks for trees, hulls for rocks...).
    #[default]
    Auto,
    /// Walk-through: decoration only.
    None,
    /// Convex hull of the mesh.
    Hull,
    /// The exact triangles of the mesh. Accurate but slower; best for big static pieces.
    Mesh,
    /// Box given by its half sizes, centered `center` above the object's origin.
    Box {
        half_extents: Vec3,
        #[serde(default)]
        center: Vec3,
    },
    /// Upright cylinder resting on the object's origin.
    Cylinder { radius: f32, height: f32 },
    Ball {
        radius: f32,
        #[serde(default)]
        center: Vec3,
    },
}

/// A resolved collision shape in the object's local space (before its transform).
#[derive(Debug, Clone, PartialEq)]
pub enum ColliderShape {
    Cuboid {
        center: Vec3,
        half_extents: Vec3,
    },
    /// Upright (Y) cylinder.
    Cylinder {
        center: Vec3,
        radius: f32,
        half_height: f32,
    },
    Ball {
        center: Vec3,
        radius: f32,
    },
    ConvexHull {
        points: Vec<Vec3>,
    },
    TriMesh {
        vertices: Vec<Vec3>,
        indices: Vec<[u32; 3]>,
    },
}

impl ColliderDesc {
    /// Picks the concrete shape for `mesh`. `None` means no collision.
    pub fn resolve(&self, desc: &MeshDesc, mesh: &MeshData) -> Option<ColliderShape> {
        match self {
            Self::None => None,
            Self::Hull => Some(ColliderShape::hull(mesh)),
            Self::Mesh => Some(ColliderShape::trimesh(mesh)),
            Self::Box { half_extents, center } => {
                Some(ColliderShape::Cuboid { center: *center, half_extents: *half_extents })
            }
            Self::Cylinder { radius, height } => Some(ColliderShape::Cylinder {
                center: Vec3::Y * height * 0.5,
                radius: *radius,
                half_height: height * 0.5,
            }),
            Self::Ball { radius, center } => Some(ColliderShape::Ball { center: *center, radius: *radius }),
            Self::Auto => Some(match desc {
                MeshDesc::Cube => ColliderShape::Cuboid { center: Vec3::Y * 0.5, half_extents: Vec3::splat(0.5) },
                MeshDesc::Plane { size } => ColliderShape::Cuboid {
                    center: Vec3::Y * -0.05,
                    half_extents: Vec3::new(size * 0.5, 0.05, size * 0.5),
                },
                MeshDesc::Cylinder { .. } => {
                    ColliderShape::Cylinder { center: Vec3::Y * 0.5, radius: 0.5, half_height: 0.5 }
                }
                MeshDesc::Sphere { .. } => ColliderShape::Ball { center: Vec3::Y * 0.5, radius: 0.5 },
                // Trees block at the trunk and the dense lower foliage, not the branch tips.
                MeshDesc::Pine { .. } => {
                    ColliderShape::Cylinder { center: Vec3::Y * 1.5, radius: 0.35, half_height: 1.5 }
                }
                MeshDesc::DeadTree { .. } => {
                    ColliderShape::Cylinder { center: Vec3::Y * 1.2, radius: 0.22, half_height: 1.2 }
                }
                MeshDesc::Wanderer => return None,
                MeshDesc::Cone { .. }
                | MeshDesc::Rock { .. }
                | MeshDesc::Crystal { .. }
                | MeshDesc::Pillar { .. }
                | MeshDesc::Model { .. } => ColliderShape::hull(mesh),
            }),
        }
    }
}

impl ColliderShape {
    pub fn hull(mesh: &MeshData) -> Self {
        Self::ConvexHull { points: mesh.vertices.iter().map(|v| Vec3::from(v.position)).collect() }
    }

    /// Triangle mesh with duplicate (flat-shading) vertices welded back together.
    pub fn trimesh(mesh: &MeshData) -> Self {
        let mut vertices = Vec::new();
        let mut lookup = std::collections::HashMap::new();
        let mut remap = |p: [f32; 3]| {
            *lookup.entry(p.map(f32::to_bits)).or_insert_with(|| {
                vertices.push(Vec3::from(p));
                vertices.len() as u32 - 1
            })
        };
        let welded: Vec<u32> = mesh.indices.iter().map(|&i| remap(mesh.vertices[i as usize].position)).collect();
        let indices = welded.as_chunks::<3>().0.iter().map(|t| [t[0], t[1], t[2]]).collect();
        Self::TriMesh { vertices, indices }
    }

    /// Bakes `transform`'s scale into the shape, leaving only position and rotation
    /// for the collider pose. Round shapes take the largest horizontal scale.
    pub fn scaled(&self, transform: &Transform) -> Self {
        let s = transform.scale;
        let horizontal = s.x.abs().max(s.z.abs());
        match self {
            Self::Cuboid { center, half_extents } => {
                Self::Cuboid { center: *center * s, half_extents: (*half_extents * s).abs() }
            }
            Self::Cylinder { center, radius, half_height } => Self::Cylinder {
                center: *center * s,
                radius: radius * horizontal,
                half_height: half_height * s.y.abs(),
            },
            Self::Ball { center, radius } => Self::Ball { center: *center * s, radius: radius * s.abs().max_element() },
            Self::ConvexHull { points } => Self::ConvexHull { points: points.iter().map(|p| *p * s).collect() },
            Self::TriMesh { vertices, indices } => {
                Self::TriMesh { vertices: vertices.iter().map(|p| *p * s).collect(), indices: indices.clone() }
            }
        }
    }
}
