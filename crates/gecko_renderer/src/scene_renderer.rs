use std::{borrow::Cow, num::NonZeroU64};

use gecko_core::math::Mat4;
use gecko_rhi::conventions::{DEPTH_CLEAR, DEPTH_COMPARE, DEPTH_FORMAT};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ObjectUniform {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    tint: [f32; 4],
}

const SHADER: &str = include_str!("../../../assets/shaders/shader.wgsl");

pub struct SceneRenderer {
    grid_pipeline: wgpu::RenderPipeline,
    cube_pipeline: wgpu::RenderPipeline,
    grid_vb: wgpu::Buffer,
    grid_vertex_count: u32,
    cube_vb: wgpu::Buffer,
    cube_ib: wgpu::Buffer,
    cube_index_count: u32,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    uniform_stride: u64,
    max_objects: usize,
}

impl SceneRenderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        frame_uniform_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene_shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });

        let uniform_size = std::mem::size_of::<ObjectUniform>() as u64;
        let alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let uniform_stride = uniform_size.div_ceil(alignment) * alignment;

        let max_objects = 256usize;

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene_uniform_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(uniform_size),
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene_pipeline_layout"),
            bind_group_layouts: &[Some(frame_uniform_layout), None, None, Some(&uniform_layout)],
            immediate_size: 0,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene_uniforms"),
            size: uniform_stride * max_objects as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_uniform_bg"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: NonZeroU64::new(uniform_size),
                }),
            }],
        });

        let make_pipeline =
            |label: &str, topology: wgpu::PrimitiveTopology, cull: Option<wgpu::Face>, depth_write: bool| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[Some(Vertex::layout())],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: cull,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(depth_write),
                        depth_compare: Some(DEPTH_COMPARE),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };

        let grid_pipeline = make_pipeline("grid_pipeline", wgpu::PrimitiveTopology::LineList, None, false);
        let cube_pipeline = make_pipeline(
            "cube_pipeline",
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back),
            true,
        );

        let mut grid_vertices: Vec<Vertex> = Vec::new();
        let half = 6i32;
        for i in -half..=half {
            let f = i as f32;
            let (h, z) = (half as f32, [0.0; 3]);

            grid_vertices.push(Vertex {
                position: [-h, 0.0, f],
                normal: z,
            });
            grid_vertices.push(Vertex {
                position: [h, 0.0, f],
                normal: z,
            });
            grid_vertices.push(Vertex {
                position: [f, 0.0, -h],
                normal: z,
            });
            grid_vertices.push(Vertex {
                position: [f, 0.0, h],
                normal: z,
            });
        }

        let grid_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid_vb"),
            contents: bytemuck::cast_slice(&grid_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let (cube_vertices, cube_indices) = cube_mesh();
        let cube_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube_vb"),
            contents: bytemuck::cast_slice(&cube_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let cube_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube_ib"),
            contents: bytemuck::cast_slice(&cube_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            grid_pipeline,
            cube_pipeline,
            grid_vb,
            grid_vertex_count: grid_vertices.len() as u32,
            cube_vb,
            cube_ib,
            cube_index_count: cube_indices.len() as u32,
            uniform_buffer,
            uniform_bind_group,
            uniform_stride,
            max_objects,
        }
    }

    #[tracing::instrument(skip_all)]
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        frame_uniform_bind_group: &wgpu::BindGroup,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        view_proj: Mat4,
        objects: &[(Mat4, [f32; 3])],
        show_grid: bool,
    ) {
        let object_count = objects.len() + usize::from(show_grid);
        assert!(object_count <= self.max_objects, "raise max_objects");

        let uniform_size = std::mem::size_of::<ObjectUniform>();
        let mut bytes = vec![0u8; self.uniform_stride as usize * object_count.max(1)];
        let mut write = |slot: usize, model: Mat4, tint: [f32; 3]| {
            let u = ObjectUniform {
                view_proj: view_proj.to_cols_array_2d(),
                model: model.to_cols_array_2d(),
                tint: [tint[0], tint[1], tint[2], 1.0],
            };

            let start = slot * self.uniform_stride as usize;
            bytes[start..start + uniform_size].copy_from_slice(bytemuck::bytes_of(&u));
        };

        let mut base = 0usize;

        if show_grid {
            write(0, Mat4::IDENTITY, [0.35, 0.37, 0.40]);
            base = 1;
        }

        for (i, (model, color)) in objects.iter().enumerate() {
            write(base + i, *model, *color);
        }

        queue.write_buffer(&self.uniform_buffer, 0, &bytes);

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.06,
                        b: 0.09,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(DEPTH_CLEAR),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_bind_group(0, frame_uniform_bind_group, &[]);

        if show_grid {
            render_pass.set_pipeline(&self.grid_pipeline);
            render_pass.set_bind_group(3, &self.uniform_bind_group, &[0]);
            render_pass.set_vertex_buffer(0, self.grid_vb.slice(..));
            render_pass.draw(0..self.grid_vertex_count, 0..1);
        }

        render_pass.set_pipeline(&self.cube_pipeline);
        render_pass.set_vertex_buffer(0, self.cube_vb.slice(..));
        render_pass.set_index_buffer(self.cube_ib.slice(..), wgpu::IndexFormat::Uint16);
        for i in 0..objects.len() {
            let offset = ((base + i) as u64 * self.uniform_stride) as u32;
            render_pass.set_bind_group(3, &self.uniform_bind_group, &[offset]);
            render_pass.draw_indexed(0..self.cube_index_count, 0, 0..1);
        }
    }
}

fn cube_mesh() -> (Vec<Vertex>, Vec<u16>) {
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [[0.5, -0.5, 0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [0.5, 0.5, 0.5]],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [[-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5]],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
    ];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corners) in faces {
        let b = vertices.len() as u16;
        for position in corners {
            vertices.push(Vertex { position, normal });
        }

        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }

    (vertices, indices)
}
