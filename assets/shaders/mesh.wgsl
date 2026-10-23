// Opaque mesh shader. Lighting tier L0: vertex color + fixed sun term.
// Hot-reloadable: edit and save while the game runs.

struct Frame {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> models: array<mat4x4<f32>>;

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
};

@vertex
fn vs_main(v: VsIn, @builtin(instance_index) instance: u32) -> VsOut {
    let model = models[instance];

    var out: VsOut;
    out.clip = frame.view_proj * model * vec4<f32>(v.pos, 1.0);
    out.normal = (model * vec4<f32>(v.normal, 0.0)).xyz;
    out.color = v.color;
    return out;
}

const SUN_DIR = vec3<f32>(0.4, 1.0, 0.3);
const AMBIENT = 0.35;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let sun = max(dot(n, normalize(SUN_DIR)), 0.0);
    let shade = AMBIENT + (1.0 - AMBIENT) * sun;

    return vec4<f32>(in.color.rgb * shade, in.color.a);
}
