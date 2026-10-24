//! Free-flying camera for exploring levels: moves where it looks, no
//! collision or gravity. Updated per tick, interpolated per frame.
//!
//! ```text
//! yaw   turns about world +Y (positive = left)
//! pitch tilts up/down, clamped short of straight up/down
//!
//!   +Y        forward at yaw 0, pitch 0 is -Z
//!    │  ╱ -Z
//!    │ ╱
//!    └──── +X
//! ```

use glam::Vec3;
use glam::camera::rh::view;

use crate::gfx::{Camera, DEFAULT_FAR, DEFAULT_NEAR};
use crate::input::Input;
use base::Key;

/// Stops short of ±90°, where yaw and forward become degenerate.
const MAX_PITCH: f32 = 89.0_f32.to_radians();

/// Whether mouse motion steers the camera this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseLook {
    /// Mouse motion turns the camera (cursor captured).
    Enabled,
    /// Mouse motion is ignored (cursor free for UI).
    Disabled,
}

/// Movement and look tuning.
#[derive(Clone, Copy, Debug)]
pub struct FlySettings {
    /// Meters per second.
    pub speed: f32,
    /// Speed multiplier while Left Shift is held.
    pub fast_multiplier: f32,
    /// Radians per unit of raw mouse motion.
    pub mouse_sensitivity: f32,
    /// Radians per second for arrow-key turning.
    pub key_turn_speed: f32,
    /// Vertical field of view, radians.
    pub fov_y: f32,
}

impl Default for FlySettings {
    fn default() -> Self {
        Self {
            speed: 5.0,
            fast_multiplier: 4.0,
            mouse_sensitivity: 0.0025,
            key_turn_speed: 2.0,
            fov_y: 75.0_f32.to_radians(),
        }
    }
}

/// Position and orientation at one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    position: Vec3,
    /// Unwrapped, so interpolating across ±180° never spins the long way.
    yaw: f32,
    pitch: f32,
}

impl Pose {
    fn forward(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();

        Vec3::new(-sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
    }

    fn lerp(&self, to: &Pose, t: f32) -> Pose {
        Pose {
            position: self.position.lerp(to.position, t),
            yaw: self.yaw + (to.yaw - self.yaw) * t,
            pitch: self.pitch + (to.pitch - self.pitch) * t,
        }
    }
}

/// WASD to move, Space / Left Ctrl for up / down, Left Shift for speed,
/// mouse or arrow keys to look.
pub struct FlyCamera {
    /// Tuning; change freely between ticks.
    pub settings: FlySettings,
    prev: Pose,
    current: Pose,
}

impl FlyCamera {
    /// Camera at `position`, facing `yaw` / `pitch` radians.
    pub fn new(position: Vec3, yaw: f32, pitch: f32) -> Self {
        let pose = Pose {
            position,
            yaw,
            pitch: pitch.clamp(-MAX_PITCH, MAX_PITCH),
        };

        Self {
            settings: FlySettings::default(),
            prev: pose,
            current: pose,
        }
    }

    /// Advances one tick from input.
    pub fn update(&mut self, input: &Input, look: MouseLook, dt: f32) {
        self.prev = self.current;
        self.turn(input, look, dt);
        self.fly(input, dt);
    }

    /// Current position (latest tick, not interpolated).
    pub fn position(&self) -> Vec3 {
        self.current.position
    }

    /// Unit view direction (latest tick).
    pub fn forward(&self) -> Vec3 {
        self.current.forward()
    }

    /// Render camera between the last two ticks; pass `frame.alpha`.
    pub fn camera(&self, alpha: f32) -> Camera {
        let pose = self.prev.lerp(&self.current, alpha);

        Camera {
            view: view::look_to_mat4(pose.position, pose.forward(), Vec3::Y),
            fov_y: self.settings.fov_y,
            near: DEFAULT_NEAR,
            far: DEFAULT_FAR,
        }
    }

    fn turn(&mut self, input: &Input, look: MouseLook, dt: f32) {
        let s = &self.settings;
        let mut yaw = 0.0;
        let mut pitch = 0.0;

        // Mouse right / down turns right / down: both lower the angle.
        if look == MouseLook::Enabled {
            let (dx, dy) = input.mouse_delta();
            yaw -= dx * s.mouse_sensitivity;
            pitch -= dy * s.mouse_sensitivity;
        }

        let key_turn = s.key_turn_speed * dt;
        yaw += key_turn * axis(input, Key::Left, Key::Right);
        pitch += key_turn * axis(input, Key::Up, Key::Down);

        self.current.yaw += yaw;
        self.current.pitch = (self.current.pitch + pitch).clamp(-MAX_PITCH, MAX_PITCH);
    }

