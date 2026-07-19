pub mod context;

use crate::context::{Capabilities, Context, ContextConfig};

#[derive(Debug, thiserror::Error)]
pub enum RhiError {
    #[error("failed to create a GPU surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),

    #[error("failed to acquire a GPU adapter: {0}")]
    RequestAdapter(#[from] wgpu::RequestAdapterError),

    #[error("failed to create the GPU device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),

    #[error("adapter does not meet required limit `{name}` (required: {required}, available: {available})")]
    UnmetLimit {
        name: &'static str,
        required: u32,
        available: u32,
    },
}

pub struct Rhi {
    context: Context,
}

impl Rhi {
    pub fn new(
        config: &ContextConfig,
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>,
    ) -> Result<(Self, wgpu::Surface<'static>), RhiError> {
        let (context, raw_surface) = Context::new(config, surface_target)?;

        Ok((Self { context }, raw_surface))
    }

    #[inline]
    pub fn context(&self) -> &Context {
        &self.context
    }

    #[inline]
    pub fn capabilities(&self) -> &Capabilities {
        self.context.capabilities()
    }

    /// Clone of the instance, for imgui initialization only.
    #[inline]
    pub fn instance(&self) -> wgpu::Instance {
        self.context.instance().clone()
    }

    /// Clone of the adapter, for imgui initialization only.
    #[inline]
    pub fn adapter(&self) -> wgpu::Adapter {
        self.context.adapter().clone()
    }

    /// Clone of the device, for imgui initialization only.
    #[inline]
    pub fn device(&self) -> wgpu::Device {
        self.context.device().clone()
    }

    /// Clone of the queue, for imgui initialization only.
    #[inline]
    pub fn queue(&self) -> wgpu::Queue {
        self.context.queue().clone()
    }
}
