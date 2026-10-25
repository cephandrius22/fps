# Retro FPS Engine

A from-scratch first-person engine in Rust with a Quake / Half-Life-era look:
low internal resolution, chunky nearest-neighbor pixels, a hand-written
renderer. The look is a setting, not a limit, so the engine can grow past it.

**Status:** Phase 1 (first world) of
[retro-fps-engine-plan.md](retro-fps-engine-plan.md), in progress. The
sandbox is a fly-through scene: a tiled stone floor, textured crates and a
spinning cube in distance fog, rendered at 640×360, quantized to a
256-color palette with ordered dithering, and upscaled to the window.

## Quick start

Requires [rustup](https://rustup.rs). The compiler version is pinned in
`rust-toolchain.toml`, and rustup installs it on first build. There are no
system libraries on macOS or Windows. On Linux, install
`libxkbcommon-dev libwayland-dev libx11-dev`.

```bash
cargo run -p game
```

| Input | Action |
|---|---|
| Left click | Capture the mouse for mouse look |
| Mouse / arrow keys | Look around |
| W A S D | Fly forward, left, back, right |
| Space / Left Ctrl | Fly up / down |
| Left Shift | Fly faster |
| Esc | Release the mouse; if already released, quit |
| F1 | Fog on / off |
| F2 | Palette: off → nearest → dithered |
| F12 | Save a screenshot to `screenshots/<timestamp>.png` |

While it runs, edit `assets/shaders/*.wgsl`, `assets/textures/*.png` or
`assets/palettes/*.png` and save. The file reloads live. A broken file logs its error and the last good
version stays active.

| Environment variable | Effect |
|---|---|
| `RUST_LOG=debug` | More logging (default: `info`, GPU internals at `warn`). `RUST_LOG=engine::app=trace` logs every OS event. |
| `ENGINE_ASSETS=<dir>` | Use this asset directory instead of searching upward for `assets/` |
| `ENGINE_CAPTURE_FRAME=<n>` | Save frame *n* to `screenshots/frame-<n>.png`, then exit |
| `REQUIRE_GPU=1` | Make GPU tests fail, instead of skip, when no GPU is found |

## Repository layout

```
assets/shaders/    WGSL shaders (hot-reloadable)
assets/textures/   PNG textures (hot-reloadable)
assets/palettes/   16×16 PNG palettes: 16 ramps of 16 shades (hot-reloadable)
crates/
  base/            foundation: logging, verify!, Key/Button, Slots, Vfs, Watcher, Image/PNG
  platform/        OS layer: window, cursor capture, OS events, fixed-timestep clock (wraps winit)
  render/          renderer: low-res targets, meshes, textures, fog, palette pass, readback (wraps wgpu)
  engine/          main loop, fly camera, asset loading + the API games are written against
game/              the game layer; today a fly-through sandbox
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
  `engine::MeshData`, `engine::MeshHandle`, `engine::TextureHandle`,
  `engine::Fog`, `engine::Quantize` and `engine::CursorMode`. `engine/src/gfx.rs` is the
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
  1. hot reload   Watcher ─► Vfs path ─► Registry ─► assets::reload
                  (shader: recompile; texture, palette: decode, replace in place)
  2. simulate     FixedStep::advance → N ticks → Game::tick(Tick { input, dt })
                  (input edges cleared after each tick; cursor requests applied after)
  3. describe     Game::draw(Frame { camera, alpha, fog, quantize }) → draw_mesh / draw_textured
  4. render       MeshPass (lit, fogged) → 640×360 scene
                  → PostPass (palette, dither) → 640×360 output
                  → BlitPass (nearest, letterbox) → window
  5. debug        F12 / ENGINE_CAPTURE_FRAME → Renderer::capture → PNG
```

### Key ideas

- **Fixed timestep.** The simulation advances in exact 1/60 s ticks
  (`platform::FixedStep`). Rendering runs at display rate and blends the last
  two ticks with `frame.alpha`; see `game/src/main.rs`. This keeps physics
  stable and makes replays possible later.
- **Input per tick.** `Input::was_pressed` is true on exactly one tick per
  press, even when a frame runs zero ticks or several.
- **The retro look is a set of dials.** The scene is drawn at
  `Video::internal_width × internal_height`, then upscaled with nearest
  sampling. `Video::mipmaps` and `frame.quantize` turn the rest up or down.
- **Palette pass.** `post.wgsl` runs at low res, after lighting and fog.
  It snaps each pixel to the nearest color of the palette loaded with
  `gfx.load_palette` (up to 256, one per pixel of the PNG). `Dithered`
  first adds a 4×4 Bayer offset, so a gradient becomes a fixed pixel
  pattern instead of bands. The pass compares sRGB-encoded values, the
  space palettes are authored in: the scene target is read through a raw
  (non-sRGB) view, so output pixels are byte-exact palette colors (see
  `render/src/target.rs`). It searches all 256 colors per pixel, which is
  cheap at 640×360; a 3D lookup texture is the upgrade if resolution grows.
- **Fog.** `frame.fog = Some(Fog { color, falloff })`, with `Linear`,
  `Exp` or `Exp2` falloff, measured as straight-line distance from the
  camera. Empty background is drawn in the fog color so distant geometry
  fades out without a silhouette. Fog is set per frame, so a level or a
  script can change it.
- **Handles, not references.** `MeshHandle` and `TextureHandle` wrap a
  generational `base::SlotId`. A handle to a destroyed resource is detected
  rather than silently drawing whatever reused its slot. Hot reload replaces
  a texture's pixels behind its handle, so draws never need updating.
- **Textures.** PNG files are decoded to RGBA8 (`base::Image`), uploaded as
  sRGB, and sampled nearest-neighbor with repeat, so UVs past 1 tile.
  `Video::mipmaps` picks `Off` (distant surfaces shimmer), `Nearest` (the
  default: hard switches between levels, as in Quake's software renderer)
  or `Linear` (blended levels). The chain is built on the CPU at upload by
  `Image::half`, which averages in linear light so a black/white checker
  turns mid-gray, not dark. Draws without a texture use a 1×1 white one,
  so the shader has no untextured special case. Fragment color = texture ×
  vertex color × light, then fog.
- **Cameras.** `engine::FlyCamera` is a ready-made free-flying camera
  (no collision). Update it in `tick`, read it in `draw` with
  `camera(frame.alpha)` so motion is smooth at any frame rate. Mouse look
  needs `tick.set_cursor(CursorMode::Captured)`; the engine frees the cursor
  when the window loses focus.
- **Errors.** Library crates return typed errors, one `thiserror` enum
  per area (`VfsError`, `RenderError`, `EngineError`, ...), so callers can
  match on what went wrong. Each error's message covers only its own level;
  `base::chain(&err)` prints the full cause chain for logs. Games see only
  `EngineError`, whose categories (`NoGpu`, `Renderer`, ...) wrap lower-layer
  errors as opaque causes. `anyhow` is used only in the `game` binary.
  Broken invariants use `base::verify!`, which logs and panics only in
  debug builds.

## Making changes

| Task | Where |
|---|---|
| Write game logic | `game/src/main.rs`: implement `engine::Game` (`init`, `tick`, `draw`) |
| Tweak shading or fog curves | `assets/shaders/mesh.wgsl`, live while running |
| Change fog | Set `frame.fog` in `draw` |
| Change the palette | Edit `assets/palettes/default.png` (live), or load another with `gfx.load_palette` in `init`. Tune dither strength with `DITHER_SPREAD` in `assets/shaders/post.wgsl` |
| Add a texture | Put a PNG in `assets/textures/`, load it in `init` with `gfx.load_texture("textures/x.png")?`, draw with `frame.draw_textured(mesh, texture, model)` |
| Use a camera | `FlyCamera` for exploring; write your own by setting `frame.camera` in `draw` |
| Change resolution, scaling or mipmaps | `Config { video: Video { .. } }` in `game/src/main.rs` |
| Add a key | `base/src/keys.rs` (variant, update `COUNT`) and `platform/src/event.rs` (`map_key`) |
| Add a shader | `render::Shader` variant, use it in a pass, add its path to `engine/src/assets.rs` (`shader_path`), add the file to the `shaders` test |
| Add a render pass | `crates/render/src/passes/`, record it in `Renderer::render` |
| Add a reloadable asset type | `Asset` variant and its case in `assets::reload` (`engine/src/assets.rs`); register the path when the asset is loaded |
| Expose a renderer feature to games | Add an engine-side type in `engine/src/gfx.rs` and convert there. Never re-export a `render` type. |

## Testing

```bash
cargo test
```

| Area | Covered by |
|---|---|
| `base` | unit tests: slots, Vfs, watcher, image validation, PNG decode (all color types) and round-trip, mip downsampling (linear-light average, odd sizes), error chains |
| `platform` | unit tests: clock math (injected time), key/button mapping, cursor grab fallback order |
| `render` | unit tests: camera, letterbox, vertex and uniform layouts, fog encoding, mip chain sizes, palette loading, readback padding, GPU-test skip policy |
| `render` GPU | `tests/headless.rs`: renders offscreen and checks pixels (culling, depth, shader reload, texture mapping, nearest filtering, texture replace/destroy/size limit, mipmaps, fog curves and background, palette quantization, dither pattern) |
| shaders | `tests/shaders.rs`: naga validates every shipped `.wgsl` (no GPU needed) |
| `engine` | unit tests: input edges, tick loop, cursor requests, fly camera (movement, look, clamping, interpolation), asset registry, type conversion, debug hooks; GPU tests for texture and palette loading and hot reload |
| `game` | shape geometry: counts, bounds, outward winding, floor UV tiling; palette mode cycling |

GPU tests in any crate use `render::test_support` (a dev-only feature), so
they share one policy: they skip only when no GPU adapter exists; any other renderer
failure (a broken shader, say) fails them. In CI a skip also shows up as a
"GPU tests skipped" warning on the run's summary page. Set `REQUIRE_GPU=1`
to make a missing GPU a failure; Linux CI does this and uses Mesa's software
Vulkan driver. To try the no-GPU path on a Mac, run with
`WGPU_BACKEND=vulkan`. Not covered by unit tests: the window event loop and
presenting to a real window. Check those with `cargo run -p game` or
`ENGINE_CAPTURE_FRAME=90 cargo run -p game`.

CI runs fmt, clippy (`-D warnings`) and the tests on macOS, Linux and
Windows, using the pinned toolchain. Lints are set once in the root
`Cargo.toml` (`[workspace.lints]`): `missing_docs` warns, so CI fails on
undocumented public API, and `unsafe_code` is forbidden.

To move to a newer Rust: change `channel` in `rust-toolchain.toml`, run
`cargo clippy --all-targets -- -D warnings`, fix any new lints, commit.

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
status section: what is implemented and where each phase stands. Phase 1
is in progress: the fly camera, textured meshes, mipmaps, distance fog and
the palette/dither pass are done. Next come a BSP map loader (needs
ericw-tools), lightmaps, and a reference-image test.
