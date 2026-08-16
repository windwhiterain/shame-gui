//! One-time GPU initialization (instance, adapter, device, surface).

use std::sync::Arc;

use winit::window::Window;

use shame_wgpu as sm;

/// Maximum push-constant (immediate) size in bytes — wgpu's guaranteed
/// minimum for `max_immediate_size`, and the limit requested in
/// [`Setup::new`]. [`Canvas`](crate::canvas::Canvas) panics when a material's
/// push constant exceeds this.
pub(crate) const MAX_IMMEDIATE_BYTES: usize = 256;

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
        let surface = instance
            .create_surface(Arc::clone(window))
            .expect("failed to create surface for window");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no GPU adapter compatible with this window/surface");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            // IMMEDIATES: shame push constants; INDIRECT_FIRST_INSTANCE:
            // the batched multi_draw_indexed_indirect path. An adapter that
            // lacks either fails here.
            required_features: wgpu::Features::IMMEDIATES | wgpu::Features::INDIRECT_FIRST_INSTANCE,
            required_limits: wgpu::Limits {
                // Cap push constants at the wgpu minimum that all adapters
                // guarantee for maxImmediateSize (see MAX_IMMEDIATE_BYTES).
                max_immediate_size: MAX_IMMEDIATE_BYTES as u32,
                ..wgpu::Limits::default().using_resolution(adapter.limits())
            },
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features: Default::default(),
        }))
        .expect("request_device failed — does the adapter support IMMEDIATES and INDIRECT_FIRST_INSTANCE?");
        device.on_uncaptured_error(Arc::new(|error| {
            eprintln!("wgpu error: {error}");
        }));
        let mut config = surface
            .get_default_config(&adapter, 1, 1)
            .expect("surface format not supported by adapter");
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

    /// Acquires the next surface texture and a view over it, or `None` when
    /// the surface is temporarily unavailable (window occluded/minimized, a
    /// transient timeout, or a lost/outdated surface that was just
    /// reconfigured). Never blocks or retries on the caller's thread: the
    /// caller skips the frame and retries on the next redraw.
    fn try_acquire_surface_texture(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Some(texture),
            wgpu::CurrentSurfaceTexture::Timeout => None,
            wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Lost
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                self.surface.configure(&self.gpu, &self.surface_config);
                None
            }
        }
    }

    /// Acquires the next surface texture and a view over it, or `None` when
    /// the surface is temporarily unavailable (see
    /// [`Self::try_acquire_surface_texture`]).
    pub fn try_acquire_surface(&self) -> Option<(wgpu::SurfaceTexture, wgpu::TextureView)> {
        let surface_texture = self.try_acquire_surface_texture()?;
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.surface_config.format),
                ..Default::default()
            });
        Some((surface_texture, view))
    }
}
