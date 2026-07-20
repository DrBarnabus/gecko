use std::cell::Cell;
use std::num::NonZeroUsize;

use encase::ShaderType;

use crate::Rhi;

/// Group-0 / per-frame data
#[derive(Copy, Clone, Debug, ShaderType)]
pub struct FrameUniform {
    pub frame_index: u32,
    pub time: f32,
    pub delta_time: f32,
}

pub(crate) fn frame_uniform_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("frame_uniform_layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(FrameUniform::min_size()),
            },
            count: None,
        }],
    })
}

pub struct FrameSlot {
    pub frame_uniform: wgpu::Buffer,
    pub frame_uniform_bind_group: wgpu::BindGroup,
}

pub struct FramesInFlight {
    slots: Vec<FrameSlot>,
    frame_index: Cell<u64>,
}

impl FramesInFlight {
    pub(crate) fn new(
        device: &wgpu::Device,
        depth: NonZeroUsize,
        frame_uniform_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let slots = (0..depth.get())
            .map(|i| {
                let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("frame-globals[{i}]")),
                    size: FrameUniform::min_size().get(),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                let frame_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("frame-globals-bg[{i}]")),
                    layout: frame_uniform_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame_uniform.as_entire_binding(),
                    }],
                });

                FrameSlot {
                    frame_uniform,
                    frame_uniform_bind_group,
                }
            })
            .collect();

        Self {
            slots,
            frame_index: Cell::new(0),
        }
    }

    #[inline]
    pub fn frame_index(&self) -> u64 {
        self.frame_index.get()
    }

    #[inline]
    pub fn slot_index(&self) -> usize {
        (self.frame_index.get() % self.slots.len() as u64) as usize
    }

    #[inline]
    pub(crate) fn current(&self) -> &FrameSlot {
        &self.slots[self.slot_index()]
    }

    #[inline]
    pub(crate) fn advance(&self) {
        self.frame_index.set(self.frame_index.get() + 1);
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct FrameTiming {
    pub time: f32,
    pub delta_time: f32,
}

pub struct FrameContext<'a> {
    pub frame_index: u64,
    pub slot_index: usize,
    pub timing: FrameTiming,

    rhi: &'a Rhi,
    command_buffers: Vec<wgpu::CommandBuffer>,
}

impl<'a> FrameContext<'a> {
    pub(crate) fn new(rhi: &'a Rhi, frame_index: u64, slot_index: usize, timing: FrameTiming) -> Self {
        Self {
            frame_index,
            slot_index,
            timing,
            rhi,
            command_buffers: Vec::new(),
        }
    }

    pub fn create_encoder(&self, label: &str) -> wgpu::CommandEncoder {
        self.rhi
            .context
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    }

    pub fn submit(&mut self, command_buffer: wgpu::CommandBuffer) {
        self.command_buffers.push(command_buffer);
    }

    pub fn end(self) {}

    #[tracing::instrument(skip_all)]
    fn end_frame(&mut self) {
        let command_buffers = std::mem::take(&mut self.command_buffers);
        self.rhi.context.queue().submit(command_buffers);

        self.rhi.frames.advance();
    }

    #[inline]
    pub fn queue(&self) -> &wgpu::Queue {
        self.rhi.context.queue()
    }

    #[inline]
    pub fn frame_uniform_bind_group(&self) -> &wgpu::BindGroup {
        &self.rhi.frames.slots[self.slot_index].frame_uniform_bind_group
    }
}

impl Drop for FrameContext<'_> {
    fn drop(&mut self) {
        self.end_frame();
    }
}
