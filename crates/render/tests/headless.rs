//! End-to-end renderer checks on a real GPU, without a window: draw a
//! frame, read it back, inspect pixels. Skipped when no GPU exists.

mod common;

use base::Image;
use common::{HEIGHT, WIDTH, quad, renderer, renderer_with};
use glam::{Mat4, Vec3};
use render::{
    Camera, Draw, Falloff, Fog, FrameView, MeshId, Mipmaps, Quantize, RenderConfig, RenderError,
    Renderer, Shader, TextureId,
};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0; 4];
const CENTER: (u32, u32) = (WIDTH / 2, HEIGHT / 2);

/// The unit quad at the test camera spans ~±18 px around the center, so
/// ±9 px lands in the middle of each texel of a 2×2 texture.
const QUADRANT: u32 = 9;
/// Camera to quad center, meters.
const QUAD_DISTANCE: f32 = 3.0;
const FOG_BLUE: [f32; 3] = [0.0, 0.0, 1.0];
/// Linear 0.214 encodes to sRGB ~0.5: halfway between black and white
/// in the encoded space the palette pass works in.
const MID_GRAY: [f32; 3] = [0.214; 3];

fn camera() -> Camera {
    let eye = Vec3::new(0.0, 0.0, QUAD_DISTANCE);
    Camera::look_at(eye, Vec3::ZERO, 60f32.to_radians())
}

fn draw(renderer: &mut Renderer, draws: &[Draw]) -> Image {
    draw_with(renderer, draws, None, Quantize::Off)
}

fn draw_with(
    renderer: &mut Renderer,
    draws: &[Draw],
    fog: Option<Fog>,
    quantize: Quantize,
) -> Image {
    renderer.render(&FrameView {
        camera: &camera(),
        draws,
        fog,
        quantize,
    });
    renderer.capture().unwrap()
}

fn fog(color: [f32; 3], falloff: Falloff) -> Option<Fog> {
    Some(Fog { color, falloff })
}

/// Fog so thick everything past 1 m is solid `color`, background too.
fn solid_fog(color: [f32; 3]) -> Option<Fog> {
    fog(
        color,
        Falloff::Linear {
            start: 0.0,
            end: 1.0,
        },
    )
}

fn center(shot: &Image) -> [u8; 4] {
    shot.pixel(CENTER.0, CENTER.1).unwrap()
}

fn palette(colors: &[[u8; 3]]) -> Image {
    let rgba = colors
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect();
    Image::new(colors.len() as u32, 1, rgba).unwrap()
}

/// 1-pixel black/white checker, `size`² pixels.
fn checker(size: u32) -> Image {
    let rgba = (0..size * size)
        .flat_map(|i| {
            let on = (i % size + i / size).is_multiple_of(2);
            [if on { 255 } else { 0 }; 3].into_iter().chain([255])
        })
        .collect();
    Image::new(size, size, rgba).unwrap()
}

fn at(mesh: MeshId, model: Mat4) -> Draw {
    Draw {
        mesh,
        texture: None,
        lightmap: None,
        model,
    }
}

fn textured(mesh: MeshId, texture: TextureId) -> Draw {
    Draw {
        mesh,
        texture: Some(texture),
        lightmap: None,
        model: Mat4::IDENTITY,
    }
}

fn lit(mesh: MeshId, lightmap: TextureId) -> Draw {
    Draw {
        lightmap: Some(lightmap),
        ..at(mesh, Mat4::IDENTITY)
    }
}

/// Lightmap of gray luxels in a row, opaque.
fn luxels(values: &[u8]) -> Image {
    let rgba = values.iter().flat_map(|&v| [v, v, v, 255]).collect();
    Image::new(values.len() as u32, 1, rgba).unwrap()
}

/// 2×2 texture: red, green on top; blue, white below.
fn four_texels() -> Image {
    #[rustfmt::skip]
    let rgba = vec![
        255, 0, 0, 255,    0, 255, 0, 255,
        0, 0, 255, 255,    255, 255, 255, 255,
    ];
    Image::new(2, 2, rgba).unwrap()
}

fn is_blue(px: [u8; 4]) -> bool {
    px[2] > 100 && px[0] < 30 && px[1] < 30
}

fn is_white(px: [u8; 4]) -> bool {
    px[..3].iter().all(|&c| c > 100)
}

fn is_red(px: [u8; 4]) -> bool {
    px[0] > 100 && px[1] < 30 && px[2] < 30
}

fn is_green(px: [u8; 4]) -> bool {
    px[1] > 100 && px[0] < 30 && px[2] < 30
}

