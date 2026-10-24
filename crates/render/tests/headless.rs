//! End-to-end renderer checks on a real GPU, without a window: draw a
//! frame, read it back, inspect pixels. Skipped when no GPU exists.

mod common;

use base::Image;
use common::{HEIGHT, WIDTH, quad, renderer};
use glam::{Mat4, Vec3};
use render::{
    Camera, Draw, FrameView, MeshId, RenderConfig, RenderError, Renderer, Shader, TextureId,
};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0; 4];
const CENTER: (u32, u32) = (WIDTH / 2, HEIGHT / 2);

/// The unit quad at the test camera spans ~±18 px around the center, so
/// ±9 px lands in the middle of each texel of a 2×2 texture.
const QUADRANT: u32 = 9;

fn camera() -> Camera {
    Camera::look_at(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, 60f32.to_radians())
}

fn draw(renderer: &mut Renderer, draws: &[Draw]) -> Image {
    renderer.render(&FrameView {
        camera: &camera(),
        draws,
    });
    renderer.capture().unwrap()
}

fn at(mesh: MeshId, model: Mat4) -> Draw {
    Draw {
        mesh,
        texture: None,
        model,
    }
}

fn textured(mesh: MeshId, texture: TextureId) -> Draw {
    Draw {
        mesh,
        texture: Some(texture),
        model: Mat4::IDENTITY,
    }
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
        "return vec4<f32>(base.rgb * shade, base.a);",
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
