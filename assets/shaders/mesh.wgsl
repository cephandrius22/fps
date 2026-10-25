// Opaque mesh shader: texture × vertex color × light, then distance fog
// (F0). Light is the baked lightmap (L1) when the draw has one, else a
// fixed sun term (L0). Hot-reloadable: edit and save while the game runs.

// Must match `FrameUniforms` in passes/mesh.rs.
struct Frame {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    fog_color: vec4<f32>,
    fog_mode: u32,
    fog_start: f32,
    fog_end: f32,
    fog_density: f32,
};

// Must match `fog_mode` in passes/mesh.rs.
const FOG_OFF = 0u;
const FOG_LINEAR = 1u;
const FOG_EXP = 2u;
const FOG_EXP2 = 3u;

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> models: array<mat4x4<f32>>;

// Untextured draws bind a 1×1 white texture, so this is always valid.
@group(1) @binding(0) var albedo: texture_2d<f32>;
@group(1) @binding(1) var albedo_sampler: sampler;

// Draws without a lightmap bind a 1×1 texture with alpha 0: "use the sun".
@group(2) @binding(0) var lightmap: texture_2d<f32>;
@group(2) @binding(1) var lightmap_sampler: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) lightmap_uv: vec2<f32>,
    @location(4) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) world: vec3<f32>,
    @location(4) lightmap_uv: vec2<f32>,
};

@vertex
fn vs_main(v: VsIn, @builtin(instance_index) instance: u32) -> VsOut {
    let model = models[instance];

    let world = model * vec4<f32>(v.pos, 1.0);

    var out: VsOut;
    out.clip = frame.view_proj * world;
    out.world = world.xyz;
    out.normal = (model * vec4<f32>(v.normal, 0.0)).xyz;
    out.color = v.color;
    out.uv = v.uv;
    out.lightmap_uv = v.lightmap_uv;
    return out;
}

const SUN_DIR = vec3<f32>(0.4, 1.0, 0.3);
const AMBIENT = 0.35;
// Quake lightmaps are gamma-space with 2× overbright: luxel 128 means
// full texture brightness. Stored as sRGB, 128 decodes to 0.216; this
// scales it back to 1.0 (and 255 to about 4.6, twice as bright in gamma).
const OVERBRIGHT = 4.63;
// Keeps a zero-width linear fog band from dividing by zero.
const MIN_FOG_BAND = 1e-4;

// Share of the surface still visible at distance `d`: 1 clear, 0 solid.
// Radial distance, not depth, so fog does not shift as the view turns.
fn visibility(d: f32) -> f32 {
    switch frame.fog_mode {
        case FOG_LINEAR: {
            let band = max(frame.fog_end - frame.fog_start, MIN_FOG_BAND);
            return clamp((frame.fog_end - d) / band, 0.0, 1.0);
        }
        case FOG_EXP: {
            return exp(-frame.fog_density * d);
        }
        case FOG_EXP2: {
            let x = frame.fog_density * d;
            return exp(-x * x);
        }
        case FOG_OFF, default: {
            return 1.0;
        }
    }
}

fn apply_fog(lit: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let d = distance(world, frame.eye.xyz);
    return mix(frame.fog_color.rgb, lit, visibility(d));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let sun = max(dot(n, normalize(SUN_DIR)), 0.0);
    let sun_shade = AMBIENT + (1.0 - AMBIENT) * sun;
    let baked = textureSample(lightmap, lightmap_sampler, in.lightmap_uv);
    // Alpha 1: lightmapped; alpha 0: the no-lightmap fallback.
    let shade = mix(vec3<f32>(sun_shade), baked.rgb * OVERBRIGHT, baked.a);
    let base = textureSample(albedo, albedo_sampler, in.uv) * in.color;

    let lit = base.rgb * shade;

    return vec4<f32>(apply_fog(lit, in.world), base.a);
}
