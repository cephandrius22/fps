//! Free-flying camera for exploring levels: moves where it looks, no
//! collision or gravity. Updated per tick, interpolated per frame. Look
//! handling is shared with the walker (see `look.rs`).

use glam::Vec3;

use crate::gfx::Camera;
use crate::input::Input;
use crate::look::{Look, LookSettings, MouseLook};
use base::Key;

/// Movement and look tuning.
#[derive(Clone, Copy, Debug)]
pub struct FlySettings {
    /// Meters per second.
    pub speed: f32,
    /// Speed multiplier while Left Shift is held.
    pub fast_multiplier: f32,
    /// Mouse and arrow-key look, field of view.
    pub look: LookSettings,
}

impl Default for FlySettings {
    fn default() -> Self {
        Self {
            speed: 5.0,
            fast_multiplier: 4.0,
            look: LookSettings::default(),
        }
    }
}

/// Position and orientation at one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    position: Vec3,
    look: Look,
}

impl Pose {
    fn lerp(&self, to: &Pose, t: f32) -> Pose {
        Pose {
            position: self.position.lerp(to.position, t),
            look: self.look.lerp(&to.look, t),
        }
    }
}

/// Free-flying camera. Bindings, read every tick (speeds are
/// `FlySettings` defaults):
///
/// ```text
/// W / S           forward / back along the view, tilt included   5 m/s
/// A / D           strafe left / right, level                      5 m/s
/// Space / LCtrl   straight up / down                              5 m/s
/// Left Shift      held: all movement × 4
/// ← → / ↑ ↓       turn / look up-down                           2 rad/s
/// mouse           turn and look, only with MouseLook::Enabled
/// ```
///
/// Opposite keys cancel; diagonals are normalized, never faster.
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
            look: Look::new(yaw, pitch),
        };

        Self {
            settings: FlySettings::default(),
            prev: pose,
            current: pose,
        }
    }

    /// Advances one tick from input.
    pub fn update(&mut self, input: &Input, mouse: MouseLook, dt: f32) {
        self.prev = self.current;
        self.current
            .look
            .turn(input, mouse, &self.settings.look, dt);
        self.fly(input, dt);
    }

    /// Current position (latest tick, not interpolated).
    pub fn position(&self) -> Vec3 {
        self.current.position
    }

    /// Unit view direction (latest tick).
    pub fn forward(&self) -> Vec3 {
        self.current.look.forward()
    }

    /// Heading, radians (latest tick). 0 faces -Z; positive turns left.
    pub fn yaw(&self) -> f32 {
        self.current.look.yaw
    }

    /// Tilt, radians (latest tick). Positive looks up.
    pub fn pitch(&self) -> f32 {
        self.current.look.pitch
    }

    /// Render camera between the last two ticks; pass `frame.alpha`.
    pub fn camera(&self, alpha: f32) -> Camera {
        let pose = self.prev.lerp(&self.current, alpha);
        pose.look.camera(pose.position, &self.settings.look)
    }

    fn fly(&mut self, input: &Input, dt: f32) {
        let forward = self.current.look.forward();
        let right = forward.cross(Vec3::Y).normalize();

        let wish = forward * input.axis(Key::W, Key::S)
            + right * input.axis(Key::D, Key::A)
            + Vec3::Y * input.axis(Key::Space, Key::LCtrl);

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

    fn origin_cam() -> FlyCamera {
        FlyCamera::new(Vec3::ZERO, 0.0, 0.0)
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
    fn arrow_keys_turn_without_mouse() {
        let mut cam = origin_cam();
        cam.update(&holding(&[Key::Right]), MouseLook::Disabled, DT);

        assert!(cam.forward().x > 0.0);
        assert!(cam.yaw() < 0.0);
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
