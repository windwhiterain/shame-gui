//! Text rendering via glyphon: [`TextObject`] is the data holder,
//! [`TextSystem`] owns the fonts, glyph cache, and GPU atlas.

use crate::color::Color;
use crate::math::Vec2;

/// A single text element — the user-facing data holder, analogous to
/// [`RectEntry`]. Position is in physical pixels, y-down, measured from
/// the top-left corner of the text. `color` is sRGB (glyphon writes it
/// directly into the atlas).
///
/// Implements [`DagStruct`](crate::DagStruct), so `Vec<TextObject>` can
/// flow through the DAG's `texts` render port.
#[derive(Clone)]
pub struct TextObject {
    /// The text content.
    pub text: String,
    /// Top-left corner in physical pixels, y-down.
    pub position: Vec2,
    /// Font size in pixels.
    pub font_size: f32,
    /// Text color in sRGB.
    pub color: Color,
    /// Depth layer; smaller z is closer.
    pub z: f32,
    /// Optional wrap width in pixels.
    pub wrap_width: Option<f32>,
}

impl TextObject {
    /// Creates a text object; all style fields are builder-configured.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            position: Vec2::default(),
            font_size: 16.0,
            color: Color::WHITE,
            // On top of typical user objects (z < 0.5) and above the GUI
            // panel layer (Z_PANEL = 0.5) — matches the GUI's Z_TEXT.
            z: 0.3,
            wrap_width: None,
        }
    }

    /// Sets the top-left position in physical pixels, y-down.
    pub fn at_position(mut self, pos: Vec2) -> Self {
        self.position = pos;
        self
    }

    /// Sets the font size in pixels.
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Sets the text color (sRGB — no conversion needed here).
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the depth; smaller z is closer (shared depth with rects).
    pub fn with_z(mut self, z: f32) -> Self {
        self.z = z;
        self
    }

    /// Wraps the text at the given pixel width.
    pub fn with_wrap_width(mut self, width: f32) -> Self {
        self.wrap_width = Some(width);
        self
    }
}

/// Line height for a font size. cosmic-text `Metrics` needs it up front,
/// so it must be computable before shaping (here: font_size * 1.2, ceiled).
fn line_height(font_size: f32) -> f32 {
    (font_size * 1.2).ceil()
}

/// Owns the font system, glyph cache, GPU atlas, and text renderer.
/// Created outside `App::run()` — `measure()` is available immediately.
/// GPU resources are lazily initialized on the first frame when the
/// wgpu device becomes available.
pub struct TextSystem {
    font_system: cosmic_text::FontSystem,
    swash_cache: cosmic_text::SwashCache,
    /// Lazily initialized GPU resources.
    gpu: Option<TextGpu>,
    /// Queued text objects for the current frame.
    objects: Vec<TextObject>,
}

struct TextGpu {
    /// Kept alive for the atlas/renderer (which borrow it at construction).
    #[allow(dead_code)]
    cache: glyphon::Cache,
    atlas: glyphon::TextAtlas,
    renderer: glyphon::TextRenderer,
    viewport: glyphon::Viewport,
}

impl TextSystem {
    /// Creates a text system. `measure()` is available immediately;
    /// GPU resources are lazily initialized on the first render frame.
    pub fn new() -> Self {
        Self {
            font_system: cosmic_text::FontSystem::new(),
            swash_cache: cosmic_text::SwashCache::new(),
            gpu: None,
            objects: Vec::new(),
        }
    }