#[test]
fn empty_frame_is_uniform_clear_color() {
    let Some(mut r) = renderer() else {
        return;
    };

    let shot = draw(&mut r, &[]);
    let clear = shot.pixel(0, 0).unwrap();

    assert_eq!((shot.width(), shot.height()), (WIDTH, HEIGHT));
    assert!(shot.rgba().as_chunks::<4>().0.iter().all(|px| *px == clear));
    // Near-black linear clear; sRGB encoding lifts 0.02 to ~39.
    assert!(
        clear[..3].iter().all(|&c| c < 64),
        "clear too bright: {clear:?}"
    );
}

#[test]
fn front_face_covers_center_only() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));

    let shot = draw(&mut r, &[at(mesh, Mat4::from_scale(Vec3::splat(0.5)))]);

    assert!(is_red(shot.pixel(CENTER.0, CENTER.1).unwrap()));
    assert!(!is_red(shot.pixel(0, 0).unwrap()));
}

#[test]
fn back_face_is_culled() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));

    let turned = Mat4::from_rotation_y(std::f32::consts::PI);
    let shot = draw(&mut r, &[at(mesh, turned)]);

    assert!(!is_red(shot.pixel(CENTER.0, CENTER.1).unwrap()));
}

#[test]
fn nearer_surface_wins_regardless_of_order() {
    let Some(mut r) = renderer() else {
        return;
    };
    let near = r.create_mesh(&quad(GREEN));
    let far = r.create_mesh(&quad(RED));

    let near_at = Mat4::from_translation(Vec3::new(0.0, 0.0, 0.5));
    let far_at = Mat4::from_translation(Vec3::new(0.0, 0.0, -0.5));
    let shot = draw(&mut r, &[at(near, near_at), at(far, far_at)]);

    assert!(is_green(shot.pixel(CENTER.0, CENTER.1).unwrap()));
}

#[test]
fn bad_shader_reload_keeps_old_pipeline() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));

    let err = r
        .reload_shader(Shader::Mesh, "this is not wgsl")
        .unwrap_err();
    assert!(matches!(err, RenderError::Validation { .. }), "{err:?}");

    let shot = draw(&mut r, &[at(mesh, Mat4::IDENTITY)]);
    assert!(is_red(shot.pixel(CENTER.0, CENTER.1).unwrap()));
}

#[test]
fn good_shader_reload_takes_effect() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));

    // Same interface as mesh.wgsl, but paints everything green.
    let green = common::load_shader(Shader::Mesh).unwrap().replace(
        "return vec4<f32>(apply_fog(lit, in.world), base.a);",
        "return vec4<f32>(0.0, 1.0, 0.0, 1.0);",
    );
    r.reload_shader(Shader::Mesh, &green).unwrap();

    let shot = draw(&mut r, &[at(mesh, Mat4::IDENTITY)]);
    assert!(is_green(shot.pixel(CENTER.0, CENTER.1).unwrap()));
}

#[test]
fn destroy_mesh_invalidates_handle_once() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));

    assert!(r.destroy_mesh(mesh));
    assert!(!r.destroy_mesh(mesh));

    // Slot reuse must not resurrect the old handle.
    let reused = r.create_mesh(&quad(GREEN));
    assert_ne!(mesh, reused);
}

#[test]
fn failed_shader_load_names_the_shader() {
    let missing = |_| Err::<String, _>(std::io::Error::other("gone"));
    let _gpu = render::test_support::lock_gpu();
    let err = match Renderer::headless(RenderConfig::default(), missing) {
        Ok(_) => panic!("renderer built without shader source"),
        Err(err @ RenderError::NoAdapter(_)) => {
            render::test_support::skip_or_fail(err);
            return;
        }
        Err(err) => err,
    };

    assert!(
        matches!(
            err,
            RenderError::ShaderSource {
                shader: Shader::Mesh,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn texture_maps_each_texel_to_its_quadrant() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(WHITE));
    let texture = r.create_texture(&four_texels()).unwrap();

    let shot = draw(&mut r, &[textured(mesh, texture)]);
    let px = |dx: i32, dy: i32| {
        let x = CENTER.0.saturating_add_signed(dx);
        let y = CENTER.1.saturating_add_signed(dy);
        shot.pixel(x, y).unwrap()
    };
    let q = QUADRANT as i32;

    assert!(is_red(px(-q, -q)), "top-left {:?}", px(-q, -q));
    assert!(is_green(px(q, -q)), "top-right {:?}", px(q, -q));
    assert!(is_blue(px(-q, q)), "bottom-left {:?}", px(-q, q));
    assert!(is_white(px(q, q)), "bottom-right {:?}", px(q, q));
}

/// Linear filtering would blend neighbors near the texel edge; nearest
/// keeps each side pure.
#[test]
fn nearest_sampling_keeps_texel_edges_hard() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(WHITE));
    let texture = r.create_texture(&four_texels()).unwrap();

    let shot = draw(&mut r, &[textured(mesh, texture)]);
    let row = CENTER.1 - QUADRANT;

    assert!(is_red(shot.pixel(CENTER.0 - 2, row).unwrap()));
    assert!(is_green(shot.pixel(CENTER.0 + 2, row).unwrap()));
}

