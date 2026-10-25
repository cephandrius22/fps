# Retro FPS Engine — Development Plan

A from-scratch first-person game engine with a Quake / Half-Life-era look, built to grow beyond it. The target game is a first-person action or survival-horror title. The engine must not assume Quake-style gameplay.

---

## Status (2026-10-25)

**Stack:** Rust (2024 edition) + wgpu 30 + winit 0.30. **Phases 0 and 0.5 are done** and CI is green on all three OSes. **Phase 1 is done**: Quake maps compiled with ericw-tools load with geometry, BSPX brushes, entities and baked lightmaps, checked by a reference-image test. **Phase 2 is done**: brush collision, the Arena and Survival walker, entities (spawned in code or from maps), doors, buttons and triggers, and Lua-scripted entity behavior (reading the world, driving movers) work in both the sandbox level and the test map. See [README.md](README.md) for how the code is organized and how to build it.

**Next.** Phase 2's planned items are all in. Recommended next: Phase 3 (own toolchain: `mapc`, our own lightmap baker, glTF props), or first the smaller follow-ups below. ericw-tools 2.0.0-alpha11 (`qbsp`, `vis`, `light`) is built on the development machine, outside the repo; CI has no ericw-tools, so compiled maps are committed. Also unblocked: colored lightmaps (`light -bspxlit`, BSPX RGBLIGHTING); `WalkSettings` exposed to scripts (the Custom profile); crouching; moving platforms that carry the player; debug drawing of collision hulls. Blocked on a decision: saves (pick a serialization approach, e.g. approve serde).

### Roadmap

| Phase | Goal | Status |
|---|---|---|
| 0 — Foundations | Window, loop, GPU, low-res cube | ✅ Done |
| 0.5 — Follow-ups | Code-health items found in review | ✅ Done (optional renames skipped) |
| 1 — First world | Fly through a lit, fogged BSP level | ✅ Done (lightmaps and reference-image test added 2026-10-25) |
| 2 — Walking around | Collision, character controller, entities, Lua | ✅ Done (2026-10-25); follow-ups listed under Phase 2 |
| 3 — Own toolchain | `mapc`, lightmap baker, glTF props | Not started |
| 4 — Dynamic lighting and atmosphere | Dynamic lights, flashlight shadows, fog volumes, 3D audio | Not started |
| 5 — Living world | Animation, AI, weapons, inventory, saves: vertical slice | Not started |
| 6 — Polish and expansion | Volumetric fog, clustered lighting, portals, menus | Not started |

Section 11 has the detail for each phase.

### Implemented