    fn fly(&mut self, input: &Input, dt: f32) {
        let forward = self.current.forward();
        let right = forward.cross(Vec3::Y).normalize();

        let wish = forward * axis(input, Key::W, Key::S)
            + right * axis(input, Key::D, Key::A)
            + Vec3::Y * axis(input, Key::Space, Key::LCtrl);

        // Normalized so diagonals are not faster than straight lines.
        let Some(direction) = wish.try_normalize() else {
            return;
        };

        let mut speed = self.settings.speed;
        if input.is_down(Key::LShift) {
            speed *= self.settings.fast_multiplier;
        }

        self.current.position += direction * speed * dt;
    }
}

/// +1 while `positive` is held, -1 for `negative`, 0 for both or neither.
fn axis(input: &Input, positive: Key, negative: Key) -> f32 {
    let held = |key| f32::from(u8::from(input.is_down(key)));
    held(positive) - held(negative)
}

#[cfg(test)]
mod tests {
    use platform::{Event, KeyState};

    use super::*;

    const DT: f32 = 0.1;
    const EPS: f32 = 1e-4;

    fn holding(keys: &[Key]) -> Input {
        let mut input = Input::default();
        for &key in keys {
            input.apply(Event::Key {
                key,
                state: KeyState::Pressed,
            });
        }
        input
    }

    fn moved_mouse(dx: f32, dy: f32) -> Input {
        let mut input = Input::default();
        input.apply(Event::MouseMotion { dx, dy });
        input
    }

    fn origin_cam() -> FlyCamera {
        FlyCamera::new(Vec3::ZERO, 0.0, 0.0)
    }

    #[test]
    fn faces_negative_z_by_default() {
        assert!((origin_cam().forward() - Vec3::NEG_Z).length() < EPS);
    }

    #[test]
    fn w_moves_forward_at_speed() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::W]), MouseLook::Disabled, DT);

        let expected = Vec3::NEG_Z * cam.settings.speed * DT;
        assert!((cam.position() - expected).length() < EPS);
    }

    #[test]
    fn strafe_and_vertical_follow_right_and_world_up() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::D]), MouseLook::Disabled, DT);
        assert!(cam.position().x > 0.0);

        let mut cam = origin_cam();
        cam.update(&holding(&[Key::Space]), MouseLook::Disabled, DT);
        assert!(cam.position().y > 0.0);
    }

    #[test]
    fn diagonal_is_not_faster() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::W, Key::D]), MouseLook::Disabled, DT);

        assert!((cam.position().length() - cam.settings.speed * DT).abs() < EPS);
    }

    #[test]
    fn shift_multiplies_speed() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::W, Key::LShift]), MouseLook::Disabled, DT);

        let fast = cam.settings.speed * cam.settings.fast_multiplier * DT;
        assert!((cam.position().length() - fast).abs() < EPS);
    }

    #[test]
    fn opposite_keys_cancel() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::W, Key::S]), MouseLook::Disabled, DT);

        assert_eq!(cam.position(), Vec3::ZERO);
    }

    #[test]
    fn mouse_right_turns_right_and_down_looks_down() {
        let mut cam = origin_cam();
        cam.update(&moved_mouse(100.0, 100.0), MouseLook::Enabled, DT);

        assert!(cam.forward().x > 0.0, "turned left: {:?}", cam.forward());
        assert!(cam.forward().y < 0.0, "looked up: {:?}", cam.forward());
    }

    #[test]
    fn mouse_ignored_when_look_disabled() {
        let mut cam = origin_cam();
        cam.update(&moved_mouse(100.0, 100.0), MouseLook::Disabled, DT);

        assert!((cam.forward() - Vec3::NEG_Z).length() < EPS);
    }

    #[test]
    fn arrow_keys_turn_without_mouse() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::Right]), MouseLook::Disabled, DT);

        assert!(cam.forward().x > 0.0);
    }

    #[test]
    fn pitch_is_clamped() {
        let mut cam = origin_cam();
        cam.update(&moved_mouse(0.0, -1e6), MouseLook::Enabled, DT);

        assert!((cam.current.pitch - MAX_PITCH).abs() < EPS);
        assert!(FlyCamera::new(Vec3::ZERO, 0.0, 10.0).current.pitch <= MAX_PITCH);
    }

    #[test]
    fn camera_interpolates_between_ticks() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::W]), MouseLook::Disabled, DT);

        let eye = |alpha| {
            cam.camera(alpha)
                .view
                .inverse()
                .transform_point3(Vec3::ZERO)
        };
        let step = cam.position();

        assert!(eye(0.0).length() < EPS);
        assert!((eye(0.5) - step * 0.5).length() < EPS);
        assert!((eye(1.0) - step).length() < EPS);
    }
}