#[test]
fn replace_texture_updates_existing_handle() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(WHITE));
    let texture = r.create_texture(&four_texels()).unwrap();

    // Different size on purpose: hot reload may change dimensions.
    r.replace_texture(texture, &Image::solid([0, 255, 0, 255]))
        .unwrap();

    let shot = draw(&mut r, &[textured(mesh, texture)]);
    assert!(is_green(shot.pixel(CENTER.0, CENTER.1).unwrap()));
}

#[test]
fn destroyed_texture_handle_is_rejected() {
    let Some(mut r) = renderer() else {
        return;
    };
    let texture = r.create_texture(&four_texels()).unwrap();

    assert!(r.destroy_texture(texture));
    assert!(!r.destroy_texture(texture));

    let err = r.replace_texture(texture, &four_texels()).unwrap_err();
    assert!(matches!(err, RenderError::UnknownTexture), "{err:?}");
}

#[test]
fn oversized_texture_is_rejected() {
    let Some(mut r) = renderer() else {
        return;
    };
    // Far beyond any GPU's 2D limit (8192–16384 today).
    let width = 1 << 16;
    let image = Image::new(width, 1, vec![0; width as usize * 4]).unwrap();

    let err = r.create_texture(&image).unwrap_err();
    assert!(
        matches!(err, RenderError::TextureTooLarge { .. }),
        "{err:?}"
    );
}

/// A 64-texel checker squeezed into ~9 px: without mips neighboring
/// pixels hit different texels (black next to lit white); with mips a
/// smaller level has averaged the checker to one flat gray.
#[test]
fn mipmaps_average_minified_textures() {
    const INNER: u32 = 2;
    let tiny = Mat4::from_scale(Vec3::splat(0.25));
    let sample = |mipmaps| {
        let mut r = renderer_with(mipmaps)?;
        let mesh = r.create_mesh(&quad(WHITE));
        let texture = r.create_texture(&checker(64)).unwrap();
        let small = Draw {
            model: tiny,
            ..textured(mesh, texture)
        };
        let shot = draw(&mut r, &[small]);

        // Red channel of the quad's inner 5×5 pixels.
        let inner = (CENTER.1 - INNER..=CENTER.1 + INNER)
            .flat_map(|y| (CENTER.0 - INNER..=CENTER.0 + INNER).map(move |x| (x, y)))
            .map(|(x, y)| shot.pixel(x, y).unwrap()[0])
            .collect::<Vec<_>>();
        Some(inner)
    };

    let Some(off) = sample(Mipmaps::Off) else {
        return;
    };
    let mipped = sample(Mipmaps::Nearest).unwrap();

    let distinct = |px: &[u8]| px.iter().collect::<std::collections::BTreeSet<_>>().len();
    assert!(distinct(&off) > 1 && off.contains(&0), "off: {off:?}");
    assert_eq!(distinct(&mipped), 1, "mipped: {mipped:?}");
    assert!(mipped[0] > 0, "mipped to black: {mipped:?}");
}

#[test]
fn solid_fog_hides_geometry_and_colors_background() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));

    let shot = draw_with(
        &mut r,
        &[at(mesh, Mat4::IDENTITY)],
        solid_fog(FOG_BLUE),
        Quantize::Off,
    );

    assert_eq!(center(&shot), [0, 0, 255, 255]);
    assert_eq!(shot.pixel(0, 0), Some([0, 0, 255, 255]));
}

#[test]
fn fog_blends_partway_inside_linear_band() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));
    let band = Falloff::Linear {
        start: QUAD_DISTANCE - 1.0,
        end: QUAD_DISTANCE + 1.0,
    };

    let shot = draw_with(
        &mut r,
        &[at(mesh, Mat4::IDENTITY)],
        fog(FOG_BLUE, band),
        Quantize::Off,
    );
    let px = center(&shot);

    // Half lit red, half fog blue.
    assert!(px[0] > 60 && px[0] < 250, "red share {px:?}");
    assert!(px[2] > 100 && px[2] < 250, "blue share {px:?}");
}

