use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use glam::DVec2;
use grimora_core::WorldPos;
use serde::{Deserialize, Serialize};

use crate::{CoastDesc, MapWindow, WorldDesc, world_to_map};

/// A grid texel with no category (open sea in the climate layer, no nation, dry land).
const NONE: u8 = 255;
/// How far, in texels, a missing biome or water body is looked for around a point that the
/// coast mask turned from sea into land, or back.
const SEARCH_TEXELS: i64 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerKind {
    Topography,
    Climate,
    Political,
    Water,
}

const KINDS: [LayerKind; 4] = [LayerKind::Topography, LayerKind::Climate, LayerKind::Political, LayerKind::Water];

/// `legend.ron`: the map's size and, for each layer, what each index means (its place in
/// the list).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Legend {
    pub width: u32,
    pub height: u32,
    pub km_per_unit: f64,
    pub layers: Vec<LayerFile>,
    pub bands: Vec<Band>,
    pub biomes: Vec<Biome>,
    pub nations: Vec<Nation>,
    pub waters: Vec<WaterBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerFile {
    pub kind: LayerKind,
    pub file: PathBuf,
    /// Map units per texel.
    pub scale: u32,
}

/// An altitude band of the topography layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub id: String,
    pub name: String,
    pub color: [u8; 3],
    pub land: bool,
    /// Typical height in meters; negative is depth below the sea.
    pub elevation: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Biome {
    pub id: String,
    pub name: String,
    pub color: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nation {
    pub id: String,
    pub name: String,
    pub color: [u8; 3],
}

/// A named body of water: ocean, sea, bay, river, lake...
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterBody {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub color: [u8; 3],
}

impl WaterBody {
    /// Rivers and lakes lie on land; everything else is sea.
    pub fn inland(&self) -> bool {
        self.kind == "rio" || self.kind == "lago"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointGroup {
    Place,
    Mineral,
    Resource,
}

/// A marked point of the compendium, in map units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapPoint {
    pub group: PointGroup,
    pub kind: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
}

impl MapPoint {
    pub fn position(&self) -> DVec2 {
        DVec2::new(self.x, self.y)
    }
}

/// One layer's grid of category indices.
#[derive(Debug, Clone)]
pub struct Layer {
    pub kind: LayerKind,
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    data: Vec<u8>,
}

impl Layer {
    fn load(kind: LayerKind, path: &Path, scale: u32) -> Result<Self> {
        let img = image::open(path).with_context(|| format!("reading {}", path.display()))?.into_luma8();
        let (width, height) = img.dimensions();
        Ok(Self { kind, width, height, scale, data: img.into_raw() })
    }

    /// The texel under a map point, if inside the grid.
    fn texel(&self, map: DVec2) -> Option<(i64, i64)> {
        let s = f64::from(self.scale);
        let (tx, ty) = ((map.x / s).floor() as i64, (map.y / s).floor() as i64);
        (tx >= 0 && ty >= 0 && tx < i64::from(self.width) && ty < i64::from(self.height)).then_some((tx, ty))
    }

    fn raw(&self, tx: i64, ty: i64) -> Option<u8> {
        if tx < 0 || ty < 0 || tx >= i64::from(self.width) || ty >= i64::from(self.height) {
            return None;
        }
        let v = self.data[(ty * i64::from(self.width) + tx) as usize];
        (v != NONE).then_some(v)
    }

    /// The category index of the nearest texel, `None` where it has none.
    pub fn index_at(&self, map: DVec2) -> Option<u8> {
        let (tx, ty) = self.texel(map)?;
        self.raw(tx, ty)
    }

    /// The category of the texel nearest `map` (by its centre) within `radius` texels that
    /// `keep` accepts. Ties go to the first in reading order, so the answer never depends on
    /// anything but the grid.
    fn nearest(&self, map: DVec2, radius: i64, keep: impl Fn(u8) -> bool) -> Option<u8> {
        let s = f64::from(self.scale);
        let (cx, cy) = ((map.x / s).floor() as i64, (map.y / s).floor() as i64);
        let mut best: Option<(f64, u8)> = None;
        for ty in cy - radius..=cy + radius {
            for tx in cx - radius..=cx + radius {
                let Some(v) = self.raw(tx, ty).filter(|&v| keep(v)) else { continue };
                let (dx, dy) = ((tx as f64 + 0.5) * s - map.x, (ty as f64 + 0.5) * s - map.y);
                let d = dx * dx + dy * dy;
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, v));
                }
            }
        }
        best.map(|(_, v)| v)
    }
}

