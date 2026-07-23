use dear_imgui_rs::TextureId;
use dear_imgui_wgpu::WgpuRenderer;
use gecko_rhi::{Rhi, target::RenderTarget};

pub struct Viewport {
    pub target: RenderTarget,
    pub texture_id: TextureId,
    pub size: (u32, u32),
    pub desired: (u32, u32),
}

impl Viewport {
    pub fn new(rhi: &mut Rhi, renderer: &mut WgpuRenderer, format: wgpu::TextureFormat) -> Self {
        let size = (1280, 720);
        let target = RenderTarget::color_depth(rhi, "viewport", to_extent(size), format);

        let texture = rhi
            .registry()
            .texture(target.presented().handle)
            .expect("target texture is valid");
        let texture_id = renderer.register_external_texture(&texture.texture, &texture.view);

        Self {
            target,
            texture_id,
            size,
            desired: size,
        }
    }

    pub fn aspect(&self) -> f32 {
        self.size.0 as f32 / self.size.1.max(1) as f32
    }

    pub fn apply_resize(&mut self, rhi: &mut Rhi, renderer: &mut WgpuRenderer) {
        if self.desired == self.size || self.desired.0 == 0 || self.desired.1 == 0 {
            return;
        }

        self.target.replace(rhi, "viewport", to_extent(self.desired));

        self.size = self.desired;

        let color_view = rhi
            .registry()
            .texture_view(self.target.presented().handle)
            .expect("target texture is valid");
        renderer.update_external_texture_view(self.texture_id, color_view);

        tracing::debug!(width = self.size.0, height = self.size.1, "game viewport resized");
    }
}

fn to_extent(size: (u32, u32)) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    }
}
