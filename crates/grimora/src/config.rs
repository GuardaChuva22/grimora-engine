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
    /// Draw [`crate::Context::ui`]; off for clean scene screenshots.
    pub ui: bool,
    /// Folder for saved games; defaults to the user's data folder (see
    /// [`crate::SaveStore::for_game`], named after `title`).
    pub save_dir: Option<PathBuf>,
    /// Saved game slot for the game to load at startup (it decides how, with
    /// [`crate::Context::load_game`]).
    pub load: Option<String>,
    /// Menu for the game to open at startup, for capturing it (the game decides
    /// what the names mean).
    pub menu: Option<String>,
    /// Escape closes the engine. Games with a pause menu turn this off and read
    /// the key themselves.
    pub escape_quits: bool,
    /// Stops the scene's clock at this hour, for repeatable captures.
    pub time_of_day: Option<f32>,
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
            ui: true,
            save_dir: None,
            load: None,
            menu: None,
            escape_quits: true,
            time_of_day: None,
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
  --no-ui                   Hide the game's HUD
  --save-dir <folder>       Where saved games go
  --load <slot>             Ask the game to load this saved game at startup
  --menu <name>             Ask the game to open this menu at startup, to capture it
                            (the sandbox has title, pause, options, video and controls)
  --time <hour>             Stop the scene's day at this hour, from 0 to 24
  --help                    Show this message

Controls: hold right mouse + move to look, WASD move, E/Q up/down,
Shift fast, F5 reload scene, F12 screenshot, Esc quit (or the game's menu).";

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
                "--no-ui" => self.ui = false,
                "--no-audio" => self.audio = false,
                "--save-dir" => self.save_dir = Some(value()?.into()),
                "--load" => self.load = Some(value()?),
                "--menu" => self.menu = Some(value()?),
                "--time" => self.time_of_day = Some(value()?.parse().context("--time expects an hour")?),
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
        let config =
            EngineConfig::default().with_args(["--load", "quick", "--save-dir", "s"].map(String::from)).unwrap();
        assert_eq!((config.load.as_deref(), config.save_dir.unwrap().to_str()), (Some("quick"), Some("s")));
        let config = EngineConfig::default().with_args(["--menu", "pause"].map(String::from)).unwrap();
        assert_eq!(config.menu.as_deref(), Some("pause"));
        let config = EngineConfig::default().with_args(["--time", "21.5"].map(String::from)).unwrap();
        assert_eq!(config.time_of_day, Some(21.5));
        assert_eq!(EngineConfig { screenshot: Some("x".into()), ..Default::default() }.frame_limit(), Some(90));
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(EngineConfig::default().with_args(["--nope".to_string()]).is_err());
    }
}
