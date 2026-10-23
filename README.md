# Retro FPS Engine

A from-scratch first-person engine in Rust with a Quake / Half-Life-era look:
low internal resolution, chunky nearest-neighbor pixels, a hand-written
renderer. The look is a setting, not a limit, so the engine can grow past it.

**Status:** Phase 0 (foundations) of
[retro-fps-engine-plan.md](retro-fps-engine-plan.md). It renders a spinning
cube at 640×360, upscaled to the window.

## Quick start

Requires stable Rust (2024 edition). There are no system libraries on macOS
or Windows. On Linux, install `libxkbcommon-dev libwayland-dev libx11-dev`.

```bash
cargo run -p game
```

| Input | Action |
|---|---|
| Esc | Quit |
| F12 | Save a screenshot to `screenshots/<timestamp>.png` |

While it runs, edit `assets/shaders/*.wgsl` and save. The shader reloads
live. A broken shader logs its error and the last good version stays active.

| Environment variable | Effect |
|---|---|
| `RUST_LOG=debug` | More logging (default: `info`, GPU internals at `warn`) |
| `ENGINE_ASSETS=<dir>` | Use this asset directory instead of searching upward for `assets/` |
| `ENGINE_CAPTURE_FRAME=<n>` | Save frame *n* to `screenshots/frame-<n>.png`, then exit |
| `REQUIRE_GPU=1` | Make GPU tests fail, instead of skip, when no GPU is found |

## Repository layout

```
assets/shaders/    WGSL shaders (hot-reloadable)
crates/
  base/            foundation: logging, verify!, Key/Button, Slots, Vfs, Watcher, PNG
  platform/        OS layer: window, OS events, fixed-timestep clock (wraps winit)
  render/          renderer: low-res target, passes, readback (wraps wgpu)
  engine/          main loop + the API games are written against
game/              the game layer; today a cube sandbox
```

## Architecture

```
            ┌────────────────────────────────────┐
            │ game        your code: impl Game   │
            └─────────────────┬──────────────────┘
                              ▼
            ┌────────────────────────────────────┐
            │ engine      loop, input state,     │
            │             assets, public types   │
            └────────┬──────────────────┬────────┘
                     ▼                  ▼
            ┌────────────────┐ ┌─────────────────┐
            │ render (wgpu)  │ │ platform (winit)│
            └────────────────┘ └─────────────────┘
   base ── foundation utilities, usable by every layer
```

Dependency rules:

- A layer talks only to the layer directly below it. `game` sees only
  `engine`. `render` and `platform` are siblings and never depend on each
  other: the renderer takes any raw window handle, not a `platform::Window`.
- Library types stop at their wrapper. winit never leaves `platform` and
  wgpu never leaves `render`.
- Lower-layer types stop at `engine`. Games use `engine::Camera`,
  `engine::MeshData` and `engine::MeshHandle`. `engine/src/gfx.rs` is the
  only place they are converted to `render` types, so render internals can
  change without breaking games.
- `base` has no knowledge of windows, GPUs or game rules. Shared vocabulary
  (`Key`, `Button`) lives there because two layers need it.
- `render` does no file I/O. The engine hands it shader source.

### What happens in one frame

```
platform event loop
  │  OS events ──► engine: Input::apply / Renderer::resize
  ▼  redraw
engine App::frame
  1. hot reload   Watcher ─► Vfs path ─► Registry ─► Renderer::reload_shader
  2. simulate     FixedStep::advance → N ticks → Game::tick(Tick { input, dt })
                  (input edges cleared after each tick)
  3. describe     Game::draw(Frame { camera, alpha }) → frame.draw_mesh(...)
  4. render       MeshPass → 640×360 target → BlitPass (nearest, letterbox) → window
  5. debug        F12 / ENGINE_CAPTURE_FRAME → Renderer::capture → PNG
```

### Key ideas

- **Fixed timestep.** The simulation advances in exact 1/60 s ticks
  (`platform::FixedStep`). Rendering runs at display rate and blends the last
  two ticks with `frame.alpha`; see `game/src/main.rs`. This keeps physics
  stable and makes replays possible later.
- **Input per tick.** `Input::was_pressed` is true on exactly one tick per
  press, even when a frame runs zero ticks or several.
- **The retro look is a render target.** The scene is drawn at
  `Video::internal_width × internal_height`, then upscaled with nearest
  sampling. Phase 1 will add a palette and dither pass in `blit.wgsl`.
- **Handles, not references.** `MeshHandle` wraps a generational
  `base::SlotId`. A handle to a destroyed mesh is detected rather than
  silently drawing whatever reused its slot.
- **Errors.** Fallible operations return `anyhow::Result`. Broken
  invariants use `base::verify!`, which logs and panics only in debug builds.

## Making changes

| Task | Where |
|---|---|
| Write game logic | `game/src/main.rs`: implement `engine::Game` (`init`, `tick`, `draw`) |
| Tweak shading | `assets/shaders/mesh.wgsl`, live while running |
| Change resolution or scaling | `Config { video: Video { .. } }` in `game/src/main.rs` |
| Add a key | `base/src/keys.rs` (variant, update `COUNT`) and `platform/src/event.rs` (`map_key`) |
| Add a shader | `render::Shader` variant, use it in a pass, add its path to `engine/src/assets.rs` (`shader_path`), add the file to the `shaders` test |
| Add a render pass | `crates/render/src/passes/`, record it in `Renderer::render` |
| Add a reloadable asset type | `Asset` variant and `Registry::register` in `engine/src/assets.rs`, dispatch in `App::reload_assets` |
| Expose a renderer feature to games | Add an engine-side type in `engine/src/gfx.rs` and convert there. Never re-export a `render` type. |

## Testing

```bash
cargo test
```

| Area | Covered by |
|---|---|
| `base` | unit tests: slots, Vfs, watcher, PNG round-trip, `verify!` |
| `platform` | unit tests: clock math (injected time), key/button mapping |
| `render` | unit tests: camera, letterbox, vertex layout, readback padding |
| `render` GPU | `tests/headless.rs`: renders offscreen and checks pixels (culling, depth, shader reload) |
| shaders | `tests/shaders.rs`: naga validates every shipped `.wgsl` (no GPU needed) |
| `engine` | unit tests: input edges, tick loop, asset registry, type conversion, debug hooks |
| `game` | cube geometry: counts, bounds, outward winding |

The GPU tests skip with a message when no adapter exists. Set
`REQUIRE_GPU=1` to make that a failure; Linux CI does this and uses Mesa's
software Vulkan driver. Not covered by unit tests: the window event loop and
presenting to a real window. Check those with `cargo run -p game` or
`ENGINE_CAPTURE_FRAME=90 cargo run -p game`.

CI runs fmt, clippy (`-D warnings`, which enforces `missing_docs` on every
library crate's public API) and the tests on macOS, Linux and Windows.

## Conventions

- Short functions with early returns. No deep nesting.
- Enums instead of boolean parameters (`VSync::On`, not `true`).
- Named constants for meaningful values. Obvious one-offs stay inline.
- Every module starts with a `//!` header saying what it does and why. Use
  ASCII diagrams where a flow is involved.
- Commit messages: imperative subject of 50 characters or fewer, and a body
  that explains why.

## Roadmap

See [retro-fps-engine-plan.md](retro-fps-engine-plan.md). It starts with a
status section: what is implemented and where each phase stands. Next is
Phase 0.5, a set of small follow-ups found in review (CI on GitHub, typed
errors, workspace lints, a pinned toolchain). After that comes Phase 1:
fly camera, textured meshes, a BSP map loader, lightmaps, fog, and the
palette/dither post-pass.
