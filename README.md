# Retro FPS Engine

A from-scratch first-person engine in Rust with a Quake / Half-Life-era look:
low internal resolution, chunky nearest-neighbor pixels, a hand-written
renderer. The look is a setting, not a limit, so the engine can grow past it.

**Status:** Phases 1 (first world) and 2 (walking around) of
[retro-fps-engine-plan.md](retro-fps-engine-plan.md) are well along:
a Quake map loader, brush collision, a character controller, entities
and Lua scripting all work. The sandbox is a
small brush-built level you walk through: stairs, a walkable and a
too-steep ramp, crates, and a room behind a sliding door opened by a
button. Inside, a Lua-scripted button opens a secret panel. Quake maps
compiled with ericw-tools load too: geometry, collision brushes and
entities (doors, buttons, triggers). Everything runs in distance fog at
640×360, quantized to a 256-color palette with ordered dithering, and
upscaled to the window.

## Quick start

Requires [rustup](https://rustup.rs). The compiler version is pinned in
`rust-toolchain.toml`, and rustup installs it on first build. There are no
system libraries on macOS or Windows. On Linux, install
`libxkbcommon-dev libwayland-dev libx11-dev`.

```bash
cargo run -p game
```

Or play the test map (`maps/test.map`, compiled to
`assets/maps/test.bsp`): three rooms, a door opened by the red button,
a door that opens as you approach, stairs, and a scripted trigger.

```bash
cargo run -p game -- maps/test.bsp
```

Bindings are physical key positions (WASD stays put on AZERTY). Only the
left-hand Shift and Ctrl are bound. You start walking, in the Survival
profile. Opposite keys cancel, and diagonals are never faster than
straight moves.

| Input | When | What it does |
|---|---|---|
| Left click | Mouse free | Captures the mouse: hides the cursor and turns mouse motion into looking. Does nothing while captured. |
| Mouse motion | Mouse captured | Right/left turns right/left, down/up looks down/up. Ignored while the mouse is free. |
| ← → | Always | Turn left / right at about 115°/s (2 rad/s) |
| ↑ ↓ | Always | Look up / down at the same rate. Looking up or down stops 1° short of vertical. |
| W / S | Walking | Walk forward / back, level whatever the view tilt. Top speed 3 m/s (Survival) or 8 m/s (Arena). |
| A / D | Walking | Strafe left / right, same speeds |
| Space | Walking | Jump, only from the ground, once per press (holding does not repeat). About 0.6 m high (Survival) or 1.2 m (Arena). |
| Left Shift | Walking, Survival | Sprint at 1.8× (5.4 m/s) while moving on the ground. A full bar lasts 5 s and refills in 8 s when not sprinting; once empty, no sprint until it is a quarter full. No effect in Arena. |
| Left Ctrl | Walking | Nothing (no crouch yet) |
| W / S | Flying | Fly forward / back along the view direction, including its tilt: looking down and pressing W descends. 5 m/s. |
| A / D | Flying | Strafe left / right, level with the ground. 5 m/s. |
| Space / Left Ctrl | Flying | Fly straight up / down, whatever the view direction. 5 m/s. |
| Left Shift | Flying | All flying 4× faster (20 m/s) |
| E | Always | Use: presses whatever usable thing is under the screen center within 2 m. Works while flying too. In the level: the red button beside the door opens it; the red button on the room's back wall is scripted (see below). |
| Esc | Mouse captured | Frees the mouse |
| Esc | Mouse free | Quits |
| F1 | Always | Toggles fog. Starts on: gray-blue mist (exp², density 0.08). |
| F2 | Always | Cycles palette quantization: dithered (the start) → off → nearest → dithered |
| F3 | Always | Walking ↔ flying. Flying passes through walls, has no gravity, and sets off no triggers. Back to walking puts your feet 1.6 m below the camera, keeping your view; if your body would be inside something there, nothing happens. |
| F4 | Always | Movement profile: Survival (the start) ↔ Arena. Applies at once while walking; while flying, it applies when you land. |
| F12 | Always | Saves the 640×360 frame, after the palette pass, to `screenshots/<ms since 1970>.png` under the working directory. Not window size. Engine-wide: works in any game. |

Switching away from the window frees the mouse; click to capture it again.

In the level: the door slides open into the wall when you press the
button, and closes 3 s after opening fully. It never closes on you; it
backs off. Walking into the room's front strip (just inside the door)
opens it again from inside. The button on the room's back wall runs
`SecretButton` in `assets/scripts/sandbox.lua`: the first two presses
log "Click. N more.", the third logs a grinding sound and the panel in
the room's west wall sinks into the floor for good. Script messages go
to the terminal log. Steps up to 45 cm climb on their own; slopes
steeper than about 45° are walls.

While it runs, edit `assets/shaders/*.wgsl`, `assets/textures/*.png`,
`assets/palettes/*.png` or `assets/scripts/*.lua` and save. The file reloads live. A broken file logs its error and the last good
version stays active.

| Environment variable | Effect |
|---|---|
| `RUST_LOG=debug` | More logging (default: `info`, GPU internals at `warn`). `RUST_LOG=engine::app=trace` logs every OS event; script messages log under the `script` target. |
| `ENGINE_ASSETS=<dir>` | Use this asset directory instead of searching upward for `assets/` |
| `ENGINE_CAPTURE_FRAME=<n>` | Save frame *n* to `screenshots/frame-<n>.png`, then exit |
| `REQUIRE_GPU=1` | Make GPU tests fail, instead of skip, when no GPU is found |
| `UPDATE_REFERENCE=1` | Rewrite reference images (`crates/engine/reference/*.png`) from the current render instead of comparing |

## Repository layout

```
assets/shaders/    WGSL shaders (hot-reloadable)
assets/textures/   PNG textures (hot-reloadable)
assets/palettes/   16×16 PNG palettes: 16 ramps of 16 shades (hot-reloadable)
assets/scripts/    Lua entity classes, all loaded at startup (hot-reloadable)
assets/maps/       compiled Quake maps (.bsp), committed so no tools are needed
maps/              map sources (.map): edit in TrenchBroom or by hand
tools/             compile-map.sh: .map → .bsp with ericw-tools
crates/
  base/            foundation: logging, verify!, Key/Button, Slots, Vfs, Watcher, Image/PNG
  platform/        OS layer: window, cursor capture, OS events, fixed-timestep clock (wraps winit)
  render/          renderer: low-res targets, meshes, textures, fog, palette pass, readback (wraps wgpu)
  physics/         collision: convex brushes, box traces, slide / step / ground moves (glam only)
  script/          Lua host: sandboxed VM, entity classes, hooks, commands (wraps mlua)
  bsp/             Quake map reader: BSP29/BSP2 faces, BSPX brushes, entities (no engine types)
  engine/          main loop, world + entities, walker and fly camera, maps, asset loading:
                   the API games are written against
game/              the game layer: a sandbox level (game/src/level.rs), or any map passed as an argument
```

## Architecture

```
            ┌────────────────────────────────────┐
            │ game        your code: impl Game   │
            └─────────────────┬──────────────────┘
                              ▼
            ┌──────────────────────────────────────────────────────────────┐
            │ engine      loop, input, world, entities, maps,              │
            │             walker, assets, public types                     │
            └────┬───────────┬────────────┬────────────┬────────────┬──────┘
                 ▼           ▼            ▼            ▼            ▼
            ┌─────────┐ ┌─────────┐ ┌──────────┐ ┌─────────┐ ┌──────────┐
            │render   │ │physics  │ │script    │ │bsp      │ │platform  │
            │(wgpu)   │ │(brushes)│ │(mlua)    │ │(.bsp)   │ │(winit)   │
            └─────────┘ └─────────┘ └──────────┘ └─────────┘ └──────────┘
   base ── foundation utilities, usable by every layer
```

Dependency rules:

- A layer talks only to the layer directly below it. `game` sees only
  `engine`. `render`, `physics`, `script`, `bsp` and `platform` are
  siblings and never depend on each other: the renderer takes any raw window handle, not a
  `platform::Window`.
- Library types stop at their wrapper. winit never leaves `platform`,
  wgpu never leaves `render`, and mlua never leaves `script`.
- Lower-layer types stop at `engine`. Games use `engine::Camera`,
  `engine::MeshData`, `engine::MeshHandle`, `engine::TextureHandle`,
  `engine::Fog`, `engine::Quantize`, `engine::Brush`, `engine::Trace`
  and `engine::CursorMode`. `engine/src/gfx.rs` is the only place they are
  converted to `render` types, `engine/src/collision.rs` the only place
  for `physics` types, `engine/src/world.rs` the only user of `script`,
  and `engine/src/map/` the only user of `bsp`, so lower-layer
  internals can change without breaking games.
- `base` has no knowledge of windows, GPUs or game rules. Shared vocabulary
  (`Key`, `Button`) lives there because two layers need it.
- `render` does no file I/O. The engine hands it shader source.

### What happens in one frame

```
platform event loop
  │  OS events ──► engine: Input::apply / Renderer::resize
  ▼  redraw
engine App::init: load assets/scripts/*.lua into the World, then Game::init(gfx, world)
engine App::frame
  1. hot reload   Watcher ─► Vfs path ─► Registry ─► assets::reload
                  (shader: recompile; texture, palette: decode, replace in place;
                   script: re-run in the world's Lua state)
  2. simulate     FixedStep::advance → N ticks → Game::tick(Tick { input, world, dt })
                  (input edges cleared after each tick; cursor requests applied after)
                  sandbox tick: walker.update(&world) → world.tick(dt, player box)
                                (movers, triggers, on_tick, due fires)
                                → E: world.use_from(eye, forward)
  3. describe     Game::draw(Frame { camera, alpha, fog, quantize }) → extra draws,
                  then the engine's world.draw(frame) (every entity model, interpolated)
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
- **Light.** A draw with a lightmap (`Frame::draw_lit`, or a `Model`
  with `lightmap`) takes its light from it at each vertex's
  `lightmap_uv`: sampled bilinear, Quake's 2× overbright (luxel 128 is
  full brightness). Without one, a fixed sun term lights by normal.
  Both run in one shader: the no-lightmap fallback texture has alpha 0,
  which selects the sun.
- **Brushes.** A level is convex `Brush`es (cuboids, turned boxes, or
  any closed set of planes, such as ramps). The same brush is what
  collides and, through `brush.mesh(meters_per_texture)`, what is drawn,
  so they never disagree. Textures are projected along the nearest world
  axis, Quake-style, so neighboring brushes line up. A map loader will
  produce the same brushes later.
- **Collision.** `physics` sweeps an axis-aligned box through the
  brushes (`World::trace`) and builds moves from that: slide along what
  is hit, step up ledges, find the floor, snap down stairs. That is
  Quake's movement code in shape. Moving bodies (doors) slide by an
  offset; nothing rotates after spawning.
- **Walker.** `engine::Walker` is the character controller: one box
  with Quake's friction, acceleration and capped air control (so
  strafe-jumping works). `WalkSettings::arena()` and `survival()` are
  profiles. Every field is public, so a custom profile is a struct
  update. Survival adds sprint with stamina and head bob.
- **Entities.** `engine::Entity` is one flat struct with optional
  components: `models` and `colliders` (lists: a map door can be several
  brushes and textures), `mover` (doors), `trigger` (fires
  on entry), `usable` (fires on E). They live in a `World` the game owns
  and talk by name, as in Quake's target/targetname: a button with
  `Usable { target: Some("door".into()) }` opens every entity named
  "door". Firing is queued and runs in `world.tick` (at most 64 fires
  per tick, so a script firing its own name cannot hang the game).
  Firing a name nobody has logs a warning. The engine owns the `World`
  (it loads scripts into it and draws it); the game gets it in `init`
  and as `tick.world`.
- **Scripts.** An entity's `script: Some("SecretButton".into())` names
  a Lua class: a global table in `assets/scripts/*.lua`. Each entity
  gets its own instance (`self`), and its hooks run on events:
  `on_spawn`, `on_fire`, `on_use`, `on_enter`, `on_tick(dt)`. Scripts
  act through `world.fire(name)`, `world.fire_after(seconds, name)` and
  `world.log(text)` (or `print`). They cannot read files or the OS, and a
  hook running past a million Lua instructions is stopped with an error.
  A failing script is logged, never fatal. Saving a script reloads it:
  classes update and every entity keeps its state. Scripts cannot read
  world state yet (positions, the player); that is the next step.
- **Maps.** `gfx.load_map("maps/test.bsp", world)` reads a Quake BSP
  (BSP29 or BSP2) and spawns it: per-texture meshes from its faces,
  collision from its BSPX brush list (so compile with `qbsp
  -wrbrushes`; plain BSPs only carry Quake's three fixed-size hulls),
  and entities from its entity text. 32 Quake units are a meter, and
  Quake's Z-up axes become Y-up. Classes: `func_door` (slides along
  "angle" by its size less "lip"; without a "targetname" it opens on
  approach, as in Quake), `func_button` (fires "target" on use),
  `trigger_once` / `trigger_multiple`, `func_illusionary` (drawn, not
  solid), other brush entities (drawn and solid), `info_player_start`
  (returned as `MapInfo::start`). Any entity's "script" key names its
  Lua class. Textures come from `textures/<name>.png`, the same files
  qbsp read sizes from, so the .bsp holds no pixels. Not yet:
  colored or switchable lights, button travel, linked doors, map hot
  reload. Baked light (L1): every face's lightmap is packed into one
  atlas (`engine/src/atlas.rs`) and drawn bilinear with Quake's 2×
  overbright; faces light never reached sample a black luxel.
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
| Use a camera | `Walker` to walk with collision, `FlyCamera` for exploring; or write your own by setting `frame.camera` in `draw` |
| Add level geometry | In `game/src/level.rs`: return another `Brush` from `stone()`, or spawn an `Entity` with `models` and `colliders` |
| Build or change a map | Edit `maps/<name>.map` (TrenchBroom, Quake format; textures are names from `assets/textures/`), then `ERICW_TOOLS=/path/to/ericw-tools/build tools/compile-map.sh <name>` and commit the `.bsp`. Play it with `cargo run -p game -- maps/<name>.bsp` |
| Script a map entity | Give it a `"script" "ClassName"` key and define the class in `assets/scripts/` (see `maps.lua`) |
| Change how the test map looks | Expect `view_from_the_start_matches_the_reference` to fail. Check the saved render in `target/reference/`, then rerun with `UPDATE_REFERENCE=1 cargo test -p engine reference` and commit `crates/engine/reference/test-map.png` |
| Add a door, button or trigger | Spawn an `Entity` with `name` + `mover`, `usable`, or `trigger` that targets the name; see `game/src/level.rs` |
| Script an entity | Add a class (global table with `on_*` methods) to a file in `assets/scripts/`, and set the entity's `script` to its name. New script files load on the next start; edits reload live. |
| Tune movement | Change `walker.settings` fields, or start from `WalkSettings::arena()` / `survival()` |
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
| `script` | unit tests: hooks with per-instance state and names, missing hooks skipped, unknown classes rejected, every `world.*` command and `print`, reload keeping state, error messages naming the file or hook, runaway loops stopped, no io/os/file access |
| `bsp` | unit tests: header and lump bounds, record sizes, the cursor, entity text (pairs, repeats, model indices, errors with line numbers), contents, missing textures, garbage input, luxel grids (snapping, negative texels), texel precision, Newell winding; `tests/test_map.rs` reads the committed test map: version, textures and sizes, models, entities, counter-clockwise faces, the floor, BRUSHLIST brushes, lightmaps that tile the lighting lump exactly, lightmap coordinates inside their grids |
| `physics` | unit tests: brush construction (planes, bevels, faces, winding), box and ray traces (fractions, normals, start-solid), slide along walls and into corners, step up and down, ground detection, snap down |
| `render` | unit tests: camera, letterbox, vertex and uniform layouts, fog encoding, mip chain sizes, palette loading, readback padding, GPU-test skip policy |
| `render` GPU | `tests/headless.rs`: renders offscreen and checks pixels (culling, depth, shader reload, texture mapping, nearest filtering, texture replace/destroy/size limit, mipmaps, fog curves and background, palette quantization, dither pattern, lightmap brightness and lightmap UVs) |
| shaders | `tests/shaders.rs`: naga validates every shipped `.wgsl` (no GPU needed) |
| `engine` | unit tests: input edges, tick loop, cursor requests, look (mouse, keys, clamping), fly camera, walker (top speed, friction, jump height, stairs, walls, staying grounded down steps, air control per profile, sprint and stamina, head bob, walking through a door), world (traces, doors opening, closing, backing off, triggers once or every entry, use within reach; scripts relaying fires, spawn/use/enter/tick hooks, delayed fires, the per-tick fire cap), shipped scripts load, brush meshes, map conversion (axes and handedness, planes, yaw, bounds, door travel and trigger fields, player start, malformed keys), multi-brush doors, `World::find`, lightmap atlas packing (no overlaps, luxels in place, power-of-two width), reference-image tolerances, asset registry, type conversion, debug hooks; GPU tests for texture and palette loading and hot reload, and for the real test map (start and title, solid floor, button opens door 1, door 2 opens on approach, a walker climbs its stairs, and the view from the start against the reference image `crates/engine/reference/test-map.png`) |
| `game` | cube geometry; level layout (spawn is clear, ramp slopes, stair rises, door seals and hides); the scripted secret button with the real `sandbox.lua` (opens on the third press only); landing from flight needs room; profile and palette cycling |

The reference-image test compares within GPU-to-GPU tolerances (a
channel within 24 levels, at most 1% of pixels off, mean difference ≤ 3),
since Metal, lavapipe and Windows' software adapter rasterize slightly
differently.

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
is done: BSP maps with baked lightmaps, checked by a reference-image
test of the test map. Phase 2 has collision, the walker, entities (spawned in code
or from maps) with doors, buttons and triggers, and the start of Lua
scripting.