| Area | What exists | Where |
|---|---|---|
| Build | Cargo workspace, one crate per layer, dependency versions set once in the root manifest | `Cargo.toml`, `crates/` |
| Platform | winit window, cursor capture (locked, falling back to confined), OS events translated to engine `Key`/`Button`, fixed-timestep clock (60 Hz, stall cap) | `crates/platform` |
| Renderer | wgpu device and swapchain, 640×360 low-res targets with depth, nearest-neighbor letterboxed upscale (fit or integer), mesh pass with back-face culling, textures (sRGB, nearest, repeat, mipmaps off / nearest / linear, white fallback, in-place replace), lightmap textures (bilinear, clamped, no mips), frame readback | `crates/render` |
| Lighting | L1: baked lightmaps from ericw-tools' `light`, Quake's 2× overbright, one atlas per map; faces no light reached draw black. L0 (a fixed sun term) for draws without a lightmap, such as the sandbox. | `assets/shaders/mesh.wgsl`, `crates/engine/src/atlas.rs`, `crates/engine/src/map/lighting.rs` |
| Fog | F0: distance fog per frame (linear, exp, exp²), radial distance, background drawn in the fog color | `assets/shaders/mesh.wgsl`, `engine::Fog` |
| Retro post-pass | Low-res pass after lighting: nearest-color palette quantization (up to 256 colors from a PNG, hot-reloadable) with optional 4×4 Bayer dither; byte-exact palette output | `assets/shaders/post.wgsl`, `crates/render/src/passes/post.rs`, `assets/palettes/` |
| Engine core | Main loop, per-tick input state, cursor mode requests, `Game` trait (`init` / `tick` / `draw` with interpolation), engine-owned public types | `crates/engine` |
| Camera | `FlyCamera`: WASD + Space/Ctrl, Shift for speed, mouse or arrow-key look, pitch clamp, per-frame interpolation. Look shared with the walker. | `crates/engine/src/fly.rs`, `look.rs` |
| Collision | Convex brushes (cuboids, turned boxes, any closed plane set) with axial bevels; axis-aligned box and ray traces; slide, step-up, ground probe, snap-down. Moving bodies slide by offset. | `crates/physics` |
| Character controller | `Walker`: Quake friction / acceleration / capped air control, jumping, 45 cm steps, slopes up to ~45°. Profiles `WalkSettings::arena()` (fast, air control) and `survival()` (slow, sprint with stamina, head bob); all fields public for custom ones. | `crates/engine/src/walk.rs` |
| Scripting | Lua 5.4 via `mlua` (vendored), wrapped in `crates/script` (VM host in `lib.rs`, the Lua-facing API in `api.rs`, the world snapshot in `view.rs`). Entity classes are global tables in `assets/scripts/*.lua`, loaded at startup and hot-reloaded (instances keep state). Hooks `on_spawn` / `on_fire` / `on_use` / `on_enter` / `on_tick`; actions `world.fire`, `world.fire_after`, `world.open`, `world.close`, `world.log`, `print`; reads `world.position`, `world.mover`, `world.player`, `world.time` from a snapshot the engine publishes before hooks (`view.rs`). Targets are names or `self`. Sandboxed (no io/os/files), with a per-call instruction budget. | `crates/script`, `crates/engine/src/world/scripting.rs`, `assets/scripts/` |
| Entities | Flat `Entity` with optional model, collider, mover, trigger, usable, script; generational `EntityId`. The engine owns the `World`. Mover state machine in its own module. Named targets: triggers, buttons and scripts fire a name (queued, at most 64 per tick, with optional delay); movers with that name open or close, scripts get `on_fire`. Doors close after a wait or on fire, and back off instead of crushing. Brushes double as meshes (`Brush::mesh`, world-projected UVs). | `crates/engine/src/world/`, `entity.rs`, `mover.rs`, `collision.rs` |
| Resources | Asset filesystem (`assets/` discovery), PNG decoding (all color types) to `base::Image`, meshes and textures addressed by generational handles | `crates/base`, `crates/render`, `crates/engine/src/gfx.rs` |
| Maps | Quake BSP29/BSP2 reader with the BSPX brush list (`crates/bsp`, no engine types; one module per part of the file: lump directory, faces and models, textures, brush list, entity text), converted to meters and Y-up (32 units/m) in `engine/src/map/` (one file each for space, entities, geometry, lighting): per-texture meshes, solid brushes, `func_door` (with Quake's trigger field when untargeted), `func_button`, `trigger_once`/`_multiple`, `func_illusionary`, other brush entities, `info_player_start`, "script" keys; style-0 lightmaps sized exactly as `light` sizes them. Textures by name from `assets/textures/`. Test map source in `maps/`, compiled with `tools/compile-map.sh` (qbsp `-wrbrushes -notex`, vis, light). | `crates/bsp`, `crates/engine/src/map/`, `maps/`, `tools/`, `assets/maps/` |
| Hot reload | File watcher plus an asset registry. Shaders, textures, the palette and scripts reload; a bad file is rejected and the old version stays. | `crates/base/src/watch.rs`, `crates/engine/src/assets.rs` |
| Debugging | Logging (`RUST_LOG`), `verify!` asserts, F12 screenshot, `ENGINE_CAPTURE_FRAME=N` capture-and-exit | `crates/base`, `crates/engine/src/debug.rs` |
| Errors | Typed `thiserror` errors per area; `EngineError` categories for games; `base::chain` for logs | `crates/*/src/error.rs` and per module |
| Tests | 317 tests: unit tests per crate, headless GPU pixel tests (shared skip policy in `render::test_support`), naga shader validation | `cargo test` |
| CI | fmt, clippy `-D warnings`, tests on macOS/Linux/Windows with the pinned toolchain. GPU tests run on all three (Linux via Mesa lavapipe); a skip would show as a run warning. | `.github/workflows/ci.yml` |
| Build config | Toolchain pinned to 1.99.1, lints in `[workspace.lints]`, `publish = false`, dependency debug info reduced to line tables | `rust-toolchain.toml`, `Cargo.toml` |

Not implemented yet, though the plan's sections describe them: colored and switchable lightmaps, map hot reload, height and volumetric fog, crouching, moving platforms that carry the player, serialization (saves), audio.

---

## 1. Goals and constraints

| Requirement | What it means for the design |
|---|---|
| Hand-written renderer | No third-party engine. Use a thin graphics API layer and write all rendering ourselves. |
| Low poly now, more detail later | No hard-coded vertex or texture limits. Use a general mesh format, not Quake's BSP as the runtime format. |
| More advanced lighting later | The renderer must support dynamic lights and shadows, and the lighting model must be layered so it can be upgraded in stages. |
| Fog | Distance fog from day one. Height fog and volumetric fog later. Fog volumes authored in the level editor. |
| Not locked into Quake gameplay | Gameplay lives in data and scripts on top of a generic entity system. Movement, combat, inventory, and AI are all swappable modules. |
| Cross-platform | macOS (primary), Linux, Windows. |

### Non-goals (for now)
- Multiplayer and networking. The simulation loop is still kept deterministic-friendly so networking stays possible later.
- Consoles and mobile.
- An in-engine level editor. TrenchBroom fills that role.

---

## 2. Key decision: rendering approach

**Decision: GPU renderer, written by hand on a modern low-level API. The "retro look" is an art direction applied through materials and post-processing, not a limitation of the renderer.**

A software rasterizer would be the most authentic route, but it fights every expansion goal: dynamic shadows, volumetric fog, and higher poly counts are all expensive or awkward on the CPU. A GPU renderer can *imitate* the software look exactly, and it can drop that look gradually as the game grows.

How the retro look is produced:
- Render at a low internal resolution (e.g. 640×360), then upscale with nearest-neighbor.
- Nearest-neighbor texture sampling, optionally with no mipmaps. ✅
- Optional palette-quantization post-pass (256 colors) with ordered dithering. ✅ Brute-force nearest-color search; a 3D LUT is the optimization if the internal resolution grows.
- Optional vertex snapping and lighting banding.
- Each item is a config toggle, so the look becomes a dial rather than a fixed constraint.

> Optional side project: a software renderer as a second backend, sharing the scene and asset code. This is fun and educational, but it is not on the critical path.

---

## 3. Key decision: language and stack

**Decided: Rust + wgpu, with winit for the window and input** (chosen 2026-10-23). The C column is kept for reference. Choices still open in the Rust column are listed in section 15.

| | **C** | **Rust** |
|---|---|---|
| Graphics API | SDL3 GPU API (Metal / Vulkan / D3D12) | wgpu (Metal / Vulkan / D3D12 / GL) |
| Window, input, audio | SDL3 | SDL3 (via `sdl3` crate) or winit + cpal |
| Math | cglm or handwritten | glam |
| Models | cgltf | gltf crate |
| Images | stb_image | image crate |
| Scripting | Lua 5.4 / LuaJIT | mlua (Lua) |
| Navigation | Recast/Detour | recastnavigation-rs bindings, or a port |
| Pros | Matches id/Abrash reference code. Fast compile. Total control. | Safer asset parsing and threading. Great tooling and package management. |
| Cons | Manual memory management. More time spent debugging. | Ownership friction around game state. wgpu adds an abstraction layer. |

Original recommendation: **C + SDL3** if studying id's code is part of the fun. **Rust + wgpu** if long-term maintainability matters more. In either case, keep the renderer behind a small internal interface so the backend can change. That interface now exists: games use `engine` types only, and wgpu stays inside `crates/render`.

---

## 4. Architecture overview

```
┌──────────────────────────────────────────────────────────┐
│                 Game layer (scripts + data)              │
│   weapons · inventory · AI behaviors · triggers · UI     │
├──────────────────────────────────────────────────────────┤
│                       Engine core                        │
│  Entity system │ Physics/collision │ Audio │ Navigation  │
│  Scripting VM  │ Save/load         │ Input │ Resources   │
├──────────────────────────────────────────────────────────┤
│                         Renderer                         │
│  Scene → visibility → passes (shadow, opaque, fog,       │
│  transparent, post) → retro post-processing              │
├──────────────────────────────────────────────────────────┤
│            Platform: winit + wgpu + filesystem           │
└──────────────────────────────────────────────────────────┘

Offline tools: .map compiler · lightmap baker · asset converters
```

### As built (Phase 0)

The diagram above is the target. The code maps it onto crates like this, and each crate may only call the crate directly below it:

```
game ──► engine ──┬──► render ───► wgpu     (Renderer row)
                  ├──► physics ──► glam     (Physics/collision box)
                  ├──► script ───► mlua     (Scripting VM box)
                  ├──► bsp ──────► glam     (map loading; Resources box)
                  └──► platform ─► winit    (Platform row)
base: foundation utilities shared by all    (logging, asserts, Vfs, watcher)
```

- The Engine core row lives in `crates/engine`. Today that is the loop, input, the asset registry, debug hooks, the world and entities, and the walker. Collision math (`crates/physics`), the Lua host (`crates/script`) and the Quake map reader (`crates/bsp`) are their own crates beside `render` and `platform`. Audio and navigation will be added the same way.
- The Game layer is `game/`. It uses only the `engine` API.
- The filesystem sits in `base::Vfs`, not in the platform layer.

### Core principles
1. **Fixed-timestep simulation** (e.g. 60 Hz). Rendering interpolates between ticks. This keeps physics stable and makes replays and networking possible later.
2. **Engine knows nothing about guns.** "Weapon", "health", and "enemy" are game-layer concepts.
3. **All world state is serializable from day one.** Survival horror needs save rooms or save-anywhere, and retrofitting serialization is painful.
4. **Hot reload** for textures, shaders, scripts, and maps. This speeds up iteration enormously.
5. **Data-driven entities.** Entity types are defined in data and scripts, and exposed to TrenchBroom through a generated FGD file.

---

## 5. World and level pipeline

### Authoring
- **TrenchBroom** for brush geometry and entity placement (Valve 220 `.map` format).
- **Blender** (or AI tools plus cleanup) for detail meshes and props, exported as glTF.
- Props are placed in TrenchBroom as point entities that reference glTF files.

### Compiling (our own tool, `mapc`)
1. Parse the `.map` file into brushes defined by planes.
2. Convert brushes into convex polygons by intersecting their planes.
3. Run CSG to remove hidden faces between touching brushes.
4. Generate collision data: keep the convex brushes themselves as collision volumes.
5. Build visibility data (see below).
6. Bake lightmaps (see Lighting).
7. Write the result to **our own level format**: meshes, materials, collision brushes, entities, lightmaps, visibility.

> Shortcut for early phases: compile with **ericw-tools** to Quake BSP and write a BSP loader. This gives a playable world quickly. Replace it with `mapc` in Phase 3, because BSP format limits (vertex precision, lightmap size, texture rules) would cap the "expand later" goal. **In place** (Phase 1), with brushes read from ericw-tools' BSPX brush list rather than Quake's fixed hulls.

### Visibility
- Phase 1: frustum culling only.
- Later: **portal-based visibility**. Rooms and doorways are authored, or derived automatically from the brush structure. This suits the room-and-corridor layouts typical of survival horror, and doors can close portals off dynamically.
- A PVS like Quake's is an option, but portals are more flexible with dynamic doors.

---

## 6. Lighting roadmap

Lighting is built in tiers. Each tier is additive, and nothing is thrown away.

| Tier | Feature | Notes |
|---|---|---|
| L0 | Fullbright + flat shading | For debugging early phases. |
| L1 | **Baked lightmaps** | Classic look. Our own baker (simple direct light plus a few bounces) or ericw-tools early on. ✅ Done with ericw-tools (Phase 1). |
| L2 | **Dynamic point and spot lights**, forward shading | Muzzle flashes, flickering lamps, a few lights per object. |
| L3 | **Shadow maps for dynamic lights** | Essential for horror: a flashlight that casts real shadows. Spot-light shadow maps first, then point-light cubemaps. |
| L4 | **Light probes / irradiance volumes** | Lets dynamic objects (enemies, props) receive baked lighting, so they match the lightmapped world. |
| L5 | **Clustered forward shading** | Scales to many lights per scene. |
| L6 | Normal maps, specular, emissive | Optional move toward richer materials as the art style develops. |
| L7 | Stretch goals | SSAO, bloom, baked GI improvements, maybe directional lightmaps. |

**Design requirement from day one:** the material and shader system must take a "lighting inputs" structure (lightmap UV, probe data, a dynamic light list), so new tiers plug in without rewriting materials.

---

## 7. Fog roadmap

| Tier | Feature | Notes |
|---|---|---|
| F0 | **Distance fog** (linear / exponential / exp²) | In every lit shader from the start. Per-level color, density, and range. ✅ Done (set per frame). |
| F1 | **Height fog** | Exponential falloff with altitude. Good for low-lying mist. |
| F2 | **Fog volumes as brush entities** | A `fog_volume` entity in TrenchBroom. Fog parameters blend as the player enters a volume. |
| F3 | **Volumetric fog** (froxel grid) | Light scattering from dynamic lights and the flashlight, with light shafts through fog. The signature horror effect. Needs L3 shadow maps. |
| F4 | Animated noise, fog particles | Drifting, swirling fog. |

The fog result also feeds the retro post-pass: quantizing fog to the palette with dithering gives the classic banded look.

---

## 8. Geometry and asset scalability

- **Meshes:** 32-bit indices, no artificial vertex limits, glTF as the interchange format.
- **Textures:** any power-of-two size. The retro look comes from authoring textures at 64–256 px, not from engine limits. Store textures in standard formats; convert to GPU-compressed formats (BC/ASTC) later if needed.
- **Materials:** described in data files (e.g. TOML or JSON): shader, textures, flags such as alpha-test, emissive, or "no fog".
- **Characters:** skeletal animation (from glTF skins) as the main path. Optionally support vertex-frame animation for a deliberately retro look.
- **LOD:** not needed at first. The mesh format leaves room for LOD levels.

---

## 9. Gameplay flexibility

The engine provides generic systems. The game layer decides what kind of game it is.

### Entity system
- Simple, cache-friendly design: entities are IDs with component arrays (light ECS) or a flat struct with optional components. Avoid over-engineering. **Decided: flat struct** (`engine::Entity`), in generational slots.
- Components: transform, mesh, collider, character controller, light, audio emitter, trigger, script. **Built:** transform (position, rotation), model, collider, mover, trigger, usable. The character controller is the separate `Walker`, owned by the game.
- Entities are spawned from map data. Entity types are defined in script and data. **Today:** spawned in code (`game/src/level.rs`); they talk through named targets (`World::fire`).

### Character controller (swappable profiles)
| Profile | Feel |
|---|---|
| **Arena** | Quake-like: fast, air control, strafe-jumping. |
| **Survival** | Slower, weighty, head bob, stamina, no air control. |
| **Custom** | Parameters exposed to script. |

Underneath all of them is the same collision code: box or capsule traces against brushes and meshes, with slide and step-up. **Built:** Arena and Survival as `WalkSettings::arena()` / `survival()`; Custom is any `WalkSettings` value (public fields), with script access to come. Box traces against brushes only; capsules and meshes later.

### Gameplay systems (game layer, mostly script)
- Inventory with limited slots, item combining, and key items.
- Interaction system: "use" raycasts, examine objects, read notes.
- Doors, locks, keys, puzzles, triggers, and scripted sequences.
- Weapons, including ammo scarcity and reloading.
- Save system: save rooms or checkpoints, using the serialization from section 4.
- AI: navmesh pathfinding (Recast/Detour) plus behavior trees or state machines in script. Perception: sight cones, hearing via sound events, and light-level awareness (useful for stealth or horror).

### Scripting
- **Lua** embedded, with a narrow, documented engine API. **Built:** Lua 5.4 through `mlua`, in its own `script` crate; the API is documented in that crate and in the README.
- Scripts own entity behavior, triggers, AI decisions, and level events. **Today:** entity behavior and level events (hooks plus `world.fire`); AI needs read access to world state first.
- Hot-reloadable. **Built:** saving a script re-runs it; classes update in place and entities keep their state.

---

## 10. Audio

Audio matters a great deal in horror, so it gets planned work, not leftover time.
- Positional 3D audio with distance attenuation.
- Occlusion: dampen sounds behind walls (raycast against level geometry).
- Reverb zones as brush entities, alongside fog volumes.
- Ambient soundscapes and music layers that respond to game state.
- Library: originally SDL3 audio plus a small mixer of our own, or miniaudio. With the Rust stack the candidates are cpal plus our own mixer, or kira. Not yet decided (section 15); it is needed by Phase 4.

---

## 11. Phased plan

Each phase ends with something runnable.

### Phase 0 — Foundations (1–2 weeks) — ✅ done
- ✅ Project layout, build system (Cargo workspace).
- ✅ CI on all three OSes (first run in Phase 0.5, once the GitHub remote existed).
- ✅ Window, input, fixed-timestep loop (winit instead of SDL3).
- ✅ GPU device setup, clear screen, render geometry.
- ✅ Logging, asserts, file and resource loading.
- ✅ Hot-reload watcher: watches all assets. Shaders, textures and palettes reload through the asset registry; scripts and maps will join it.
- ✅ **Milestone:** a spinning colored cube at low resolution with nearest-neighbor upscale. Verified with a captured frame on Apple M1 (Metal).
- Beyond scope, also done: screenshot capture, a headless renderer with pixel-checking GPU tests, generational mesh handles, enforced layer boundaries, README.

### Phase 0.5 — Follow-ups (days) — ✅ done
Found in review. Each is cheaper now than once Phase 1 builds on top of it.
- ✅ Pushed to GitHub; CI green on Linux, Windows and macOS. GPU tests run on all three (Linux on lavapipe).
- ✅ Typed errors (`thiserror`) in the library crates, so callers can tell "no GPU" from "bad shader". `anyhow` stays in `game`.
- ✅ Lint settings in a root `[workspace.lints]` table instead of per-crate attributes. `unsafe_code` is forbidden there too.
- ✅ `publish = false` on internal crates.
- ✅ Toolchain pinned with `rust-toolchain.toml`, so local and CI clippy agree.
- Skipped for now (optional): prefix crate names (e.g. `retro-render`) and switch `passes/mod.rs` to `passes.rs`.
- ✅ **Milestone:** CI green on all three OSes.

### Phase 1 — First world (2–4 weeks) — ✅ done
- ✅ Fly camera: `engine::FlyCamera`, with mouse look through cursor capture (click to capture, Esc to release) and arrow-key look as a fallback.
- ✅ Textured meshes, nearest-neighbor sampling, texture hot reload through the asset registry. PNG only.
- ✅ Mipmaps as a config toggle (`Video::mipmaps`: off, nearest, linear). Default nearest, as Quake's software renderer did. Chains are built on the CPU at upload, averaged in linear light.
- ✅ Distance fog (F0): `frame.fog`, linear / exp / exp², background in the fog color.
- ✅ Retro post-pass: palette quantization and ordered dither (`frame.quantize`: off, nearest, dithered). Its own low-res pass (`post.wgsl`) rather than inside `blit.wgsl`, so captures and headless tests see it and the dither stays on low-res pixels. Default 256-color palette in `assets/palettes/default.png`.
- ✅ Load a map: ericw-tools BSP loader as the shortcut. `crates/bsp` reads BSP29/BSP2 plus the BSPX BRUSHLIST (qbsp `-wrbrushes`), so the engine collides with the map's real brushes instead of Quake's three fixed hulls. `engine::Gfx::load_map` spawns the same `Brush`es and `Entity`s the sandbox uses, so collision, the walker, doors and scripts work on maps unchanged. Test map `maps/test.map` (three rooms, two doors, a button, triggers, stairs, a scripted trigger) is compiled with `tools/compile-map.sh` and committed as `assets/maps/test.bsp`; `cargo run -p game -- maps/test.bsp` plays it. Textures are `assets/textures/<name>.png` by name: qbsp `-notex -path assets` reads their sizes, and the .bsp holds no pixels. Not yet: button travel, linked doors, door spawnflags, map hot reload.
- ✅ Baked lightmaps (L1): style-0 grayscale lightmaps read per face (sizes match ericw `light` exactly; a test checks they tile the lump), packed into one atlas, drawn bilinear with 2× overbright. Faces no light reached draw black. Not yet: colored (RGBLIGHTING/.lit), switchable styles, LMSHIFT.
- ✅ Reference-image test: the test map from its start, 320×180, no fog or palette, against `crates/engine/reference/test-map.png` with GPU-tolerant thresholds; `UPDATE_REFERENCE=1` rewrites it.
- ✅ **Milestone:** fly through a lit, fogged TrenchBroom level that looks like 1997. Met with the compiled test map (`cargo run -p game -- maps/test.bsp`, F3 to fly); not yet with a map authored in TrenchBroom itself.

### Phase 2 — Walking around (2–4 weeks) — ✅ done
Built against hand-placed brushes, since there is no map loader yet. A BSP or `mapc` loader will produce the same `Brush`es and `Entity`s.
- ✅ Collision: box traces against convex brushes, slide, step-up, snap-down (`crates/physics`). Non-axial brush edges stop a box slightly early (no edge bevels), as in Quake.
- ✅ Character controller with Arena and Survival profiles (`Walker`, `WalkSettings`). No crouch yet.
- ✅ Entity system: flat struct with optional components (decided over a light ECS), in generational slots. Models and colliders are lists, since map brush entities are often several brushes and textures. ✅ Spawning from map entities (see Phase 1's map loader).
- ✅ Doors, buttons, triggers via named targets.
- ✅ Lua integration (`mlua` approved 2026-10-25): entity classes with event hooks, firing names (now or delayed), opening and closing movers directly, reading entity positions, mover states, the player and the time, logging, hot reload, sandbox, instruction budget. The sandbox's secret button is all Lua (counts presses, closes the panel behind a player who wanders off). Not yet: `WalkSettings` exposed to scripts for the Custom profile; spawning or despawning entities from Lua.
- Follow-ups: moving platforms that carry the player (movers only block today); serializing `World` for saves (core principle 3; needs a serialization approach, e.g. serde, to be approved).
- ✅ **Milestone:** walk through a level, open doors, trigger events. Works in the sandbox level and in a compiled Quake map (`maps/test.map`); a GPU test walks a walker up the map's stairs. Not yet tried with a TrenchBroom-authored map.

### Phase 3 — Own toolchain (3–5 weeks)
- `mapc`: .map → own level format (CSG, collision, entities).
- Own lightmap baker (direct light, then bounces).
- glTF prop loading, props placed from TrenchBroom.
- Generate the FGD file from entity definitions.
- **Milestone:** the BSP dependency is gone, and levels mix brushes with detailed props.

### Phase 4 — Dynamic lighting and atmosphere (4–6 weeks)
- Dynamic point and spot lights (L2).
- Flashlight with shadow maps (L3).
- Light probes for dynamic objects (L4).
- Height fog and fog volumes (F1, F2).
- Positional audio and reverb zones.
- **Milestone:** a dark corridor, a flashlight with real shadows, mist on the floor, echoing footsteps.

### Phase 5 — Living world (4–8 weeks)
- Skeletal animation.
- Navmesh generation and pathfinding.
- AI framework: perception, behavior trees or state machines.
- Weapons, damage, health.
- Inventory and interaction systems.
- Save and load.
- **Milestone:** a vertical slice — one level, one enemy type, items, a save point.

### Phase 6 — Polish and expansion (ongoing)
- Volumetric fog (F3), clustered lighting (L5), material upgrades (L6).
- Portal visibility.
- Particles, decals (blood, bullet holes).
- Menus, settings, key rebinding.
- Performance profiling and an in-game debug overlay.

> The time estimates assume part-time solo work and are rough. Phases 0–2 are the most predictable. Phase 5 is where scope can grow, so define the vertical slice tightly before starting it.

---

## 12. Tooling and debugging

Build these early; they pay for themselves.
- In-game console: commands, variables (cvars), and level loading.
- Debug draw: lines, boxes, collision hulls, light radii, navmesh, portals.
- Frame timing overlay (CPU and GPU).
- Screenshot and frame-capture hooks (RenderDoc on Linux/Windows, Xcode GPU capture on macOS). 🟡 Screenshots are done (F12, `ENGINE_CAPTURE_FRAME`); GPU-debugger capture hooks are not.
- Map reload without restarting the game.
- ✅ Shader hot reload, and a headless renderer for automated image tests.

---

## 13. AI-assisted content (optional)

| Area | Use | Caveat |
|---|---|---|
| Textures | Generate tileable materials, then downscale and palette-quantize | Keep a consistent style guide and palette. |
| Props | Meshy / Tripo / Rodin / Hunyuan3D for static props | Needs cleanup: topology, UVs, scale, collision. |
| Characters | Concept art and base meshes | Rigging and animation still mostly manual. |
| Levels | LLM-written scripts that generate `.map` blockouts and place entities | Level design itself (flow, pacing, scares) stays hand-made. |
| Code | Boilerplate, parsers, tooling | Review everything. Keep the architecture under your own control. |

Check storefront rules: Steam requires disclosure of AI-generated content.

---

## 14. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Engine work crowds out game work | Hard milestones per phase. Reach the vertical slice before polish. |
| Lighting complexity grows | Tiered lighting plan. Don't start L5+ before the vertical slice. |
| Collision bugs (stuck in walls, falling through floors) | Brush-based convex collision, extensive debug drawing, automated "walk the level" tests. (Started: walker tests on brushes, and on the compiled test map.) |
| Survival-horror scope creep (inventory, puzzles, AI) | Keep gameplay in scripts. Prototype quickly, cut aggressively. |
| macOS GPU quirks | Develop on Mac (primary OS) and test via CI on the others from day one. |
| Asset bottleneck as the only artist | Strong low-res style guide. AI-assisted textures. Reuse assets in modular kits. |

---

## 15. Open decisions

- [x] Language: **Rust + wgpu + winit** (decided 2026-10-23).
- [ ] Audio library: cpal plus our own mixer, or kira. Needed by Phase 4.
- [x] Scripting binding: **mlua with vendored Lua 5.4** (decided 2026-10-25).
- [ ] Navigation: Recast bindings or a Rust port. Needed by Phase 5.
- [x] Entity model: **flat struct with optional components** (decided 2026-10-25).
- [ ] Gameplay direction: action FPS, survival horror, or a hybrid. This can wait until Phase 5.
- [ ] Save model: save rooms, checkpoints, or save anywhere.
- [ ] How far to push the retro look: strict 256-color palette, or a "retro-inspired" look with modern lighting. Both are now runtime settings (`frame.quantize`), so this can be decided by eye.

---

## 16. References

- Michael Abrash, *Graphics Programming Black Book* — Quake chapters.
- Fabien Sanglard, Quake / Quake 2 / Quake 3 source code reviews.
- id Software source: `github.com/id-Software/Quake`, `Quake-2`, `Quake-III-Arena`.
- TrenchBroom documentation — entity definitions (FGD), map formats.
- ericw-tools documentation — compiler and light tool options.
- *Real-Time Rendering* (Akenine-Möller et al.) — shadows, fog, clustered shading.
- Sébastien Hillaire, "Physically Based and Unified Volumetric Rendering in Frostbite" — volumetric fog.
- Recast/Detour documentation — navmesh generation.
