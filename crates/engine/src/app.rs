//! Main loop. Per frame:
//!
//!   hot reload ─► N fixed ticks (game.tick) ─► game.draw(alpha) ─► render
//!   ─► screenshot (F12, or frame $ENGINE_CAPTURE_FRAME)

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use base::{Vfs, Watcher};
use platform::{FixedStep, Flow, Handler, Input, Key, Window, WindowConfig};
use render::{FrameView, Renderer};

use crate::context::{Frame, Gfx, Tick};
use crate::{Config, Game};

const SCREENSHOT_KEY: Key = Key::F12;
const SCREENSHOT_DIR: &str = "screenshots";
/// Captures frame N to screenshots/frame-N.png, then exits. For
/// automated image checks, e.g. `ENGINE_CAPTURE_FRAME=120 cargo run`.
const CAPTURE_FRAME_ENV: &str = "ENGINE_CAPTURE_FRAME";

pub(crate) fn run(config: Config, game: impl Game) -> Result<()> {
    let vfs = Vfs::discover()?;
    log::info!("assets: {}", vfs.root().display());

    // Hot reload is a convenience; run without it rather than fail.
    let watcher = Watcher::new(vfs.root())
        .inspect_err(|err| log::warn!("hot reload disabled: {err:#}"))
        .ok();

    let window = WindowConfig {
        title: config.title.clone(),
        width: config.width,
        height: config.height,
    };
    let app = App {
        config,
        game,
        vfs,
        watcher,
        running: None,
        capture_frame: std::env::var(CAPTURE_FRAME_ENV)
            .ok()
            .and_then(|v| v.parse().ok()),
    };

    platform::run(window, app)
}

struct App<G> {
    config: Config,
    game: G,
    vfs: Vfs,
    watcher: Option<Watcher>,
    running: Option<Running>,
    capture_frame: Option<u64>,
}

/// State that exists only once the window is up.
struct Running {
    renderer: Renderer,
    clock: FixedStep,
    frame: Frame,
    frame_index: u64,
    /// Engine-side edge detection; tick edges are consumed by the game.
    shot_key_held: bool,
}

impl Running {
    fn save_screenshot(&self, name: &str) {
        let path = PathBuf::from(SCREENSHOT_DIR).join(format!("{name}.png"));
        let result = self
            .renderer
            .capture()
            .and_then(|shot| base::image::write_png(&path, shot.width, shot.height, &shot.rgba));

        match result {
            Ok(()) => log::info!("screenshot: {}", path.display()),
            Err(err) => log::error!("screenshot failed: {err:#}"),
        }
    }
}

impl<G: Game> App<G> {
    fn reload_assets(&mut self) {
        let (Some(watcher), Some(running)) = (&self.watcher, &mut self.running) else {
            return;
        };

        for os_path in watcher.drain() {
            let Some(path) = self.vfs.asset_path(&os_path) else {
                continue;
            };

            running.renderer.on_asset_changed(&path);
        }
    }
}

impl<G: Game> Handler for App<G> {
    fn init(&mut self, window: Window) -> Result<()> {
        let mut renderer = Renderer::new(window, self.vfs.clone(), self.config.render)?;
        self.game.init(&mut Gfx {
            renderer: &mut renderer,
        })?;

        // Clock starts after loading so load time is not simulated.
        self.running = Some(Running {
            renderer,
            clock: FixedStep::new(self.config.tick_hz),
            frame: Frame::new(),
            frame_index: 0,
            shot_key_held: false,
        });

        Ok(())
    }

    fn resized(&mut self, width: u32, height: u32) {
        if let Some(running) = &mut self.running {
            running.renderer.resize(width, height);
        }
    }

    fn frame(&mut self, input: &mut Input) -> Flow {
        self.reload_assets();

        let Some(running) = &mut self.running else {
            return Flow::Continue;
        };

        let ticks = running.clock.advance();
        let dt = running.clock.dt();

        for _ in 0..ticks {
            let mut tick = Tick {
                input,
                dt,
                quit: false,
            };
            self.game.tick(&mut tick);

            if tick.quit {
                return Flow::Exit;
            }

            input.end_tick();
        }

        running.frame.begin(running.clock.alpha());
        self.game.draw(&mut running.frame);

        running.renderer.render(&FrameView {
            camera: &running.frame.camera,
            draws: &running.frame.draws,
        });
        running.frame_index += 1;

        let shot_key = input.is_down(SCREENSHOT_KEY);
        if shot_key && !running.shot_key_held {
            running.save_screenshot(&unix_millis().to_string());
        }
        running.shot_key_held = shot_key;

        if self.capture_frame == Some(running.frame_index) {
            running.save_screenshot(&format!("frame-{}", running.frame_index));
            return Flow::Exit;
        }

        Flow::Continue
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}
