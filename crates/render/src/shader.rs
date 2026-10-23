//! Error-checked GPU object creation, so hot reload can reject a bad
//! shader instead of crashing.

use anyhow::{Result, bail};

/// Runs `create`, returning any validation error it raised.
pub(crate) fn checked<T>(
    device: &wgpu::Device,
    label: &str,
    create: impl FnOnce() -> T,
) -> Result<T> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = create();

    if let Some(err) = pollster::block_on(scope.pop()) {
        bail!("{label}: {err}");
    }

    Ok(value)
}

pub(crate) fn compile(
    device: &wgpu::Device,
    label: &str,
    source: &str,
) -> Result<wgpu::ShaderModule> {
    checked(device, label, || {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        })
    })
}
