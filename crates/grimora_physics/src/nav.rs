//! Finding a way across the world. There is no baked navigation mesh: searches
//! probe the collision world as it is right now, so scenes need no preparation
//! and streamed objects and moved crates simply count. The grid has one floor
//! per cell, so a path can't choose between a bridge and what is under it.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use grimora_core::Vec3;
use rapier3d::math::Pose;
use rapier3d::parry::query::Ray;
use rapier3d::parry::shape::Capsule;

use crate::{Physics, scenery, to_rapier};

/// Side of a search cell in meters: narrower than a person, wide enough for speed.
const CELL: f32 = 0.5;
/// Ground checks one search may make, which bounds its cost: past this it
/// settles for the closest place found.
const MAX_PROBES: usize = 1000;
/// Extra checks for straightening the path afterwards.
const SMOOTHING_PROBES: usize = 400;
/// A goal inside something solid moves up to this many cells away to find floor.
const GOAL_RINGS: i32 = 6;
/// Above 1, the search heads for the goal instead of exploring evenly: slightly
/// longer paths, far fewer checks. Smoothing takes the difference back.
const GREED: f32 = 1.3;

/// Who is walking: what fits and what can be climbed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavShape {
    pub radius: f32,
    pub height: f32,
    /// Tallest ledge it climbs (`CharacterBody` climbs 0.4 m).
    pub step: f32,
    /// Steepest slope it walks, in radians (`CharacterBody` starts sliding at 40°).
    pub max_slope: f32,
}

impl NavShape {
    pub fn new(radius: f32, height: f32) -> Self {
        Self { radius, height, step: 0.4, max_slope: 40f32.to_radians() }
    }
}

/// A way found by [`Physics::find_path`].
#[derive(Debug, Clone, PartialEq)]
pub struct NavPath {
    /// Places to walk through in order, on the ground, without the start.
    pub points: Vec<Vec3>,
    /// False when this only gets as close as could be found.
    pub complete: bool,
}

impl Physics {
    /// The way from `from` (feet) to `to` on foot, around static geometry and
    /// dynamic bodies but ignoring agents, which move on their own. A goal inside
    /// something solid means the nearest floor. Costs up to a few milliseconds
    /// when the goal is hard to reach, so don't search every frame.
    pub fn find_path(&self, from: Vec3, to: Vec3, shape: &NavShape) -> NavPath {
        Search::new(self, from, shape).run(to)
    }
}

/// Standable ground.
#[derive(Debug, Clone, Copy)]
struct Ground {
    y: f32,
    /// Tangent of its slope.
    slope: f32,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    x: f32,
    z: f32,
    ground: Ground,
    parent: u32,
    cost: f32,
}

impl Node {
    fn point(&self) -> Vec3 {
        Vec3::new(self.x, self.ground.y, self.z)
    }

    fn distance(&self, goal: Vec3) -> f32 {
        (self.x - goal.x).hypot(self.z - goal.z)
    }
}

/// A node waiting in the open list, cheapest first.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Open {
    priority: f32,
    node: u32,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.priority.total_cmp(&self.priority).then(other.node.cmp(&self.node))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Search<'a> {
    physics: &'a Physics,
    from: Vec3,
    shape: NavShape,
    /// Vertical reach of one probe: the steepest change between neighbors.
    reach: f32,
    min_normal_y: f32,
    capsule: Capsule,
    capsule_center: f32,
    probes: usize,
    nodes: Vec<Node>,
    /// Cell to node, `None` where nobody can stand.
    cells: HashMap<(i32, i32), Option<u32>>,
}

impl<'a> Search<'a> {
    fn new(physics: &'a Physics, from: Vec3, shape: &NavShape) -> Self {
        // The body clears whatever it can step over, so small stones don't block.
        let clearance = (shape.height - shape.step).max(shape.radius * 2.0);
        let half_segment = (clearance * 0.5 - shape.radius).max(0.01);
        Self {
            physics,
            from,
            shape: *shape,
            reach: shape.step + CELL * std::f32::consts::SQRT_2 * shape.max_slope.tan(),
            min_normal_y: shape.max_slope.cos(),
            capsule: Capsule::new_y(half_segment, shape.radius),
            capsule_center: shape.step + clearance * 0.5,
            probes: 0,
            nodes: Vec::new(),
            cells: HashMap::new(),
        }
    }

