pub mod context;
pub mod conventions;
pub mod frame;
pub mod resource;
pub mod target;

use std::num::NonZeroUsize;

use crate::{
    context::{Capabilities, Context, ContextConfig},
    frame::{FrameContext, FrameTiming, FrameUniform, FramesInFlight, frame_uniform_layout},
    resource::{BufferHandle, ResourceRegistry, TextureHandle},
    target::{RenderTarget, ResolvedTarget},
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
    registry: ResourceRegistry,

    frames: FramesInFlight,
    frames_in_flight: NonZeroUsize,
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
                registry: ResourceRegistry::default(),

                frames,
                frames_in_flight: config.frames_in_flight,
                frame_uniform_layout,
            },
            raw_surface,
        ))
    }

    // --- frame lifecycle -------------------------------------------------------------------------

    #[inline]
    pub fn frames_in_flight(&self) -> NonZeroUsize {
        self.frames_in_flight
    }

    #[tracing::instrument(skip_all)]
    pub fn begin_frame(&self, timing: FrameTiming) -> FrameContext<'_> {
        let frame_index = self.frames.frame_index();
        let slot_index = self.frames.slot_index();

        let frame_uniform = FrameUniform {
            frame_index: frame_index as u32,
            time: timing.time,
            delta_time: timing.delta_time,
        };

        let mut encoded = encase::UniformBuffer::new(Vec::new());
        encoded.write(&frame_uniform).expect("encode frame uniform");
        self.context
            .queue()
            .write_buffer(&self.frames.current().frame_uniform, 0, &encoded.into_inner());

        FrameContext::new(self, frame_index, slot_index, timing)
    }

    #[inline]
    pub fn frame_uniform_layout(&self) -> &wgpu::BindGroupLayout {
        &self.frame_uniform_layout
    }

    // --- texture ---------------------------------------------------------------------------------

    pub fn create_texture(&mut self, desc: &wgpu::TextureDescriptor) -> TextureHandle {
        self.registry.create_texture(self.context.device(), desc)
    }

    #[inline]
    pub fn texture_view(&self, handle: TextureHandle) -> Option<&wgpu::TextureView> {
        self.registry.texture_view(handle)
    }

    pub fn replace_texture(&mut self, handle: TextureHandle, desc: &wgpu::TextureDescriptor) -> bool {
        self.registry.replace_texture(self.context.device(), handle, desc)
    }

    pub fn destroy_texture(&mut self, handle: TextureHandle) -> bool {
        self.registry.remove_texture(handle)
    }

    // --- render target ---------------------------------------------------------------------------

    pub fn resolve_target(&self, target: &RenderTarget) -> Option<ResolvedTarget<'_>> {
        self.registry.resolve_target(target)
    }

    // --- buffer ----------------------------------------------------------------------------------

    pub fn create_buffer(&mut self, desc: &wgpu::BufferDescriptor) -> BufferHandle {
        self.registry.create_buffer(self.context.device(), desc)
    }

    pub fn create_buffer_init(&mut self, desc: &wgpu::util::BufferInitDescriptor) -> BufferHandle {
        self.registry.create_buffer_init(self.context.device(), desc)
    }

    pub fn upload_buffer(&self, handle: BufferHandle, offset: u64, data: &[u8]) -> bool {
        let Some(resource) = self.registry.buffer(handle) else {
            return false;
        };

        self.context.queue().write_buffer(&resource.buffer, offset, data);

        true
    }

    pub fn destroy_buffer(&mut self, handle: BufferHandle) -> bool {
        self.registry.remove_buffer(handle)
    }

    // --- accessors -------------------------------------------------------------------------------

    #[inline]
    pub fn context(&self) -> &Context {
        &self.context
    }

    #[inline]
    pub fn registry(&self) -> &ResourceRegistry {
        &self.registry
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
