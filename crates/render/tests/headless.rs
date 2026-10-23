//! End-to-end renderer checks on a real GPU, without a window: draw a
//! frame, read it back, inspect pixels. Skipped when no GPU exists.

mod common;

use common::{HEIGHT, WIDTH, quad, renderer};
use glam::{Mat4, Vec3};
use render::{
    Camera, Capture, Draw, FrameView, MeshId, RenderConfig, RenderError, Renderer, Shader,
};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const CENTER: (u32, u32) = (WIDTH / 2, HEIGHT / 2);

fn camera() -> Camera {
    Camera::look_at(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, 60f32.to_radians())
}

fn draw(renderer: &mut Renderer, draws: &[Draw]) -> Capture {
    renderer.render(&FrameView {
        camera: &camera(),
        draws,
    });
    renderer.capture().unwrap()
}

fn at(mesh: MeshId, model: Mat4) -> Draw {
    Draw { mesh, model }
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

    assert_eq!((shot.width, shot.height), (WIDTH, HEIGHT));
    assert!(shot.rgba.as_chunks::<4>().0.iter().all(|px| *px == clear));
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
        "return vec4<f32>(in.color.rgb * shade, in.color.a);",
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
            common::skip_or_fail(err);
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
