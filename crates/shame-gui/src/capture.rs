//! Frame capture and test runner infrastructure.
//!
//! `AppRunner` is the extension point for tests — implement it to inject
//! events, inspect state, capture frames, or control the event loop.
//! `read_frame_rgba` is a pure GPU readback utility (no image dep).

use shame_wgpu as sm;

use crate::graph::{DagStruct, Graph};
use crate::gui::Gui;
use crate::gui::event::InputEvent;

/// Hooks called after each DAG tick (both `step()` and `run()`) and after
/// each rendered frame (`run()` only). Tests implement this to simulate
/// interaction sequences, inspect state, capture frames, or control exit.
pub trait AppRunner<S: DagStruct> {
    /// Called after the DAG tick completes. The runner inspects state and
    /// returns events to inject. In `step()` these events are processed at
    /// the start of the next `step()` call.
    fn after_tick(&mut self, _ctx: &AppContext<'_, S>) -> Vec<InputEvent> {
        vec![]
    }

    /// Called after rendering a frame (`run()` only — not `step()`).
    /// Return `true` to stop the event loop.
    fn after_render(&mut self, _frame: &FrameOutput<'_>) -> bool {
        false
    }
}

/// Read-only snapshot of app state available to runners in `after_tick`.
pub struct AppContext<'a, S: DagStruct> {
    /// The app state (read port values, inspect widget data).
    pub state: &'a S,
    /// The graph (dirty/fired state, condition map).
    pub graph: &'a Graph<S>,
    /// The attached GUI, if any.
    pub gui: Option<&'a Gui<S>>,
}

/// Per-frame render output available to runners in `after_render`.
pub struct FrameOutput<'a> {
    /// The surface texture containing the rendered frame.
    pub surface_texture: &'a wgpu::Texture,
    /// The surface texture format.
    pub surface_format: wgpu::TextureFormat,
    /// The surface width in physical pixels.
    pub surface_width: u32,
    /// The surface height in physical pixels.
    pub surface_height: u32,
    /// The shame GPU handle (for readbacks, extra encoders, ...).
    pub gpu: &'a sm::Gpu,
}

/// Reads back the surface texture into a contiguous RGBA buffer (no row
/// padding). Handles BGRA→RGBA swizzle automatically.
pub fn read_frame_rgba(
    gpu: &sm::Gpu,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Vec<u8> {
    assert!(
        matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ),
        "capture only supports 8-bit RGBA surface formats, got {format:?}"
    );
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let padded_size = bytes_per_row as u64 * height as u64;

    let buffer = gpu.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture staging"),
        size: padded_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("capture"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue().submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    let _ = gpu.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    if !matches!(rx.recv(), Ok(Ok(()))) {
        return Vec::new();
    }

    let data = buffer.slice(..).get_mapped_range();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * bytes_per_row) as usize;
        let end = start + (width * 4) as usize;
        rgba.extend_from_slice(&data[start..end]);
    }
    drop(data);

    if matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for chunk in rgba.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }
    }

    rgba
}
