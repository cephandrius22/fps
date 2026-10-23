//! Camera. The renderer supplies aspect, since it owns the target size.

use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec3};

/// Near clip plane, meters. Small enough for a held weapon model.
pub const DEFAULT_NEAR: f32 = 0.05;
/// Far clip plane, meters.
pub const DEFAULT_FAR: f32 = 1000.0;

/// Viewpoint and lens. World is right-handed, Y up.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// World → view transform (camera looks down its -Z).
    pub view: Mat4,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    /// Near clip distance.
    pub near: f32,
    /// Far clip distance.
    pub far: f32,
}

impl Camera {
    /// Camera at `eye` facing `target`, world Y up.
    pub fn look_at(eye: Vec3, target: Vec3, fov_y: f32) -> Self {
        Self {
            view: view::look_at_mat4(eye, target, Vec3::Y),
            fov_y,
            near: DEFAULT_NEAR,
            far: DEFAULT_FAR,
        }
    }

    pub(crate) fn view_proj(&self, aspect: f32) -> Mat4 {
        // wgpu NDC matches D3D: depth 0..1, Y up.
        let proj = proj::directx::perspective(self.fov_y, aspect, self.near, self.far);
        proj * self.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOV: f32 = std::f32::consts::FRAC_PI_2;
    const EPS: f32 = 1e-4;

    fn camera() -> Camera {
        Camera::look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, FOV)
    }

    #[test]
    fn target_projects_to_screen_center() {
        let ndc = camera().view_proj(1.0).project_point3(Vec3::ZERO);

        assert!(ndc.x.abs() < EPS && ndc.y.abs() < EPS);
        assert!(ndc.z > 0.0 && ndc.z < 1.0);
    }

    #[test]
    fn near_and_far_map_to_depth_zero_and_one() {
        let cam = camera();
        let vp = cam.view_proj(1.0);
        let near = vp.project_point3(Vec3::new(0.0, 0.0, 5.0 - cam.near));
        let far = vp.project_point3(Vec3::new(0.0, 0.0, 5.0 - cam.far));

        assert!(near.z.abs() < EPS);
        assert!((far.z - 1.0).abs() < EPS);
    }

    #[test]
    fn world_up_is_screen_up() {
        let ndc = camera().view_proj(1.0).project_point3(Vec3::Y);
        assert!(ndc.y > 0.0);
    }
}
