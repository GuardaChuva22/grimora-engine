//! Draws the icon of every item in `assets/items/items.ron` from its model, with the engine's
//! own renderer: a small scene shot twice, against a plain sky whose color is then taken out,
//! and against a gray one for the colors, since metal mirrors the sky. Then it's shrunk to big
//! pixels with a dark outline.
//!
//!     cargo run --release -p icon_maker              every item
//!     cargo run --release -p icon_maker -- adaga     only these
//!     cargo run --release -p icon_maker -- --pieces  the inventory's pieces, one [ItemDesc::size] of cells
//!
//! Each one is its own run of the engine, which is what the `--scene` arguments are for.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use grimora::prelude::*;
use grimora::scene::{ObjectDesc, SceneDesc};
use image::{Rgba, RgbaImage};

const ITEMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/items/items.ron");
/// Texels on a side of the finished icon.
const ICON: u32 = 48;
/// Screen pixels each of them comes from.
const SHRINK: u32 = 8;
/// Vertical field of view. Narrow, so that it's almost flat, as icons are.
const FOV: f32 = 12.0;
/// Room left around the item, as a fraction of its size. The outline goes there.
const MARGIN: f32 = 1.12;
/// How much a texel must be covered by the item to count.
const COVERED: f32 = 0.28;
/// How close a pixel must be to the sky to be sky, in each of its 0..255 channels.
const KEY_TOLERANCE: i32 = 30;
const KEY_SKY: Vec3 = Vec3::new(1.0, 0.0, 1.0);
const PLAIN_SKY: Vec3 = Vec3::new(0.62, 0.6, 0.6);
const OUTLINE: Rgba<u8> = Rgba([10, 8, 13, 255]);
/// Weapons are shown with the blade going up and to the right, and other things from above.
const WEAPON_POSE: Vec3 = Vec3::new(0.0, 16.0, -45.0);
const OBJECT_POSE: Vec3 = Vec3::new(24.0, 32.0, 0.0);

/// Texels on a side of a cell of the fitted inventory.
const CELL_ART: u32 = 24;
/// Long things stand up, showing the side their curve is on.
const UPRIGHT: Vec3 = Vec3::new(0.0, 16.0, 0.0);
/// A book seen from above, on its cover.
const COVER: Vec3 = Vec3::new(42.0, -32.0, 0.0);

/// Armor keeps the way it was made, from the front and a little to the side.
const WORN: Vec3 = Vec3::new(8.0, -24.0, 0.0);

/// Brightened or darkened, for what would come out wrong on the dark background.
fn gain_of(id: &str) -> f32 {
    match id {
        "grimorio_de_gelo" => 1.25,
        "bala_de_chumbo" => 0.5,
        _ => 1.0,
    }
}

/// How a piece of the fitted inventory is turned: long things up, and a book on its cover.
fn piece_pose(item: &ItemDesc) -> Vec3 {
    match item.kind {
        grimora::scene::ItemKind::Weapon { .. } => UPRIGHT,
        grimora::scene::ItemKind::Grimoire { .. } => COVER,
        grimora::scene::ItemKind::Armor { .. } => WORN,
        _ => OBJECT_POSE,
    }
}

/// What one picture is to look like.
struct Shot {
    pose: Vec3,
    /// Finished texels.
    size: (u32, u32),
    gain: f32,
}

struct Still;

impl Game for Still {
    fn update(&mut self, _ctx: &mut Context) {}
}

/// The scene that shows `item` against `sky`, with nothing else in the picture.
fn stage(catalog: &ItemCatalog, item: &ItemDesc, shot: &Shot, sky: Vec3) -> Result<String> {
    let path = catalog.model_path(item).context("it has no model")?;
    let pose = shot.pose;
    let turn = Transform::from_euler_degrees(Vec3::ZERO, pose, Vec3::ONE).rotation;
    let mesh = catalog.mesh(item);
    let points = mesh.vertices.iter().map(|v| turn * Vec3::from(v.position));
    let (low, high) = points.fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(low, high), p| (low.min(p), high.max(p)));
    // Anything is shot at about a meter: the camera can't get close enough to a bullet.
    let zoom = 1.0 / (high - low).max_element().max(1e-3);
    let (low, high) = (low * zoom, high * zoom);
    let (middle, size) = ((low + high) * 0.5, high - low);
    let aspect = shot.size.0 as f32 / shot.size.1 as f32;
    let half = size.y.max(size.x / aspect) * 0.5 * MARGIN;

    let mut scene = SceneDesc::default();
    scene.camera.position = Vec3::new(middle.x, middle.y, half / (FOV * 0.5).to_radians().tan() + high.z);
    scene.camera.look_at = Some(Vec3::new(middle.x, middle.y, 0.0));
    scene.camera.fov = FOV;
    let environment = &mut scene.environment;
    environment.sky.zenith = sky;
    environment.sky.horizon = sky;
    environment.sky.ground = sky;
    environment.sky.sun_size = 0.0;
    environment.sky.sun_glow = 0.0;
    environment.sky.halo = 0.0;
    environment.sky.stars = 0.0;
    environment.sky.clouds.amount = 0.0;
    environment.fog.density = 0.0;
    environment.fog.sun_scatter = 0.0;
    environment.sun.direction = Vec3::new(-0.4, 0.7, 0.8);
    environment.sun.color = Vec3::new(1.0, 0.95, 0.88);
    environment.sun.intensity = 2.6;
    environment.ambient.sky = Vec3::new(0.62, 0.62, 0.72);
    environment.ambient.ground = Vec3::new(0.38, 0.34, 0.36);
    environment.ambient.intensity = 0.9;
    environment.post.vignette = 0.0;
    environment.post.grain = 0.0;
    environment.post.bloom = 0.0;
    environment.post.contrast = 1.0;
    environment.post.saturation = 1.0;
    environment.post.exposure = 1.0;
    scene.objects.push(ObjectDesc {
        mesh: MeshDesc::Model { path: path.to_string_lossy().into_owned(), max_texture_size: 0, events: Vec::new() },
        rotation: pose,
        scale: Vec3::splat(zoom),
        snap_to_ground: false,
        collider: grimora::scene::ColliderDesc::None,
        ..Default::default()
    });
    scene.to_ron()
}