struct Coast {
    x: f64,
    y: f64,
    texel: f64,
    width: u32,
    height: u32,
    land: Vec<bool>,
}

impl Coast {
    fn load(desc: &CoastDesc, dir: &Path) -> Result<Self> {
        let path = dir.join(&desc.file);
        let img = image::open(&path).with_context(|| format!("reading {}", path.display()))?.into_luma8();
        if desc.texel <= 0.0 {
            bail!("{}: texel must be above 0", path.display());
        }
        let (width, height) = img.dimensions();
        let land = img.into_raw().into_iter().map(|v| v != 0).collect();
        Ok(Self { x: desc.x, y: desc.y, texel: desc.texel, width, height, land })
    }

    fn land_at(&self, map: DVec2) -> Option<bool> {
        let tx = ((map.x - self.x) / self.texel).floor() as i64;
        let ty = ((map.y - self.y) / self.texel).floor() as i64;
        if tx < 0 || ty < 0 || tx >= i64::from(self.width) || ty >= i64::from(self.height) {
            return None;
        }
        Some(self.land[(ty * i64::from(self.width) + tx) as usize])
    }
}

/// What the map says at one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapSample<'a> {
    /// Land or water: the coast mask where there is one, else the topography.
    pub land: bool,
    /// Always on the same side as `land`: where the coast mask disagrees with the topography,
    /// the band is the nearest one across (the lowest land, or the coastal sea).
    pub band: Option<&'a Band>,
    /// Only on land.
    pub biome: Option<&'a Biome>,
    /// Only on land.
    pub nation: Option<&'a Nation>,
    /// The sea or the river or lake here.
    pub water: Option<&'a WaterBody>,
}

/// The macro map of Nyraldar, loaded from `assets/world/`.
pub struct MacroMap {
    pub legend: Legend,
    pub desc: WorldDesc,
    /// The world file's folder, which `desc`'s paths are relative to.
    pub dir: PathBuf,
    layers: Vec<Layer>,
    coasts: Vec<Coast>,
    pub points: Vec<MapPoint>,
}

impl MacroMap {
    /// Loads the world a [`WorldDesc`] `.ron` names: its map, layers and coasts.
    pub fn load(world: impl AsRef<Path>) -> Result<Self> {
        let (desc, dir) = WorldDesc::load(world)?;
        let mut map = Self::open(dir.join(&desc.map), &desc.layers)?;
        map.coasts = desc.coasts.iter().map(|c| Coast::load(c, &dir)).collect::<Result<_>>()?;
        map.desc = desc;
        map.dir = dir;
        Ok(map)
    }

    /// Loads a map's `legend.ron` and the layers in `kinds` (all of them when empty), with no
    /// world around it: no window and no coasts.
    pub fn open(legend: impl AsRef<Path>, kinds: &[LayerKind]) -> Result<Self> {
        let path = legend.as_ref();
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let legend: Legend = ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut layers = Vec::new();
        for file in &legend.layers {
            if kinds.is_empty() || kinds.contains(&file.kind) {
                layers.push(Layer::load(file.kind, &dir.join(&file.file), file.scale)?);
            }
        }
        let points_path = dir.join("points.ron");
        let points = match std::fs::read_to_string(&points_path) {
            Ok(text) => ron::from_str(&text).with_context(|| format!("parsing {}", points_path.display()))?,
            Err(_) => Vec::new(),
        };
        Ok(Self { legend, desc: WorldDesc::default(), dir: dir.to_path_buf(), layers, coasts: Vec::new(), points })
    }

