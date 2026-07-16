use dear_imgui_rs::TextureId;
use dear_imgui_wgpu::WgpuRenderer;
use gecko_renderer::DEPTH_FORMAT;

pub struct Viewport {
    pub color: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub texture_id: TextureId,
    pub size: (u32, u32),
    pub desired: (u32, u32),
}

impl Viewport {
    fn create_targets(device: &wgpu::Device, format: wgpu::TextureFormat, (w, h): (u32, u32)) -> (wgpu::Texture, wgpu::TextureView, wgpu::Texture, wgpu::TextureView) {
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };

        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());

        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

        (color, color_view, depth, depth_view)
    }

    pub fn new(device: &wgpu::Device, renderer: &mut WgpuRenderer, format: wgpu::TextureFormat) -> Self {
        let size = (1280, 720);
        let (color, color_view, depth, depth_view) = Self::create_targets(device, format, size);

        let texture_id = renderer.register_external_texture(&color, &color_view);

        Self { color, color_view, depth, depth_view, texture_id, size, desired: size }
    }

    pub fn aspect(&self) -> f32 {
        self.size.0 as f32 / self.size.1.max(1) as f32
    }

    pub fn apply_resize(&mut self, device: &wgpu::Device, renderer: &mut WgpuRenderer, format: wgpu::TextureFormat) {
        if self.desired == self.size || self.desired.0 == 0 || self.desired.1 == 0 {
            return;
        }

        let (color, color_view, depth, depth_view) = Self::create_targets(device, format, self.desired);

        self.color = color;
        self.color_view = color_view;
        self.depth = depth;
        self.depth_view = depth_view;
        self.size = self.desired;

        renderer.update_external_texture_view(self.texture_id, &self.color_view);

        tracing::debug!(width = self.size.0, height = self.size.1, "game viewport resized");
    }
}
