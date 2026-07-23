use slotmap::{SlotMap, new_key_type};
use wgpu::util::DeviceExt;

use crate::{
    conventions::MAX_COLOR_ATTACHMENTS,
    target::{RenderTarget, ResolvedTarget},
};

new_key_type! {
    pub struct TextureHandle;
    pub struct BufferHandle;
}

pub struct TextureResource {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub size: wgpu::Extent3d,
    pub usage: wgpu::TextureUsages,
}

pub struct BufferResource {
    pub buffer: wgpu::Buffer,
    pub size: u64,
    pub usage: wgpu::BufferUsages,
}

#[derive(Default)]
pub struct ResourceRegistry {
    textures: SlotMap<TextureHandle, TextureResource>,
    buffers: SlotMap<BufferHandle, BufferResource>,
}

impl ResourceRegistry {
    // --- texture ---------------------------------------------------------------------------------

    pub(crate) fn create_texture(&mut self, device: &wgpu::Device, desc: &wgpu::TextureDescriptor) -> TextureHandle {
        let texture = device.create_texture(desc);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.textures.insert(TextureResource {
            texture,
            view,
            format: desc.format,
            size: desc.size,
            usage: desc.usage,
        })
    }

    #[inline]
    pub fn texture(&self, handle: TextureHandle) -> Option<&TextureResource> {
        self.textures.get(handle)
    }

    #[inline]
    pub fn texture_view(&self, handle: TextureHandle) -> Option<&wgpu::TextureView> {
        self.textures.get(handle).map(|t| &t.view)
    }

    pub(crate) fn replace_texture(
        &mut self,
        device: &wgpu::Device,
        handle: TextureHandle,
        desc: &wgpu::TextureDescriptor,
    ) -> bool {
        let Some(slot) = self.textures.get_mut(handle) else {
            return false;
        };

        let texture = device.create_texture(desc);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        *slot = TextureResource {
            texture,
            view,
            format: desc.format,
            size: desc.size,
            usage: desc.usage,
        };

        true
    }

    pub(crate) fn remove_texture(&mut self, handle: TextureHandle) -> bool {
        self.textures.remove(handle).is_some()
    }

    // --- render target ---------------------------------------------------------------------------

    pub fn resolve_target(&self, target: &RenderTarget) -> Option<ResolvedTarget<'_>> {
        let mut colors = [None; MAX_COLOR_ATTACHMENTS];
        for (i, color) in target.colors().enumerate() {
            colors[i] = Some(self.texture_view(color.handle)?);
        }

        let depth = match target.depth {
            Some(handle) => Some(self.texture_view(handle)?),
            None => None,
        };

        Some(ResolvedTarget {
            colors,
            color_count: target.color_count(),
            depth,
            extent: target.extent,
            sample_count: target.sample_count,
        })
    }

    // --- buffer ----------------------------------------------------------------------------------

    pub(crate) fn create_buffer(&mut self, device: &wgpu::Device, desc: &wgpu::BufferDescriptor) -> BufferHandle {
        let buffer = device.create_buffer(desc);

        self.buffers.insert(BufferResource {
            buffer,
            size: desc.size,
            usage: desc.usage,
        })
    }

    pub(crate) fn create_buffer_init(
        &mut self,
        device: &wgpu::Device,
        desc: &wgpu::util::BufferInitDescriptor,
    ) -> BufferHandle {
        let buffer = device.create_buffer_init(desc);
        let size = buffer.size();

        self.buffers.insert(BufferResource {
            buffer,
            size,
            usage: desc.usage,
        })
    }

    #[inline]
    pub fn buffer(&self, handle: BufferHandle) -> Option<&BufferResource> {
        self.buffers.get(handle)
    }

    #[inline]
    pub fn buffer_ref(&self, handle: BufferHandle) -> Option<&wgpu::Buffer> {
        self.buffers.get(handle).map(|b| &b.buffer)
    }

    pub(crate) fn remove_buffer(&mut self, handle: BufferHandle) -> bool {
        self.buffers.remove(handle).is_some()
    }
}
