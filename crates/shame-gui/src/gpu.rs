//! One-time GPU initialization (instance, adapter, device, surface).

use std::sync::Arc;

use winit::window::Window;

use shame_wgpu as sm;

/// One-time GPU initialization (instance, adapter, device, surface) following
/// the upstream shame `hello_triangles` pattern. Panics on failure.
pub struct Setup {
    // Held for their lifetimes (device/surface borrow from them).
    #[allow(dead_code)]
    instance: wgpu::Instance,
    #[allow(dead_code)]
    adapter: wgpu::Adapter,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    /// The shame GPU wrapper (device + queue).
    pub gpu: sm::Gpu,
}

impl Setup {
    /// Initializes the GPU stack for a window. Panics on failure.
    pub fn new(window: &Arc<Window>) -> Self {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(Arc::clone(window)).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::IMMEDIATES | wgpu::Features::INDIRECT_FIRST_INSTANCE,
            required_limits: wgpu::Limits {
                max_immediate_size: 256,
                ..wgpu::Limits::default().using_resolution(adapter.limits())
            },
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features: Default::default(),
        }))
        .unwrap();
        device.on_uncaptured_error(Arc::new(|error| {
            eprintln!("wgpu error: {error}");
        }));
        let mut config = surface.get_default_config(&adapter, 1, 1).unwrap();
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.usage |= wgpu::TextureUsages::COPY_SRC;
        let mut setup = Setup {
            instance,
            adapter,
            surface,
            gpu: sm::Gpu::new(device, queue, Some(config.format)),
            surface_config: config,
        };
        let size: winit::dpi::PhysicalSize<u32> = window.inner_size();
        setup.resize(size.width, size.height);
        setup
    }

    /// Reconfigures the surface for a new window size (physical pixels).
    pub fn resize(&mut self, width: u32, height: u32) {
        let config = &mut self.surface_config;
        config.width = width.max(1);
        config.height = height.max(1);
        self.surface.configure(&self.gpu, config);
    }

    /// The current surface configuration (format, size, present mode).
    pub fn surface_config(&self) -> &wgpu::SurfaceConfiguration {
        &self.surface_config
    }

    /// Acquires the next surface texture and a view over it, retrying on
    /// transient failures and reconfiguring on lost/outdated surfaces.
    fn try_acquire_surface_texture(&self) -> wgpu::SurfaceTexture {
        let mut attempts = 0;
        loop {
            match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture)
                | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => return texture,
                wgpu::CurrentSurfaceTexture::Timeout => {}
                wgpu::CurrentSurfaceTexture::Outdated
                | wgpu::CurrentSurfaceTexture::Lost
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Validation => {
                    self.surface.configure(&self.gpu, &self.surface_config);
                }
            }
            attempts += 1;
            assert!(
                attempts < 100,
                "surface acquire failed after {attempts} retries"
            );
        }
    }

    /// Acquires the next surface texture and a view over it.
    pub fn try_acquire_surface(&self) -> (wgpu::SurfaceTexture, wgpu::TextureView) {
        let surface_texture = self.try_acquire_surface_texture();
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.surface_config.format),
                ..Default::default()
            });
        (surface_texture, view)
    }
}
