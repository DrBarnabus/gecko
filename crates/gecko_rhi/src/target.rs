use crate::{
    Rhi,
    conventions::{DEPTH_FORMAT, MAX_COLOR_ATTACHMENTS},
    extent_of,
    resource::TextureHandle,
};

const SAMPLE_COUNT: u32 = 1;

/// What an attachment means, so passes and sinks select by role rather than by index.
/// `Color` is the presented image the UI samples and a surface blit consumes; the rest are
/// reserved names, first constructed by the render paths that need them in Phase 4+.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentRole {
    Color,
    Normal,
    Motion,
    ObjectId,
}

#[derive(Clone, Copy, Debug)]
pub struct ColorAttachment {
    pub handle: TextureHandle,
    pub format: wgpu::TextureFormat,
    pub role: AttachmentRole,
    pub usage: wgpu::TextureUsages,
}

pub struct ColorSpec {
    pub format: wgpu::TextureFormat,
    pub role: AttachmentRole,
    pub usage: wgpu::TextureUsages,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderTarget {
    colors: [Option<ColorAttachment>; MAX_COLOR_ATTACHMENTS],
    color_count: usize,
    pub depth: Option<TextureHandle>,
    pub extent: wgpu::Extent3d,
    pub sample_count: u32,
}

impl RenderTarget {
    pub fn new(
        rhi: &mut Rhi,
        label: &str,
        extent: wgpu::Extent3d,
        attachments: &[ColorSpec],
        with_depth: bool,
    ) -> RenderTarget {
        let caps = rhi.capabilities();
        let max_color_attachments = caps.max_color_attachments as usize;
        let max_bytes_per_sample = caps.max_color_attachment_bytes_per_sample;

        assert!(
            attachments.len() <= max_color_attachments,
            "render target '{label}' declares {} color attachments but the device supports at most {max_color_attachments}; reduce the attachment count",
            attachments.len(),
        );

        let formats: Vec<wgpu::TextureFormat> = attachments.iter().map(|spec| spec.format).collect();
        let bytes_per_sample = bytes_per_sample(&formats);
        assert!(
            bytes_per_sample <= max_bytes_per_sample,
            "render target '{label}' needs {bytes_per_sample} color bytes per sample but the device supports at most {max_bytes_per_sample}; use narrower formats or fewer attachments",
        );

        let mut colors = [None; MAX_COLOR_ATTACHMENTS];
        for (i, color) in attachments.iter().enumerate() {
            let usage = color.usage | wgpu::TextureUsages::RENDER_ATTACHMENT;
            let handle = rhi.create_texture(&wgpu::TextureDescriptor {
                label: Some(&format!("{label}_color_{i}")),
                size: extent,
                mip_level_count: 1,
                sample_count: SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: color.format,
                usage,
                view_formats: &[],
            });

            colors[i] = Some(ColorAttachment {
                handle,
                format: color.format,
                role: color.role,
                usage,
            });
        }

        let depth = if with_depth {
            Some(rhi.create_texture(&wgpu::TextureDescriptor {
                label: Some(&format!("{label}_depth")),
                size: extent,
                mip_level_count: 1,
                sample_count: SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            }))
        } else {
            None
        };

        RenderTarget {
            colors,
            color_count: attachments.len(),
            depth,
            extent,
            sample_count: SAMPLE_COUNT,
        }
    }

    pub fn color_depth(
        rhi: &mut Rhi,
        label: &str,
        extent: wgpu::Extent3d,
        color_format: wgpu::TextureFormat,
    ) -> RenderTarget {
        Self::new(
            rhi,
            label,
            extent,
            &[ColorSpec {
                format: color_format,
                role: AttachmentRole::Color,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
            }],
            true,
        )
    }

    pub fn color_count(&self) -> usize {
        self.color_count
    }

    pub fn colors(&self) -> impl Iterator<Item = ColorAttachment> + '_ {
        self.colors[..self.color_count].iter().flatten().copied()
    }

    pub fn find(&self, role: AttachmentRole) -> Option<(usize, ColorAttachment)> {
        self.colors()
            .enumerate()
            .find(|(_, attachment)| attachment.role == role)
    }

    pub fn presented(&self) -> ColorAttachment {
        self.find(AttachmentRole::Color)
            .expect("target has a presented color attachment")
            .1
    }

