// Retro post-pass, at low res: snaps each pixel to the nearest palette
// color, optionally after an ordered dither. Works on sRGB-encoded
// values (see target.rs): palettes are authored that way, and output
// pixels come out byte-exact palette colors. Hot-reloadable.

// Must match `PostUniforms` in passes/post.rs.
struct Post {
    mode: u32,
    count: u32,
    colors: array<vec4<f32>, 256>,
};

// Must match `quantize_mode` in passes/post.rs.
const QUANTIZE_OFF = 0u;
const QUANTIZE_NEAREST = 1u;
const QUANTIZE_DITHERED = 2u;

// Dither offsets span about one palette step. A 256-color palette in
// ramps of 16 shades steps ~1/16 between neighbors.
const DITHER_SPREAD = 1.0 / 16.0;
const BAYER_SIZE = 4u;
const BAYER_CELLS = 16.0;

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<uniform> post: Post;

// One triangle covering the target.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

// Offset in -0.5..0.5 from the 4×4 Bayer matrix: neighbors get very
// different thresholds, so flat areas turn into an even pattern.
fn bayer(pixel: vec2<u32>) -> f32 {
    var matrix = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    let cell = (pixel.y % BAYER_SIZE) * BAYER_SIZE + pixel.x % BAYER_SIZE;
    return (matrix[cell] + 0.5) / BAYER_CELLS - 0.5;
}

// Brute-force search: 256 compares per pixel is cheap at 640×360. If
// the internal resolution grows, bake this into a 3D lookup texture.
fn nearest(rgb: vec3<f32>) -> vec3<f32> {
    var best = post.colors[0].rgb;
    var best_dist = 3.0e38;

    for (var i = 0u; i < post.count; i++) {
        let delta = post.colors[i].rgb - rgb;
        let dist = dot(delta, delta);
        if dist < best_dist {
            best = post.colors[i].rgb;
            best_dist = dist;
        }
    }

    return best;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(pos.xy);
    let color = textureLoad(scene, pixel, 0);

    // No palette yet: pass through rather than paint everything one color.
    if post.mode == QUANTIZE_OFF || post.count == 0u {
        return color;
    }

    var rgb = color.rgb;
    if post.mode == QUANTIZE_DITHERED {
        rgb += bayer(pixel) * DITHER_SPREAD;
    }

    return vec4<f32>(nearest(rgb), color.a);
}
