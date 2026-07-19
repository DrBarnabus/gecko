pub mod context;
pub mod frame;

use crate::{
    context::{Capabilities, Context, ContextConfig},
    frame::{FrameContext, FrameTiming, FrameUniform, FramesInFlight, frame_uniform_layout},
};

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
    frames: FramesInFlight,
    frame_uniform_layout: wgpu::BindGroupLayout,
}

impl Rhi {
    pub fn new(
        config: &ContextConfig,
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>,
    ) -> Result<(Self, wgpu::Surface<'static>), RhiError> {
        let (context, raw_surface) = Context::new(config, surface_target)?;

        let frame_uniform_layout = frame_uniform_layout(context.device());
        let frames = FramesInFlight::new(context.device(), config.frames_in_flight, &frame_uniform_layout);

        Ok((
            Self {
                context,
                frames,
                frame_uniform_layout,
            },
            raw_surface,
        ))
    }

    // --- frame lifecycle -------------------------------------------------------------------------

    #[tracing::instrument(skip_all)]
    pub fn begin_frame(&mut self, timing: FrameTiming) -> FrameContext<'_> {
        let frame_index = self.frames.frame_index();
        let slot_index = self.frames.slot_index();

        let frame_uniform = FrameUniform {
            frame_index: frame_index as u32,
            _pad: 0,
            time: timing.time,
            delta_time: timing.delta_time,
        };

        self.context.queue().write_buffer(
            &self.frames.current().frame_uniform,
            0,
            bytemuck::bytes_of(&frame_uniform),
        );

        FrameContext::new(self, frame_index, slot_index, timing)
    }

    // --- accessors -------------------------------------------------------------------------------

    #[inline]
    pub fn context(&self) -> &Context {
        &self.context
    }

    #[inline]
    pub fn capabilities(&self) -> &Capabilities {
        self.context.capabilities()
    }

    #[inline]
    pub fn frame_uniform_layout(&self) -> &wgpu::BindGroupLayout {
        &self.frame_uniform_layout
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