    pub fn layer(&self, kind: LayerKind) -> Option<&Layer> {
        self.layers.iter().find(|l| l.kind == kind)
    }

    pub fn window(&self) -> MapWindow {
        self.desc.window
    }

    /// What the map says under a world position.
    pub fn sample(&self, world: WorldPos) -> MapSample<'_> {
        self.sample_map(world_to_map(world))
    }

    /// What the map says at a map point (x east, y south, in map units).
    pub fn sample_map(&self, p: DVec2) -> MapSample<'_> {
        let legend = &self.legend;
        let index = |kind| self.layer(kind).and_then(|l| l.index_at(p));
        let (land, band, moved) = self.land_and_band(p);
        let biome = if land {
            let found = index(LayerKind::Climate).or_else(|| {
                moved.then(|| self.layer(LayerKind::Climate)?.nearest(p, SEARCH_TEXELS, |_| true)).flatten()
            });
            found.and_then(|i| legend.biomes.get(usize::from(i)))
        } else {
            None
        };
        let nation =
            if land { index(LayerKind::Political).and_then(|i| legend.nations.get(usize::from(i))) } else { None };
        let body = |i: u8| legend.waters.get(usize::from(i));
        let water = match index(LayerKind::Water).and_then(body) {
            // The coast mask made this land: only a river or lake stays.
            Some(w) if land && moved && !w.inland() => None,
            None if !land && moved => self
                .layer(LayerKind::Water)
                .and_then(|l| l.nearest(p, SEARCH_TEXELS, |i| body(i).is_some_and(|w| !w.inland())))
                .and_then(body),
            other => other,
        };
        MapSample { land, band, biome, nation, water }
    }

    /// Only the land and the altitude band at a map point, as [`MacroMap::sample_map`] says them,
    /// without the lookups for biome, nation and water: what the relief asks at every vertex.
    pub fn band_at(&self, p: DVec2) -> (bool, Option<&Band>) {
        let (land, band, _) = self.land_and_band(p);
        (land, band)
    }

    /// Land, band, and whether a coast mask decided the land.
    fn land_and_band(&self, p: DVec2) -> (bool, Option<&Band>, bool) {
        let legend = &self.legend;
        let band = self
            .layer(LayerKind::Topography)
            .and_then(|l| l.index_at(p))
            .and_then(|i| legend.bands.get(usize::from(i)));
        let coast = self.coasts.iter().find_map(|c| c.land_at(p));
        let land = coast.or(band.map(|b| b.land)).unwrap_or(false);
        let band = match band {
            Some(b) if b.land != land => legend
                .bands
                .iter()
                .filter(|b| b.land == land)
                .min_by(|a, b| a.elevation.abs().total_cmp(&b.elevation.abs())),
            other => other,
        };
        (land, band, coast.is_some())
    }

    /// The points within `radius` meters of a world position, nearest first.
    pub fn points_near(&self, world: WorldPos, radius: f64) -> Vec<(&MapPoint, f64)> {
        self.points_near_map(world_to_map(world), radius / crate::METERS_PER_UNIT)
            .into_iter()
            .map(|(p, d)| (p, d * crate::METERS_PER_UNIT))
            .collect()
    }

    /// The points within `radius` map units of a map point, nearest first (then by name).
    pub fn points_near_map(&self, at: DVec2, radius: f64) -> Vec<(&MapPoint, f64)> {
        let mut near: Vec<_> =
            self.points.iter().map(|p| (p, p.position().distance(at))).filter(|&(_, d)| d <= radius).collect();
        near.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.name.cmp(&b.0.name)));
        near
    }

    /// Every layer kind, in a fixed order.
    pub fn kinds() -> [LayerKind; 4] {
        KINDS
    }
}
