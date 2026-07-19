use crate::RhiError;

const BINDLESS_FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING);

#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub power_preference: wgpu::PowerPreference,
    pub min_immediate_size: u32,
    pub min_bind_groups: u32,
    pub frames_in_flight: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            power_preference: wgpu::PowerPreference::HighPerformance,
            min_immediate_size: 128,
            min_bind_groups: 8,
            frames_in_flight: 2,
        }
    }
}

pub struct Capabilities {
    pub features: wgpu::Features,
    pub limits: wgpu::Limits,
    pub max_immediate_size: u32,
    pub max_bind_groups: u32,
    pub supports_bindless: bool,
}

pub struct Context {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    capabilities: Capabilities,
}

impl Context {
    pub fn new(
        config: &ContextConfig,
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>,
    ) -> Result<(Self, wgpu::Surface<'static>), RhiError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let raw_surface = instance.create_surface(surface_target)?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: config.power_preference,
            compatible_surface: Some(&raw_surface),
            apply_limit_buckets: false,
            force_fallback_adapter: false,
        }))?;

        tracing::info!(adapter = %adapter.get_info().name, backend = ?adapter.get_info().backend, "aquired adapter");

        let adapter_features = adapter.features();
        let adapter_supports_bindless = adapter_features.contains(BINDLESS_FEATURES);

        let adapter_limits = adapter.limits();

        if adapter_limits.max_immediate_size < config.min_immediate_size {
            return Err(RhiError::UnmetLimit {
                name: "min_immediate_size",
                required: config.min_immediate_size,
                available: adapter_limits.max_immediate_size,
            });
        }

        if adapter_limits.max_bind_groups < config.min_bind_groups {
            return Err(RhiError::UnmetLimit {
                name: "min_bind_groups",
                required: config.min_bind_groups,
                available: adapter_limits.max_bind_groups,
            });
        }

        let mut required_features = wgpu::Features::empty();

        if adapter_supports_bindless {
            required_features = required_features.union(BINDLESS_FEATURES);
        }

        let required_limits = adapter_limits.clone();

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gecko-device"),
            required_features,
            required_limits,
            memory_hints: wgpu::MemoryHints::Performance,
            ..Default::default()
        }))?;

        let capabilities = Capabilities {
            features: device.features(),
            limits: device.limits(),
            max_immediate_size: adapter_limits.max_immediate_size,
            max_bind_groups: adapter_limits.max_bind_groups,
            supports_bindless: adapter_supports_bindless,
        };

        tracing::info!(
            max_immediate_size = capabilities.max_immediate_size,
            max_bind_groups = capabilities.max_bind_groups,
            supports_bindless = capabilities.supports_bindless,
            "aquired device"
        );

        Ok((
            Self {
                instance,
                adapter,
                device,
                queue,
                capabilities,
            },
            raw_surface,
        ))
    }

    #[inline]
    pub(crate) fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    #[inline]
    pub(crate) fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    #[inline]
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    #[inline]
    pub(crate) fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    #[inline]
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}
