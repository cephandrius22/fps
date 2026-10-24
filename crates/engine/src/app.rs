//! Main loop. Per frame:
//!
//!   hot reload ─► N fixed ticks (game.tick) ─► game.draw(alpha) ─► render
//!   ─► debug captures (F12, $ENGINE_CAPTURE_FRAME)

use base::{Vfs, Watcher};
use platform::{Event, FixedStep, Flow, Handler, RunError, Window, WindowConfig};
use render::Renderer;

use crate::assets::{self, Registry, shader_path};
use crate::debug::{self, Latch, SCREENSHOT_KEY};
use crate::error::EngineError;
use crate::gfx::{Frame, Gfx};
use crate::input::{CursorMode, Input};
use crate::{Config, Game, sim};

pub(crate) fn run(config: Config, game: impl Game) -> Result<(), EngineError> {
    let vfs = Vfs::discover()?;
    log::info!("assets: {}", vfs.root().display());

    // Hot reload is a convenience; run without it rather than fail.
    let watcher = Watcher::new(vfs.root())
        .inspect_err(|err| log::warn!("hot reload disabled: {}", base::chain(err)))
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
        cursor: CursorMode::Free,
        capture_frame: debug::capture_frame(),
        running: None,
    };

    // Init errors are already ours; only OS failures need wrapping.
    match platform::run(window, app) {
        Ok(()) => Ok(()),
        Err(RunError::Init(err)) => Err(err),
        Err(RunError::Platform(err)) => Err(EngineError::Window(err.into())),
    }
}

struct App<G> {
    config: Config,
    game: G,
    vfs: Vfs,
    watcher: Option<Watcher>,
    registry: Registry,
    input: Input,
    /// What the game wants; `Running::applied_cursor` is what the window has.
    cursor: CursorMode,
    capture_frame: Option<u64>,
    running: Option<Running>,
}

/// State that exists only once the window is up.
struct Running {
    window: Window,
    applied_cursor: CursorMode,
    renderer: Renderer,
    clock: FixedStep,
    frame: Frame,
    frame_index: u64,
    screenshot: Latch,
}

impl Running {
    /// Brings the window's cursor in line with `wanted`. On failure the
    /// cursor stays free, and the game sees that next tick.
    fn apply_cursor(&mut self, wanted: &mut CursorMode) {
        if *wanted == self.applied_cursor {
            return;
        }

        if let Err(err) = self.window.set_cursor(wanted.to_platform()) {
            log::warn!("cursor {wanted:?} unavailable: {}", base::chain(&err));
            *wanted = CursorMode::Free;
        }
        self.applied_cursor = *wanted;
    }
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

            match assets::reload(&mut running.renderer, &self.vfs, &path, asset) {
                Ok(()) => log::info!("reloaded {path}"),
                Err(err) => log::error!(
                    "reload {path} failed, keeping old version: {}",
                    base::chain(&*err)
                ),
            }
        }
    }
}

impl<G: Game> Handler for App<G> {
    type Error = EngineError;

    fn init(&mut self, window: Window) -> Result<(), EngineError> {
        let size = window.size();
        let vfs = &self.vfs;
        let source = |shader| vfs.read_text(shader_path(shader));
        let config = self.config.video.to_render();
        let mut renderer = Renderer::new(window.clone(), size, config, source)?;

        self.game
            .init(&mut Gfx {
                renderer: &mut renderer,
                vfs: &self.vfs,
                registry: &mut self.registry,
            })
            .map_err(EngineError::Game)?;

        // Clock starts after loading so load time is not simulated.
        self.running = Some(Running {
            window,
            applied_cursor: CursorMode::Free,
            renderer,
            clock: FixedStep::new(self.config.tick_hz),
            frame: Frame::new(),
            frame_index: 0,
            screenshot: Latch::default(),
        });

        Ok(())
    }

    fn event(&mut self, event: Event) {
        log::trace!("{event:?}");

        match event {
            Event::Resized { width, height } => {
                if let Some(running) = &mut self.running {
                    running.renderer.resize(width, height);
                }
            }
            // The OS drops a grab on focus loss; mirror that so the game
            // knows to ask again.
            Event::FocusLost => {
                self.cursor = CursorMode::Free;
                self.input.apply(event);
            }
            _ => self.input.apply(event),
        }
    }

    fn frame(&mut self) -> Flow {
        self.reload_assets();

        let Some(running) = &mut self.running else {
            return Flow::Continue;
        };

        let ticks = running.clock.advance();
        let dt = running.clock.dt();
        let flow = sim::run_ticks(&mut self.game, &mut self.input, &mut self.cursor, ticks, dt);
        if flow == Flow::Exit {
            return Flow::Exit;
        }
        running.apply_cursor(&mut self.cursor);

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
