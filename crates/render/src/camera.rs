//! Camera. The renderer supplies aspect, since it owns the target size.

use glam::camera::rh::{proj, view};
use glam::{Mat4, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub view: Mat4,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn look_at(eye: Vec3, target: Vec3, fov_y: f32) -> Self {
        Self {
            view: view::look_at_mat4(eye, target, Vec3::Y),
            fov_y,
            near: 0.05,
            far: 1000.0,
        }
    }

    pub(crate) fn view_proj(&self, aspect: f32) -> Mat4 {
        // wgpu NDC matches D3D: depth 0..1, Y up.
        let proj = proj::directx::perspective(self.fov_y, aspect, self.near, self.far);
        proj * self.view
    }
}