#[test]
fn thin_exponential_fog_barely_tints() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));
    let thin = fog(FOG_BLUE, Falloff::Exp2 { density: 0.001 });

    let clear = draw(&mut r, &[at(mesh, Mat4::IDENTITY)]);
    let fogged = draw_with(&mut r, &[at(mesh, Mat4::IDENTITY)], thin, Quantize::Off);

    assert_eq!(center(&fogged), center(&clear));
}

#[test]
fn nearest_quantize_outputs_only_palette_colors() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));
    let colors = [[10, 20, 30], [200, 50, 50]];
    r.set_palette(&palette(&colors)).unwrap();

    let shot = draw_with(&mut r, &[at(mesh, Mat4::IDENTITY)], None, Quantize::Nearest);

    assert_eq!(center(&shot), [200, 50, 50, 255]);
    for &[r, g, b, a] in shot.rgba().as_chunks::<4>().0 {
        assert!(
            colors.contains(&[r, g, b]) && a == 255,
            "{:?}",
            [r, g, b, a]
        );
    }
}

#[test]
fn quantize_is_a_no_op_without_palette_or_when_off() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&quad(RED));
    let draws = [at(mesh, Mat4::IDENTITY)];

    let plain = draw(&mut r, &draws);
    assert_eq!(draw_with(&mut r, &draws, None, Quantize::Nearest), plain);

    r.set_palette(&palette(&[[0, 0, 0]])).unwrap();
    assert_eq!(draw(&mut r, &draws), plain);
}

/// A flat mid-gray between black and white: nearest picks one for
/// every pixel; the dither splits it into a repeating 4×4 pattern.
#[test]
fn dither_breaks_flat_color_into_4x4_pattern() {
    let Some(mut r) = renderer() else {
        return;
    };
    r.set_palette(&palette(&[[0, 0, 0], [255, 255, 255]]))
        .unwrap();
    let gray = solid_fog(MID_GRAY);

    let banded = draw_with(&mut r, &[], gray, Quantize::Nearest);
    let banded_px = banded.rgba().as_chunks::<4>().0;
    assert!(banded_px.iter().all(|px| *px == banded_px[0]));

    let dithered = draw_with(&mut r, &[], gray, Quantize::Dithered);
    let px = |x, y| dithered.pixel(x, y).unwrap();
    let whites = (0..4)
        .flat_map(|y| (0..4).map(move |x| (x, y)))
        .filter(|&(x, y)| px(x, y)[0] == 255)
        .count();

    assert!((4..=12).contains(&whites), "{whites} of 16 white");
    for (x, y) in [(0, 0), (1, 2), (3, 3)] {
        assert_eq!(px(x, y), px(x + 4, y + 8), "pattern repeats every 4 px");
    }
}

#[test]
fn bad_post_shader_is_rejected() {
    let Some(mut r) = renderer() else {
        return;
    };

    let err = r.reload_shader(Shader::Post, "fn broken(").unwrap_err();
    assert!(matches!(err, RenderError::Validation { .. }), "{err:?}");
}

/// Quake overbright: luxel 128 is full texture brightness, whatever the
/// sun term would have said.
#[test]
fn lightmap_value_128_is_full_brightness() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&common::lit_quad(WHITE));
    let lightmap = r.create_lightmap(&luxels(&[128])).unwrap();

    let px = center(&draw(&mut r, &[lit(mesh, lightmap)]));
    assert!(px[..3].iter().all(|&c| c >= 250), "{px:?}");
}

/// Four luxels across the quad, dark, dark, bright, bright: each half
/// takes its own pair's light, blending only near the middle. (Pairs,
/// because a single luxel per half would blend toward the other a little
/// at any off-center sample, and near black that shows.)
#[test]
fn lightmap_follows_lightmap_uvs() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mesh = r.create_mesh(&common::lit_quad(WHITE));
    let lightmap = r.create_lightmap(&luxels(&[0, 0, 128, 128])).unwrap();

    let shot = draw(&mut r, &[lit(mesh, lightmap)]);
    let left = shot.pixel(CENTER.0 - QUADRANT, CENTER.1).unwrap();
    let right = shot.pixel(CENTER.0 + QUADRANT, CENTER.1).unwrap();

    assert!(left[0] < 20, "dark luxel: {left:?}");
    assert!(right[0] > 240, "bright luxel: {right:?}");
}
