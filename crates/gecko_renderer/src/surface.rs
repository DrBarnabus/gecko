use anyhow::Result;
use gecko_rhi::Rhi;

pub enum Frame {
    // Frame acquired, bool = surface should be reconfigured after present.
    Ready(wgpu::SurfaceTexture, bool),
    // Transient state (lost/outdated/occluded); skip this frame.
    Skip,
}

pub struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Surface {
    pub fn new(rhi: &Rhi, surface: wgpu::Surface<'static>, width: u32, height: u32) -> Self {
        let adapter = rhi.adapter();
        let device = rhi.device();
        let queue = rhi.queue();

        let capabilities = surface.get_capabilities(&adapter);
        let format = [wgpu::TextureFormat::Bgra8UnormSrgb, wgpu::TextureFormat::Rgba8UnormSrgb]
            .into_iter()
            .find(|f| capabilities.formats.contains(f))
            .unwrap_or(capabilities.formats[0]);

        let config = wgpu::SurfaceConfiguration {
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

        surface.configure(&device, &config);

        Self {
            surface,
            config,
            device,
            queue,
        }
    }

    #[inline]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.config.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.config.height
    }

    #[tracing::instrument(skip_all)]
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;

            self.surface.configure(&self.device, &self.config);
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }

    #[tracing::instrument(skip_all)]
    pub fn acquire_frame(&mut self) -> Result<Frame> {
        Ok(match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Frame::Ready(frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Frame::Ready(frame, true),
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.reconfigure();

                Frame::Skip
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Frame::Skip,
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("surface acquisition failed with a validation error")
            }
        })
    }

    #[tracing::instrument(skip_all, fields(reconfigure))]
    pub fn present(&self, frame: wgpu::SurfaceTexture, reconfigure: bool) {
        self.queue.present(frame);

        if reconfigure {
            self.reconfigure();
        }
    }
}
