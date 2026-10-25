//! Renderer failures. Setup errors are usually fatal; `Validation`
//! from a hot reload is not (the old shader stays active).

use base::BoxError;

use crate::Shader;

/// Why a renderer operation failed.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The window handle could not back a GPU surface.
    #[error("create surface")]
    Surface(#[from] wgpu::CreateSurfaceError),
    /// No GPU (or none able to present to the window).
    #[error("no compatible GPU adapter")]
    NoAdapter(#[from] wgpu::RequestAdapterError),
    /// The adapter refused to open a device.
    #[error("request GPU device")]
    Device(#[from] wgpu::RequestDeviceError),
    /// The adapter cannot present to this surface.
    #[error("surface unsupported by adapter")]
    SurfaceUnsupported,
    /// The caller's shader loader failed.
    #[error("load {shader:?} shader source")]
    ShaderSource {
        /// Which shader.
        shader: Shader,
        /// Loader's error.
        #[source]
        source: BoxError,
    },
    /// wgpu rejected a shader or pipeline. `message` holds wgpu's report,
    /// including source location for WGSL errors.
    #[error("{label}: {message}")]
    Validation {
        /// What was being created.
        label: String,
        /// wgpu's report.
        message: String,
    },
    /// Waiting on the GPU failed during readback.
    #[error("wait for GPU")]
    Poll(#[from] wgpu::PollError),
    /// The readback buffer could not be mapped.
    #[error("map readback buffer")]
    Map(#[from] wgpu::BufferAsyncError),
    /// The mapped readback range could not be accessed.
    #[error("access readback buffer")]
    MapRange(#[from] wgpu::MapRangeError),
    /// The image exceeds the GPU's texture size limit.
    #[error("texture {width}×{height} exceeds GPU limit {max}")]
    TextureTooLarge {
        /// Image width.
        width: u32,
        /// Image height.
        height: u32,
        /// Largest supported dimension.
        max: u32,
    },
    /// The palette image has more pixels than the shader holds.
    #[error("palette has {colors} colors, max {max}")]
    PaletteTooLarge {
        /// Pixels in the image.
        colors: usize,
        /// Largest supported palette.
        max: usize,
    },
    /// The texture handle was destroyed.
    #[error("unknown or destroyed texture")]
    UnknownTexture,
    /// wgpu dropped the map callback without calling it.
    #[error("readback callback dropped")]
    MapDropped,
}

pub(crate) type Result<T, E = RenderError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync + 'static>() {}

    /// Engine and game errors wrap this; it must cross threads.
    #[test]
    fn error_is_thread_safe() {
        assert_send_sync::<RenderError>();
    }

    #[test]
    fn shader_source_error_chains_loader_cause() {
        let err = RenderError::ShaderSource {
            shader: Shader::Mesh,
            source: "file missing".into(),
        };

        assert_eq!(
            base::chain(&err).to_string(),
            "load Mesh shader source: file missing"
        );
    }
}