/// Takes the shape out of `keyed` and the colors out of `plain`, and shrinks it to the finished icon.
fn pixelate(keyed: &RgbaImage, plain: &RgbaImage, (width, height): (u32, u32), gain: f32) -> RgbaImage {
    let key = *keyed.get_pixel(0, 0);
    let is_sky = |pixel: &Rgba<u8>| (0..3).all(|c| (i32::from(pixel[c]) - i32::from(key[c])).abs() <= KEY_TOLERANCE);
    let mut icon = RgbaImage::new(width, height);
    let total = (SHRINK * SHRINK) as f32;
    for (x, y, out) in icon.enumerate_pixels_mut() {
        let (mut sum, mut count) = ([0u32; 3], 0u32);
        for sy in y * SHRINK..(y + 1) * SHRINK {
            for sx in x * SHRINK..(x + 1) * SHRINK {
                if !is_sky(keyed.get_pixel(sx, sy)) {
                    let pixel = plain.get_pixel(sx, sy);
                    (0..3).for_each(|c| sum[c] += u32::from(pixel[c]));
                    count += 1;
                }
            }
        }
        if count as f32 / total >= COVERED {
            let channel = |sum: u32| ((sum / count) as f32 * gain).min(255.0) as u8;
            *out = Rgba([channel(sum[0]), channel(sum[1]), channel(sum[2]), 255]);
        }
    }
    let shape = icon.clone();
    for (x, y, out) in icon.enumerate_pixels_mut() {
        let filled = |dx: i32, dy: i32| {
            let (x, y) = (x as i32 + dx, y as i32 + dy);
            (0..width as i32).contains(&x)
                && (0..height as i32).contains(&y)
                && shape.get_pixel(x as u32, y as u32)[3] > 0
        };
        if out[3] == 0 && [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().any(|(dx, dy)| filled(dx, dy)) {
            *out = OUTLINE;
        }
    }
    icon
}

/// A screenshot of `item` against `sky`.
fn shoot(catalog: &ItemCatalog, item: &ItemDesc, shot: &Shot, sky: Vec3, work: &Path) -> Result<RgbaImage> {
    let scene_path = work.join(format!("{}.ron", item.id));
    let shot_path = work.join(format!("{}.png", item.id));
    std::fs::write(&scene_path, stage(catalog, item, shot, sky)?)?;
    let status = Command::new(std::env::current_exe()?)
        .args(["--scene".as_ref(), scene_path.as_os_str()])
        .args(["--screenshot".as_ref(), shot_path.as_os_str()])
        .args(["--size", &format!("{}x{}", shot.size.0 * SHRINK, shot.size.1 * SHRINK), "--no-ui"])
        .stdout(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("the engine stopped with {status}");
    }
    Ok(image::open(&shot_path).with_context(|| format!("reading {}", shot_path.display()))?.to_rgba8())
}

fn make_icon(catalog: &ItemCatalog, item: &ItemDesc, shot: &Shot, path: &Path, work: &Path) -> Result<()> {
    let keyed = shoot(catalog, item, shot, KEY_SKY, work)?;
    let plain = shoot(catalog, item, shot, PLAIN_SKY, work)?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    pixelate(&keyed, &plain, shot.size, shot.gain).save(path).with_context(|| format!("writing {}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--scene") {
        return grimora::run(EngineConfig::default().with_cli()?, Still);
    }
    let catalog = ItemCatalog::load(ITEMS)?;
    let work = std::env::temp_dir().join(format!("grimora_icons_{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    let mut failed = false;
    let pieces = args.iter().any(|arg| arg == "--pieces");
    let names: Vec<&String> = args.iter().filter(|arg| !arg.starts_with("--")).collect();
    let wanted = |item: &&ItemDesc| names.is_empty() || names.contains(&&item.id);
    if pieces {
        for item in catalog.iter().filter(wanted).filter(|item| item.model.is_some()) {
            let (across, down) = catalog.cells(&item.id);
            let size = (across * CELL_ART, down * CELL_ART);
            let shot = Shot { pose: piece_pose(item), size, gain: gain_of(&item.id) };
            if let Err(err) = make_icon(&catalog, item, &shot, &catalog.piece_path(item), &work) {
                eprintln!("{}: {err:#}", item.id);
                failed = true;
            }
        }
    }
    for item in catalog.iter().filter(wanted).filter(|_| !pieces) {
        let weapon = matches!(item.kind, grimora::scene::ItemKind::Weapon { .. });
        let gain = gain_of(&item.id);
        let armor = matches!(item.kind, grimora::scene::ItemKind::Armor { .. });
        let pose = if weapon {
            WEAPON_POSE
        } else if armor {
            WORN
        } else {
            OBJECT_POSE
        };
        let shot = Shot { pose, size: (ICON, ICON), gain };
        if let Err(err) = make_icon(&catalog, item, &shot, &catalog.icon_path(item), &work) {
            eprintln!("{}: {err:#}", item.id);
            failed = true;
        }
    }
    std::fs::remove_dir_all(&work)?;
    if failed {
        bail!("some icons couldn't be made");
    }
    Ok(())
}