    /// The ground at `(x, z)` if the walker fits there, within reach of height `near`.
    fn probe(&mut self, x: f32, z: f32, near: f32) -> Option<Ground> {
        self.probes += 1;
        let top = near + self.reach;
        self.standing_room(x, z, top, self.reach * 2.0)
    }

    fn standing_room(&self, x: f32, z: f32, top: f32, depth: f32) -> Option<Ground> {
        let world = &self.physics.world;
        let ray = Ray::new(to_rapier(Vec3::new(x, top, z)), to_rapier(Vec3::NEG_Y));
        let (_, hit) = world.cast_ray_and_get_normal(&ray, depth, true, scenery())?;
        // Starting inside something, or standing on what is too steep.
        if hit.time_of_impact < 1e-3 || hit.normal.y < self.min_normal_y {
            return None;
        }
        let y = top - hit.time_of_impact;
        let pose = Pose::from_translation(to_rapier(Vec3::new(x, y + self.capsule_center, z)));
        let queries = world.query_pipeline_with_filter(scenery());
        if queries.intersect_shape(pose, &self.capsule).next().is_some() {
            return None;
        }
        Some(Ground { y, slope: (1.0 - hit.normal.y * hit.normal.y).max(0.0).sqrt() / hit.normal.y.max(1e-3) })
    }

    /// A ledge is only as tall as `step`; beyond that the change must come from a slope.
    fn can_cross(&self, a: Ground, b: Ground, distance: f32) -> bool {
        (b.y - a.y).abs() <= self.shape.step + distance * a.slope.max(b.slope)
    }

    /// Follows the ground straight from `start` to `to`; true if nothing stops it.
    fn walk(&mut self, start: Vec3, ground: Ground, to: Vec3, budget: usize) -> bool {
        let span = Vec3::new(to.x - start.x, 0.0, to.z - start.z);
        let steps = (span.length() / CELL).ceil().max(1.0) as u32;
        let stride = span.length() / steps as f32;
        let mut last = ground;
        for i in 1..=steps {
            if self.probes >= budget {
                return false;
            }
            let at = start + span * (i as f32 / steps as f32);
            let Some(next) = self.probe(at.x, at.z, last.y) else { return false };
            if !self.can_cross(last, next, stride) {
                return false;
            }
            last = next;
        }
        true
    }

    /// Where to aim for `to`: the floor there, or the nearest one around it.
    fn goal(&self, to: Vec3) -> Option<Vec3> {
        // The top of what the goal is inside of doesn't count, only floor at its level.
        let floor = |x: f32, z: f32| {
            let ground = self.standing_room(x, z, to.y + 2.5, 6.0).filter(|g| g.y <= to.y + self.shape.step)?;
            Some(Vec3::new(x, ground.y, z))
        };
        floor(to.x, to.z).or_else(|| {
            (1..=GOAL_RINGS).find_map(|ring| {
                let radius = ring as f32 * CELL;
                (0..8 + ring * 2).find_map(|i| {
                    let (sin, cos) = (i as f32 / (8 + ring * 2) as f32 * std::f32::consts::TAU).sin_cos();
                    floor(to.x + cos * radius, to.z + sin * radius)
                })
            })
        })
    }

    /// The node of a cell, probing it the first time, `None` if nobody can stand there.
    fn cell(&mut self, i: i32, j: i32, near: f32) -> Option<u32> {
        if let Some(known) = self.cells.get(&(i, j)) {
            return *known;
        }
        let (x, z) = (self.from.x + i as f32 * CELL, self.from.z + j as f32 * CELL);
        let found = self.probe(x, z, near).map(|ground| {
            self.nodes.push(Node { x, z, ground, parent: u32::MAX, cost: f32::INFINITY });
            self.nodes.len() as u32 - 1
        });
        self.cells.insert((i, j), found);
        found
    }