    /// Creates a text system from embedded font data (no system font
    /// scanning). Deterministic across machines — intended for tests.
    pub fn with_font_data(font_data: &[u8]) -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(font_data.to_vec());
        Self {
            font_system: cosmic_text::FontSystem::new_with_locale_and_db("en-US".into(), db),
            swash_cache: cosmic_text::SwashCache::new(),
            gpu: None,
            objects: Vec::new(),
        }
    }

    /// Measure the pixel dimensions of text with the given font size.
    /// `wrap_width` of `None` means single-line (width is the natural width);
    /// `Some(w)` wraps at width `w` and computes the resulting height.
    ///
    /// Callable before, during, or after `App::run()` — it only uses the
    /// `FontSystem`, no GPU resources.
    pub fn measure(&mut self, text: &str, font_size: f32, wrap_width: Option<f32>) -> Vec2 {
        let mut buffer = cosmic_text::Buffer::new(
            &mut self.font_system,
            cosmic_text::Metrics::new(font_size, line_height(font_size)),
        );
        buffer.set_text(
            &mut self.font_system,
            text,
            &cosmic_text::Attrs::new(),
            cosmic_text::Shaping::Advanced,
            None,
        );
        if let Some(width) = wrap_width {
            buffer.set_size(&mut self.font_system, Some(width), None);
        }
        buffer.shape_until_scroll(&mut self.font_system, false);
        // `Buffer::size()` reports the *set* dimensions (the wrap constraint),
        // not the laid-out text extent; compute the extent from layout runs.
        let mut width = 0.0f32;
        let mut height = 0.0f32;
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            height = height.max(run.line_top + run.line_height);
        }
        Vec2::new(width.ceil(), height.ceil())
    }

    /// Queue a text object for rendering this frame.
    pub(crate) fn queue(&mut self, object: TextObject) {
        self.objects.push(object);
    }

    /// Clear queued text for a new frame. Capacity retained.
    pub(crate) fn clear(&mut self) {
        self.objects.clear();
    }

    /// Lazily initialize GPU resources. Called once when the device
    /// first becomes available.
    pub(crate) fn ensure_gpu(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) {
        if self.gpu.is_some() {
            return;
        }
        let cache = glyphon::Cache::new(device);
        let mut atlas = glyphon::TextAtlas::new(device, queue, &cache, format);
        let viewport = glyphon::Viewport::new(device, &cache);
        let renderer = glyphon::TextRenderer::new(
            &mut atlas,
            device,
            wgpu::MultisampleState::default(),
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
        );
        self.gpu = Some(TextGpu {
            cache,
            atlas,
            renderer,
            viewport,
        });
    }

    /// Update viewport resolution for the current framebuffer size.
    pub(crate) fn update_viewport(&mut self, queue: &wgpu::Queue, width: u32, height: u32) {
        if let Some(ref mut gpu) = self.gpu {
            gpu.viewport
                .update(queue, glyphon::Resolution { width, height });
        }
    }

    /// Prepare glyphs: build cosmic-text buffers from queued objects,
    /// rasterize to the atlas. Call once per frame after all text objects
    /// have been queued.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<(), glyphon::PrepareError> {
        let Some(ref mut gpu) = self.gpu else {
            return Ok(());
        };
        if self.objects.is_empty() {
            return Ok(());
        }

        // Build cosmic-text buffers and glyphon TextAreas from queued objects.
        let mut buffers: Vec<cosmic_text::Buffer> = Vec::with_capacity(self.objects.len());
        let mut areas: Vec<glyphon::TextArea> = Vec::with_capacity(self.objects.len());
        let mut depths: Vec<f32> = Vec::with_capacity(self.objects.len());

        for (area_index, obj) in self.objects.iter().enumerate() {
            let mut buffer = cosmic_text::Buffer::new(
                &mut self.font_system,
                cosmic_text::Metrics::new(obj.font_size, line_height(obj.font_size)),
            );
            // glyphon's `prepare_with_depth` maps each glyph to a depth via
            // its `LayoutGlyph.metadata` — which comes from the Attrs. Stamp
            // the text-area index so the depth callback can resolve z.
            let attrs = cosmic_text::Attrs::new().metadata(area_index);
            buffer.set_text(
                &mut self.font_system,
                &obj.text,
                &attrs,
                cosmic_text::Shaping::Advanced,
                None,
            );
            if let Some(wrap_width) = obj.wrap_width {
                buffer.set_size(&mut self.font_system, Some(wrap_width), None);
            }
            buffer.shape_until_scroll(&mut self.font_system, false);

            depths.push(obj.z);
            buffers.push(buffer);
        }

        // Build TextAreas referencing the buffers.
        for (i, buffer) in buffers.iter().enumerate() {
            let obj = &self.objects[i];
            areas.push(glyphon::TextArea {
                buffer,
                left: obj.position.x,
                top: obj.position.y,
                scale: 1.0,
                bounds: glyphon::TextBounds {
                    left: 0,
                    top: 0,
                    right: i32::MAX,
                    bottom: i32::MAX,
                },
                default_color: cosmic_text::Color::rgba(
                    // Round (not truncate) and clamp, so fractional sRGB
                    // components map to the nearest 8-bit value.
                    (obj.color.r * 255.0).round().clamp(0.0, 255.0) as u8,
                    (obj.color.g * 255.0).round().clamp(0.0, 255.0) as u8,
                    (obj.color.b * 255.0).round().clamp(0.0, 255.0) as u8,
                    (obj.color.a * 255.0).round().clamp(0.0, 255.0) as u8,
                ),
                custom_glyphs: &[],
            });
        }

        gpu.renderer.prepare_with_depth(
            device,
            queue,
            &mut self.font_system,
            &mut gpu.atlas,
            &gpu.viewport,
            areas,
            &mut self.swash_cache,
            |i| *depths.get(i).unwrap_or(&0.5),
        )
    }

    /// Render prepared text to the given render pass. Must be called
    /// after `prepare()`.
    pub(crate) fn render<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
    ) -> Result<(), glyphon::RenderError> {
        let Some(ref gpu) = self.gpu else {
            return Ok(());
        };
        gpu.renderer.render(&gpu.atlas, &gpu.viewport, pass)
    }
}

/// Creates a text system using system fonts. System font availability and
/// metrics vary across machines — use [`TextSystem::with_font_data`] for
/// deterministic output (tests, snapshots).
impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}
