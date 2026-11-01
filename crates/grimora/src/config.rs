use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use grimora_core::Vec3;

/// How the engine starts. Every field can also be set from the command line,
/// which is how automated tools (and AI assistants) drive the engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub msaa: u32,
    /// RON scene loaded at startup and hot-reloaded when the file changes.
    pub scene: Option<PathBuf>,
    /// Save the frame rendered at `frames` to this PNG, then exit.
    pub screenshot: Option<PathBuf>,
    /// Exit after this many frames (defaults to 90 with `screenshot`).
    pub frames: Option<u64>,
    /// Overrides the scene camera position.
    pub camera_position: Option<Vec3>,
    /// Overrides the scene camera target.
    pub look_at: Option<Vec3>,
    /// Watch the scene file and reload it on change.
    pub hot_reload: bool,
    /// Play sound (screenshot runs mix without opening a device).
    pub audio: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            title: "Grimora".into(),
            width: 1600,
            height: 900,
            vsync: true,
            msaa: 4,
            scene: None,
            screenshot: None,
            frames: None,
            camera_position: None,
            look_at: None,
            hot_reload: true,
            audio: true,
        }
    }
}

pub const USAGE: &str = "\
Options:
  --scene <file.ron>        Scene to load (hot-reloaded on save)
  --screenshot <file.png>   Render, save a PNG and exit
  --frames <n>              Exit after n frames (default 90 with --screenshot)
  --camera <x,y,z>          Override the camera position
  --look-at <x,y,z>         Override the camera target
  --size <width>x<height>   Window size, e.g. 1280x720
  --no-vsync                Uncapped frame rate (for benchmarking)
  --no-msaa                 Disable anti-aliasing
  --no-hot-reload           Do not watch the scene file
  --no-audio                No sound (screenshots are always silent)
  --help                    Show this message

Controls: hold right mouse + move to look, WASD move, E/Q up/down,
Shift fast, F5 reload scene, F12 screenshot, Esc quit.";

impl EngineConfig {
    /// Applies command line flags on top of `self`.
    pub fn with_args(mut self, args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = || args.next().with_context(|| format!("{arg} expects a value"));
            match arg.as_str() {
                "--scene" => self.scene = Some(value()?.into()),
                "--screenshot" => self.screenshot = Some(value()?.into()),
                "--frames" => self.frames = Some(value()?.parse().context("--frames expects a number")?),
                "--camera" => self.camera_position = Some(parse_vec3(&value()?)?),
                "--look-at" => self.look_at = Some(parse_vec3(&value()?)?),
                "--size" => {
                    let v = value()?;
                    let (w, h) = v.split_once('x').context("--size expects WIDTHxHEIGHT")?;
                    self.width = w.trim().parse().context("bad width")?;
                    self.height = h.trim().parse().context("bad height")?;
                }
                "--no-vsync" => self.vsync = false,
                "--no-msaa" => self.msaa = 1,
                "--no-hot-reload" => self.hot_reload = false,
                "--no-audio" => self.audio = false,
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                other => bail!("unknown option `{other}`\n\n{USAGE}"),
            }
        }
        Ok(self)
    }

    /// Applies the process's own command line (skipping the program name).
    pub fn with_cli(self) -> Result<Self> {
        self.with_args(std::env::args().skip(1))
    }

    pub(crate) fn frame_limit(&self) -> Option<u64> {
        self.frames.or(self.screenshot.as_ref().map(|_| 90))
    }
}

fn parse_vec3(s: &str) -> Result<Vec3> {
    let parts: Vec<f32> = s.split(',').map(|p| p.trim().parse::<f32>()).collect::<Result<_, _>>()?;
    match parts[..] {
        [x, y, z] => Ok(Vec3::new(x, y, z)),
        _ => bail!("expected x,y,z but got `{s}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags() {
        let args = ["--scene", "a.ron", "--screenshot", "out.png", "--camera", "1,2.5,-3", "--size", "800x600"];
        let config = EngineConfig::default().with_args(args.map(String::from)).unwrap();
        assert_eq!(config.scene.unwrap().to_str(), Some("a.ron"));
        assert_eq!(config.camera_position, Some(Vec3::new(1.0, 2.5, -3.0)));
        assert_eq!((config.width, config.height), (800, 600));
        assert_eq!(EngineConfig { screenshot: Some("x".into()), ..Default::default() }.frame_limit(), Some(90));
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(EngineConfig::default().with_args(["--nope".to_string()]).is_err());
    }
}
