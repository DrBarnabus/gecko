use anyhow::Result;

pub enum Frame {
    // Frame acquired, bool = surface should be reconfigured after present.
    Ready(wgpu::SurfaceTexture, bool),
    // Transient state (lost/outdated/occluded); skip this frame.
    Skip,
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub surface_config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    pub fn new(target: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(target)?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
            force_fallback_adapter: false,
        }))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = [wgpu::TextureFormat::Bgra8UnormSrgb, wgpu::TextureFormat::Rgba8UnormSrgb]
            .into_iter()
            .find(|f| capabilities.formats.contains(f))
            .unwrap_or(capabilities.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface,
            surface_config,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;

            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn acquire_frame(&mut self) -> Result<Frame> {
        Ok(match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Frame::Ready(frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Frame::Ready(frame, true),
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.surface_config);
                Frame::Skip
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Frame::Skip,
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("surface acquisition failed with a validation error")
            }
        })
    }
}
