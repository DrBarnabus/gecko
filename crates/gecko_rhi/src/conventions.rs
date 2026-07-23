pub const MAX_COLOR_ATTACHMENTS: usize = 8;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const DEPTH_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::Greater;
pub const DEPTH_CLEAR: f32 = 0.0;