    pub fn replace(&mut self, rhi: &mut Rhi, label: &str, extent: wgpu::Extent3d) {
        for (i, color) in self.colors[..self.color_count].iter().flatten().enumerate() {
            rhi.replace_texture(
                color.handle,
                &wgpu::TextureDescriptor {
                    label: Some(&format!("{label}_color_{i}")),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: SAMPLE_COUNT,
                    dimension: wgpu::TextureDimension::D2,
                    format: color.format,
                    usage: color.usage,
                    view_formats: &[],
                },
            );
        }

        if let Some(depth) = self.depth {
            rhi.replace_texture(
                depth,
                &wgpu::TextureDescriptor {
                    label: Some(&format!("{label}_depth")),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: SAMPLE_COUNT,
                    dimension: wgpu::TextureDimension::D2,
                    format: DEPTH_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
            );
        }

        self.extent = extent;
    }
}

fn bytes_per_sample(formats: &[wgpu::TextureFormat]) -> u32 {
    let mut total: u32 = 0;
    for format in formats {
        let alignment = format
            .target_component_alignment()
            .expect("color-renderable format has a component alignment");
        let cost = format
            .target_pixel_byte_cost()
            .expect("color-renderable format has a pixel byte cost");
        total = total.next_multiple_of(alignment) + cost;
    }
    total
}

pub struct RenderTargetRing {
    slots: Vec<RenderTarget>,
    size: (u32, u32),
    desired: (u32, u32),
}

impl RenderTargetRing {
    pub fn new(
        rhi: &mut Rhi,
        label: &str,
        color_format: wgpu::TextureFormat,
        size: (u32, u32),
        slot_count: usize,
    ) -> Self {
        let extent = extent_of(size);
        let slots = (0..slot_count)
            .map(|i| RenderTarget::color_depth(rhi, &format!("{label}[{i}]"), extent, color_format))
            .collect();

        Self {
            slots,
            size,
            desired: size,
        }
    }

    pub fn slot(&self, i: usize) -> &RenderTarget {
        &self.slots[i]
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn presented_handle(&self, i: usize) -> TextureHandle {
        self.slot(i).presented().handle
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn set_desired(&mut self, size: (u32, u32)) {
        self.desired = size;
    }

    pub fn apply_resize(&mut self, rhi: &mut Rhi, label: &str) -> bool {
        if self.desired == self.size || self.desired.0 == 0 || self.desired.1 == 0 {
            return false;
        }

        let extent = extent_of(self.desired);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.replace(rhi, &format!("{label}[{i}]"), extent);
        }

        self.size = self.desired;

        true
    }
}

pub struct ResolvedTarget<'a> {
    pub(crate) colors: [Option<&'a wgpu::TextureView>; MAX_COLOR_ATTACHMENTS],
    pub(crate) color_count: usize,
    pub depth: Option<&'a wgpu::TextureView>,
    pub extent: wgpu::Extent3d,
    pub sample_count: u32,
}

impl<'a> ResolvedTarget<'a> {
    pub fn color_count(&self) -> usize {
        self.color_count
    }

    pub fn color(&self, i: usize) -> Option<&'a wgpu::TextureView> {
        self.colors[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(format: wgpu::TextureFormat, role: AttachmentRole) -> ColorAttachment {
        ColorAttachment {
            handle: TextureHandle::default(),
            format,
            role,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        }
    }

    fn render_target(attachments: &[ColorAttachment]) -> RenderTarget {
        let mut colors = [None; MAX_COLOR_ATTACHMENTS];
        for (i, attachment) in attachments.iter().enumerate() {
            colors[i] = Some(*attachment);
        }
        RenderTarget {
            colors,
            color_count: attachments.len(),
            depth: None,
            extent: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            sample_count: 1,
        }
    }

    #[test]
    fn tracks_attachment_order_and_roles() {
        let target = render_target(&[
            attachment(wgpu::TextureFormat::Rgba8Unorm, AttachmentRole::Color),
            attachment(wgpu::TextureFormat::R32Uint, AttachmentRole::ObjectId),
        ]);

        assert_eq!(target.color_count(), 2);

        let roles: Vec<_> = target.colors().map(|attachment| attachment.role).collect();
        assert_eq!(roles, [AttachmentRole::Color, AttachmentRole::ObjectId]);

        let (index, found) = target
            .find(AttachmentRole::ObjectId)
            .expect("object-id attachment present");
        assert_eq!(index, 1);
        assert_eq!(found.format, wgpu::TextureFormat::R32Uint);

        assert_eq!(
            target.find(AttachmentRole::Color).expect("color attachment present").0,
            0
        );
        assert_eq!(target.presented().role, AttachmentRole::Color);
    }

    #[test]
    fn bytes_per_sample_mirrors_webgpu_rule() {
        use wgpu::TextureFormat::{R8Unorm, R32Uint, Rg16Float, Rgba8Unorm, Rgba16Float, Rgba32Float};

        assert_eq!(bytes_per_sample(&[Rgba8Unorm, Rgba16Float, Rg16Float, R32Uint]), 24);
        assert_eq!(bytes_per_sample(&[Rgba8Unorm; 4]), 32);
        assert_eq!(bytes_per_sample(&[Rgba16Float; 5]), 40);
        assert_eq!(bytes_per_sample(&[R8Unorm, Rgba32Float]), 20);
        assert_eq!(bytes_per_sample(&[Rgba32Float, R8Unorm]), 17);
    }
}