    fn run(mut self, to: Vec3) -> NavPath {
        // Standing somewhere tight is no reason to give up: only leaving has to work.
        let ground = self.standing_room(self.from.x, self.from.z, self.from.y + self.reach, self.reach * 2.0);
        let start = ground.unwrap_or(Ground { y: self.from.y, slope: 0.0 });
        let goal = self.goal(to);
        let aim = goal.unwrap_or(to);

        if goal.is_some() && self.walk(self.from, start, aim, MAX_PROBES) {
            return NavPath { points: vec![aim], complete: true };
        }

        self.nodes.push(Node { x: self.from.x, z: self.from.z, ground: start, parent: u32::MAX, cost: 0.0 });
        self.cells.insert((0, 0), Some(0));
        let mut open = BinaryHeap::new();
        open.push(Open { priority: 0.0, node: 0 });
        let mut closest = (self.nodes[0].distance(aim), 0);
        let mut reached = false;

        while let Some(Open { node: id, .. }) = open.pop() {
            let node = self.nodes[id as usize];
            let left = node.distance(aim);
            if left < closest.0 {
                closest = (left, id);
            }
            if goal.is_some() && left <= CELL * 1.5 && self.walk(node.point(), node.ground, aim, MAX_PROBES + 8) {
                closest = (left, id);
                reached = true;
                break;
            }
            if self.probes >= MAX_PROBES {
                break;
            }
            let i = ((node.x - self.from.x) / CELL).round() as i32;
            let j = ((node.z - self.from.z) / CELL).round() as i32;
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let diagonal = di != 0 && dj != 0;
                // No cutting corners: both cells beside a diagonal step must be free.
                if diagonal
                    && (self.cell(i + di, j, node.ground.y).is_none() || self.cell(i, j + dj, node.ground.y).is_none())
                {
                    continue;
                }
                let Some(next) = self.cell(i + di, j + dj, node.ground.y) else { continue };
                let stride = if diagonal { CELL * std::f32::consts::SQRT_2 } else { CELL };
                let other = self.nodes[next as usize];
                if !self.can_cross(node.ground, other.ground, stride) {
                    continue;
                }
                let cost = node.cost + stride + (other.ground.y - node.ground.y).abs();
                if cost < other.cost {
                    let other = &mut self.nodes[next as usize];
                    other.cost = cost;
                    other.parent = id;
                    let priority = cost + other.distance(aim) * GREED;
                    open.push(Open { priority, node: next });
                }
            }
        }

        let mut cells = Vec::new();
        let mut at = closest.1;
        while at != u32::MAX {
            cells.push(self.nodes[at as usize]);
            at = self.nodes[at as usize].parent;
        }
        cells.reverse();
        if reached {
            let last = cells[cells.len() - 1];
            cells.push(Node { x: aim.x, z: aim.z, ground: Ground { y: aim.y, ..last.ground }, ..last });
        }
        NavPath { points: self.straighten(&cells), complete: reached }
    }

    /// Skips every cell that a straight walk can do without.
    fn straighten(&mut self, cells: &[Node]) -> Vec<Vec3> {
        let budget = self.probes + SMOOTHING_PROBES;
        let mut points = Vec::new();
        let mut at = 0;
        while at + 1 < cells.len() {
            // Bisects for the farthest cell in a straight line from this one.
            let (mut near, mut far) = (at + 1, cells.len() - 1);
            while near < far {
                let middle = (near + far).div_ceil(2);
                if self.walk(cells[at].point(), cells[at].ground, cells[middle].point(), budget) {
                    near = middle;
                } else {
                    far = middle - 1;
                }
            }
            points.push(cells[near].point());
            at = near;
        }
        points
    }
}

#[cfg(test)]
mod tests {
    use grimora_core::Transform;
    use grimora_scene::ColliderShape;

    use super::*;

    const WALKER: NavShape =
        NavShape { radius: 0.35, height: 1.8, step: 0.4, max_slope: 40.0 * std::f32::consts::PI / 180.0 };

