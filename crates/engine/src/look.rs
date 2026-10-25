//! First-person look, shared by every camera controller: yaw and pitch
//! from the mouse or the arrow keys, and the camera built from them.
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
pub(crate) const MAX_PITCH: f32 = 89.0_f32.to_radians();

/// Whether mouse motion steers the camera this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseLook {
    /// Mouse motion turns the camera (cursor captured).
    Enabled,
    /// Mouse motion is ignored (cursor free for UI).
    Disabled,
}

/// Look tuning and lens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookSettings {
    /// Radians per unit of raw mouse motion.
    pub mouse_sensitivity: f32,
    /// Radians per second for arrow-key turning.
    pub key_turn_speed: f32,
    /// Vertical field of view, radians.
    pub fov_y: f32,
}

impl Default for LookSettings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.0025,
            key_turn_speed: 2.0,
            fov_y: 75.0_f32.to_radians(),
        }
    }
}

/// View direction. Yaw is unwrapped, so interpolating across ±180°
/// never spins the long way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Look {
    pub yaw: f32,
    pub pitch: f32,
}

impl Look {
    pub fn new(yaw: f32, pitch: f32) -> Self {
        Self {
            yaw,
            pitch: pitch.clamp(-MAX_PITCH, MAX_PITCH),
        }
    }

    /// Applies one tick of mouse (if enabled) and arrow-key turning.
    pub fn turn(&mut self, input: &Input, mouse: MouseLook, settings: &LookSettings, dt: f32) {
        let mut yaw = 0.0;
        let mut pitch = 0.0;

        // Mouse right / down turns right / down: both lower the angle.
        if mouse == MouseLook::Enabled {
            let (dx, dy) = input.mouse_delta();
            yaw -= dx * settings.mouse_sensitivity;
            pitch -= dy * settings.mouse_sensitivity;
        }

        let key_turn = settings.key_turn_speed * dt;
        yaw += key_turn * input.axis(Key::Left, Key::Right);
        pitch += key_turn * input.axis(Key::Up, Key::Down);

        self.yaw += yaw;
        self.pitch = (self.pitch + pitch).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Unit view direction.
    pub fn forward(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();

        Vec3::new(-sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
    }

    /// Forward and right along the ground, ignoring pitch: walking
    /// directions.
    pub fn ground_axes(&self) -> (Vec3, Vec3) {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let forward = Vec3::new(-sin_yaw, 0.0, -cos_yaw);
        let right = Vec3::new(cos_yaw, 0.0, -sin_yaw);

        (forward, right)
    }

    pub fn lerp(&self, to: &Look, t: f32) -> Look {
        Look {
            yaw: self.yaw + (to.yaw - self.yaw) * t,
            pitch: self.pitch + (to.pitch - self.pitch) * t,
        }
    }

    /// Render camera at `eye` looking this way.
    pub fn camera(&self, eye: Vec3, settings: &LookSettings) -> Camera {
        Camera {
            view: view::look_to_mat4(eye, self.forward(), Vec3::Y),
            fov_y: settings.fov_y,
            near: DEFAULT_NEAR,
            far: DEFAULT_FAR,
        }
    }
}

#[cfg(test)]
mod tests {
    use platform::{Event, KeyState};

    use super::*;

    const DT: f32 = 0.1;
    const EPS: f32 = 1e-4;

    fn holding(key: Key) -> Input {
        let mut input = Input::default();
        input.apply(Event::Key {
            key,
            state: KeyState::Pressed,
        });
        input
    }

    fn moved_mouse(dx: f32, dy: f32) -> Input {
        let mut input = Input::default();
        input.apply(Event::MouseMotion { dx, dy });
        input
    }

    fn turned(input: &Input, mouse: MouseLook) -> Look {
        let mut look = Look::new(0.0, 0.0);
        look.turn(input, mouse, &LookSettings::default(), DT);
        look
    }

    #[test]
    fn faces_negative_z_at_zero() {
        assert!((Look::new(0.0, 0.0).forward() - Vec3::NEG_Z).length() < EPS);
    }

    #[test]
    fn mouse_right_turns_right_and_down_looks_down() {
        let look = turned(&moved_mouse(100.0, 100.0), MouseLook::Enabled);

        assert!(look.forward().x > 0.0, "turned left: {:?}", look.forward());
        assert!(look.forward().y < 0.0, "looked up: {:?}", look.forward());
    }

    #[test]
    fn mouse_ignored_when_disabled() {
        let look = turned(&moved_mouse(100.0, 100.0), MouseLook::Disabled);
        assert_eq!(look, Look::new(0.0, 0.0));
    }

    #[test]
    fn arrow_keys_turn_at_key_speed() {
        let look = turned(&holding(Key::Right), MouseLook::Disabled);

        assert!((look.yaw + LookSettings::default().key_turn_speed * DT).abs() < EPS);
        assert!(turned(&holding(Key::Up), MouseLook::Disabled).pitch > 0.0);
    }

    #[test]
    fn pitch_is_clamped() {
        let look = turned(&moved_mouse(0.0, -1e6), MouseLook::Enabled);

        assert!((look.pitch - MAX_PITCH).abs() < EPS);
        assert!(Look::new(0.0, 10.0).pitch <= MAX_PITCH);
    }

    #[test]
    fn ground_axes_ignore_pitch_and_stay_level() {
        let look = Look::new(0.3, -1.2);
        let (forward, right) = look.ground_axes();

        assert_eq!(forward.y, 0.0);
        assert!((forward.length() - 1.0).abs() < EPS);
        assert!(forward.dot(right).abs() < EPS);
        // Right is forward turned clockwise seen from above.
        assert!((forward.cross(right) - Vec3::NEG_Y).length() < EPS);
    }
}
