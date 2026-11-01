//! Grimora sandbox: loads `assets/scenes/sandbox.ron`, flies a debug camera
//! and animates the heart crystal on the altar.
//!
//!     cargo run -p sandbox
//!     cargo run -p sandbox -- --screenshot shots/sandbox.png
//!     cargo run -p sandbox -- --help

use grimora::prelude::*;

const DEFAULT_SCENE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/sandbox.ron");

#[derive(Default)]
struct Sandbox {
    fly: FlyCamera,
    /// Index of the "heart" object and its resting position.
    heart: Option<(usize, Vec3)>,
}

impl Game for Sandbox {
    fn scene_loaded(&mut self, ctx: &mut Context) {
        self.heart = ctx
            .world
            .objects
            .iter()
            .position(|o| o.name.as_deref() == Some("heart"))
            .map(|i| (i, ctx.world.objects[i].transform.position));
    }

    fn update(&mut self, ctx: &mut Context) {
        let looking = self.fly.update(&mut ctx.camera, &ctx.input, ctx.time.delta());
        ctx.set_cursor_grab(looking);

        if let Some((index, rest)) = self.heart {
            let t = ctx.time.elapsed();
            let heart = &mut ctx.world.objects[index];
            heart.transform.position = rest + Vec3::Y * (0.25 + 0.15 * (t * 1.3).sin());
            heart.transform.rotation = Quat::from_rotation_y(t * 0.4);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let config =
        EngineConfig { title: "Grimora Sandbox".into(), scene: Some(DEFAULT_SCENE.into()), ..Default::default() }
            .with_cli()?;
    grimora::run(config, Sandbox::default())
}