    /// A 60×60 floor with its top at y = 0, plus the given boxes (center, half extents).
    fn world(boxes: &[(Vec3, Vec3)]) -> Physics {
        let mut physics = Physics::new();
        let floor = ColliderShape::Cuboid { center: Vec3::Y * -0.5, half_extents: Vec3::new(30.0, 0.5, 30.0) };
        physics.add_static(&floor, &Transform::IDENTITY);
        for &(center, half_extents) in boxes {
            physics.add_static(&ColliderShape::Cuboid { center, half_extents }, &Transform::IDENTITY);
        }
        physics.refresh();
        physics
    }

    /// Whether a straight line between path points passes through a box on the ground plane.
    fn crosses(path: &[Vec3], from: Vec3, center: Vec3, half: Vec3) -> bool {
        let mut last = from;
        for &point in path {
            for i in 0..=100 {
                let p = last.lerp(point, i as f32 / 100.0);
                if (p.x - center.x).abs() < half.x && (p.z - center.z).abs() < half.z {
                    return true;
                }
            }
            last = point;
        }
        false
    }

    #[test]
    fn open_ground_is_one_straight_line() {
        let physics = world(&[]);
        let path = physics.find_path(Vec3::ZERO, Vec3::new(10.0, 0.0, -6.0), &WALKER);
        assert!(path.complete);
        assert_eq!(path.points.len(), 1, "{:?}", path.points);
        assert!(path.points[0].distance(Vec3::new(10.0, 0.0, -6.0)) < 0.01);
    }

    #[test]
    fn goes_around_a_wall() {
        let (center, half) = (Vec3::new(0.0, 1.0, -5.0), Vec3::new(6.0, 1.0, 0.25));
        let physics = world(&[(center, half)]);
        let to = Vec3::new(0.0, 0.0, -10.0);
        let path = physics.find_path(Vec3::ZERO, to, &WALKER);
        assert!(path.complete, "{path:?}");
        assert!(!crosses(&path.points, Vec3::ZERO, center, half), "{:?}", path.points);
        assert!(path.points.last().unwrap().distance(to) < 0.01);
        // Straightened: a corner or two, not a staircase of cells.
        assert!(path.points.len() <= 5, "{:?}", path.points);
    }

    #[test]
    fn fits_through_a_door_but_not_a_crack() {
        // A wall with a gap at x = 4: wide enough, then too narrow for the shoulders.
        for (gap, passes) in [(1.2, true), (0.4, false)] {
            let half = 0.5 * gap;
            // Both reach the edges of the floor, so the door is the only way through.
            let (left_edge, right_edge) = (4.0 - half, 4.0 + half);
            let left = (Vec3::new((left_edge - 30.0) * 0.5, 1.0, -5.0), Vec3::new((left_edge + 30.0) * 0.5, 1.0, 0.25));
            let right =
                (Vec3::new((right_edge + 30.0) * 0.5, 1.0, -5.0), Vec3::new((30.0 - right_edge) * 0.5, 1.0, 0.25));
            let physics = world(&[left, right]);
            let to = Vec3::new(0.0, 0.0, -9.0);
            let path = physics.find_path(Vec3::ZERO, to, &WALKER);
            assert_eq!(path.complete, passes, "gap {gap}: {path:?}");
        }
    }

    #[test]
    fn a_goal_inside_a_box_means_the_floor_beside_it() {
        let (center, half) = (Vec3::new(0.0, 1.0, -6.0), Vec3::new(1.0, 1.0, 1.0));
        let physics = world(&[(center, half)]);
        let path = physics.find_path(Vec3::ZERO, center, &WALKER);
        assert!(path.complete, "{path:?}");
        let end = *path.points.last().unwrap();
        assert!(!crosses(&[end], end, center, half), "{end}");
        assert!(end.distance(center) < 3.0, "{end}");
    }

