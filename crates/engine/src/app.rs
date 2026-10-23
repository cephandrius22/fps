//! Main loop. Per frame:
//!
//!   hot reload ─► N fixed ticks (game.tick) ─► game.draw(alpha) ─► render
//!   ─► debug captures (F12, $ENGINE_CAPTURE_FRAME)

use anyhow::Result;
use base::{Vfs, Watcher};
use platform::{Event, FixedStep, Flow, Handler, Window, WindowConfig};
use render::Renderer;

use crate::assets::{Asset, Registry, shader_path};
use crate::debug::{self, Latch, SCREENSHOT_KEY};
use crate::gfx::{Frame, Gfx};
use crate::input::Input;
use crate::{Config, Game, sim};

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
        registry: Registry::with_shaders(),
        input: Input::default(),
        capture_frame: debug::capture_frame(),
        running: None,
    };

    platform::run(window, app)
}

struct App<G> {
    config: Config,
    game: G,
    vfs: Vfs,
    watcher: Option<Watcher>,
    registry: Registry,
    input: Input,
    capture_frame: Option<u64>,
    running: Option<Running>,
}

/// State that exists only once the window is up.
struct Running {
    renderer: Renderer,
    clock: FixedStep,
    frame: Frame,
    frame_index: u64,
    screenshot: Latch,
}

impl<G: Game> App<G> {
    /// Forwards changed files to whichever subsystem owns them.
    fn reload_assets(&mut self) {
        let (Some(watcher), Some(running)) = (&self.watcher, &mut self.running) else {
            return;
        };

        for os_path in watcher.drain() {
            let Some(path) = self.vfs.asset_path(&os_path) else {
                continue;
            };
            let Some(asset) = self.registry.lookup(&path) else {
                continue;
            };

            let result = match asset {
                Asset::Shader(shader) => self
                    .vfs
                    .read_text(&path)
                    .and_then(|src| running.renderer.reload_shader(shader, &src)),
            };

            match result {
                Ok(()) => log::info!("reloaded {path}"),
                Err(err) => log::error!("reload {path} failed, keeping old version: {err:#}"),
            }
        }
    }
}

impl<G: Game> Handler for App<G> {
    fn init(&mut self, window: Window) -> Result<()> {
        let size = window.size();
        let vfs = &self.vfs;
        let source = |shader| vfs.read_text(shader_path(shader));
        let mut renderer = Renderer::new(window, size, self.config.video.to_render(), source)?;

        self.game.init(&mut Gfx {
            renderer: &mut renderer,
        })?;

        // Clock starts after loading so load time is not simulated.
        self.running = Some(Running {
            renderer,
            clock: FixedStep::new(self.config.tick_hz),
            frame: Frame::new(),
            frame_index: 0,
            screenshot: Latch::default(),
        });

        Ok(())
    }

    fn event(&mut self, event: Event) {
        let Event::Resized { width, height } = event else {
            self.input.apply(event);
            return;
        };

        if let Some(running) = &mut self.running {
            running.renderer.resize(width, height);
        }
    }

    fn frame(&mut self) -> Flow {
        self.reload_assets();

        let Some(running) = &mut self.running else {
            return Flow::Continue;
        };

        let ticks = running.clock.advance();
        let dt = running.clock.dt();
        if sim::run_ticks(&mut self.game, &mut self.input, ticks, dt) == Flow::Exit {
            return Flow::Exit;
        }

        running.frame.begin(running.clock.alpha());
        self.game.draw(&mut running.frame);
        running.frame.submit(&mut running.renderer);
        running.frame_index += 1;

        if running
            .screenshot
            .rising(self.input.is_down(SCREENSHOT_KEY))
        {
            debug::save_screenshot(&running.renderer, &debug::timestamp_name());
        }

        if self.capture_frame == Some(running.frame_index) {
            debug::save_screenshot(&running.renderer, &format!("frame-{}", running.frame_index));
            return Flow::Exit;
        }

        Flow::Continue
    }
}