    #[test]
    fn a_closed_room_gets_as_close_as_it_can() {
        let walls = [
            (Vec3::new(0.0, 2.0, -8.0), Vec3::new(3.0, 2.0, 0.25)),
            (Vec3::new(0.0, 2.0, -14.0), Vec3::new(3.0, 2.0, 0.25)),
            (Vec3::new(-3.0, 2.0, -11.0), Vec3::new(0.25, 2.0, 3.25)),
            (Vec3::new(3.0, 2.0, -11.0), Vec3::new(0.25, 2.0, 3.25)),
        ];
        let physics = world(&walls);
        let path = physics.find_path(Vec3::ZERO, Vec3::new(0.0, 0.0, -11.0), &WALKER);
        assert!(!path.complete, "{path:?}");
        let end = *path.points.last().expect("it still walks towards the room");
        assert!(end.distance(Vec3::new(0.0, 0.0, -11.0)) < 5.0, "{end}");
    }

    #[test]
    fn climbs_steps_but_not_walls_and_never_jumps_off_ledges() {
        // A low step on the way: straight over it.
        let step = (Vec3::new(0.0, 0.15, -4.0), Vec3::new(3.0, 0.15, 1.0));
        let path = world(&[step]).find_path(Vec3::ZERO, Vec3::new(0.0, 0.0, -8.0), &WALKER);
        assert_eq!(path.points.len(), 1, "{:?}", path.points);

        // A block too tall to step on, as wide as the floor: no way across.
        let block = (Vec3::new(0.0, 0.5, -4.0), Vec3::new(30.0, 0.5, 1.0));
        let path = world(&[block]).find_path(Vec3::ZERO, Vec3::new(0.0, 0.0, -8.0), &WALKER);
        assert!(!path.complete, "{path:?}");

        // Up on a platform, with nothing leading down: stays up.
        let platform = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(3.0, 1.0, 3.0));
        let path = world(&[platform]).find_path(Vec3::Y * 2.0, Vec3::new(10.0, 0.0, 0.0), &WALKER);
        assert!(!path.complete, "{path:?}");
        assert!(path.points.iter().all(|p| p.y > 1.9), "{:?}", path.points);
    }

    #[test]
    fn ignores_agents_and_counts_dynamic_bodies() {
        let mut physics = world(&[]);
        let agent = physics.add_agent(Vec3::new(0.0, 0.0, -4.0), 0.35, 1.8);
        physics.refresh();
        let to = Vec3::new(0.0, 0.0, -8.0);
        assert_eq!(physics.find_path(Vec3::ZERO, to, &WALKER).points.len(), 1, "agents get out of the way");
        physics.remove(agent);

        let wide = ColliderShape::Cuboid { center: Vec3::Y, half_extents: Vec3::new(2.0, 1.0, 0.5) };
        let body = grimora_scene::BodyDesc::Dynamic {
            density: 500.0,
            mass: None,
            friction: 0.6,
            restitution: 0.0,
            angular_damping: 0.5,
        };
        physics.add_body(&wide, &Transform::from_position(Vec3::new(0.0, 0.0, -4.0)), &body).unwrap();
        physics.refresh();
        let path = physics.find_path(Vec3::ZERO, to, &WALKER);
        assert!(path.complete);
        assert!(path.points.len() > 1, "around the crate: {:?}", path.points);
    }

    #[test]
    fn cost_is_bounded_and_the_same_every_time() {
        // A wall too long to get around within the budget.
        let walls = [(Vec3::new(0.0, 1.0, -5.0), Vec3::new(29.0, 1.0, 0.25))];
        let physics = world(&walls);
        let to = Vec3::new(0.0, 0.0, -10.0);
        let path = physics.find_path(Vec3::ZERO, to, &WALKER);
        assert_eq!(path, physics.find_path(Vec3::ZERO, to, &WALKER));
        let start = std::time::Instant::now();
        physics.find_path(Vec3::ZERO, to, &WALKER);
        let elapsed = start.elapsed();
        println!("worst case: {elapsed:?}, {} points", path.points.len());
        assert!(elapsed.as_millis() < 50, "{elapsed:?}");
    }
}
