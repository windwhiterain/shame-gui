//! Snapshot comparison for the visual tests.
//!
//! Each snapshot test binary lives in `tests/snapshot_*.rs` and uses
//! `#[path]` to import the scene it needs from `examples/`. The shared
//! comparison + runner helpers live here.
//!
//! `compare_snapshot(name, app, text_system)` runs the scene, captures the
//! first frame to `tests/snapshots/<name>/actual.png`, and compares it
//! pixel-exactly against `expected.png`. A missing golden is auto-accepted
//! (first run / new scene) — re-run to compare after verifying it visually.

use std::path::{Path, PathBuf};

use image::GenericImageView;
use shame_gui::app::App;
use shame_gui::capture::{AppRunner, FrameOutput};
use shame_gui::graph::{AppState, DagStruct};
use shame_gui::text::TextSystem;

/// Returns a `TextSystem` backed by the embedded Inter font — no system
/// fonts, so glyph metrics are deterministic across machines.
#[allow(dead_code)]
pub fn test_text_system() -> TextSystem {
    TextSystem::with_font_data(include_bytes!("fonts/Inter-Regular.ttf"))
}

fn snap_dir(name: &str) -> String {
    format!("tests/snapshots/{name}")
}

/// Saves the RGBA buffer as a PNG using the `image` crate (dev-dependency).
fn save_png(path: &Path, rgba: &[u8], width: u32, height: u32) {
    image::save_buffer(path, rgba, width, height, image::ColorType::Rgba8)
        .unwrap_or_else(|e| panic!("failed to save {}: {e}", path.display()));
}

/// Runner that captures the first rendered frame to a PNG and exits.
struct SnapshotRunner {
    path: PathBuf,
}

impl<S: AppState + 'static> AppRunner<S> for SnapshotRunner {
    fn after_render(&mut self, frame: &FrameOutput<'_>) -> bool {
        let rgba = shame_gui::capture::read_frame_rgba(
            frame.gpu,
            frame.surface_texture,
            frame.surface_width,
            frame.surface_height,
            frame.surface_format,
        );
        save_png(&self.path, &rgba, frame.surface_width, frame.surface_height);
        println!("captured first frame to {}", self.path.display());
        true
    }
}

/// Runner that captures the second rendered frame to a PNG and exits.
/// Useful when a DAG node lags one frame behind widget writes (e.g.
/// ViewportRect → DAG → push constant).
struct TwoFrameRunner {
    path: PathBuf,
    count: u32,
}

impl<S: AppState + 'static> AppRunner<S> for TwoFrameRunner {
    fn after_render(&mut self, frame: &FrameOutput<'_>) -> bool {
        self.count += 1;
        if self.count < 2 {
            return false; // keep running
        }
        let rgba = shame_gui::capture::read_frame_rgba(
            frame.gpu,
            frame.surface_texture,
            frame.surface_width,
            frame.surface_height,
            frame.surface_format,
        );
        save_png(&self.path, &rgba, frame.surface_width, frame.surface_height);
        println!("captured second frame to {}", self.path.display());
        true
    }
}

/// Like [`compare_snapshot`] but captures the second frame instead of the first.
pub fn compare_snapshot_frame2<S: AppState + DagStruct + 'static>(
    name: &str,
    app: App<S>,
    text_system: TextSystem,
) {
    let path = format!("{}/actual.png", snap_dir(name));
    let runner = TwoFrameRunner {
        path: PathBuf::from(&path),
        count: 0,
    };
    app.with_runner(runner).run(text_system);
    compare_images(
        Path::new(&path),
        Path::new(&format!("{}/expected.png", snap_dir(name))),
        Path::new(&format!("{}/diff.png", snap_dir(name))),
    );
}

/// Runs the scene, captures the first frame, and compares it against the
/// golden. A missing golden is auto-accepted from the actual frame.
pub fn compare_snapshot<S: AppState + DagStruct + 'static>(
    name: &str,
    app: App<S>,
    text_system: TextSystem,
) {
    let path = format!("{}/actual.png", snap_dir(name));
    let runner = SnapshotRunner {
        path: PathBuf::from(&path),
    };
    app.with_runner(runner).run(text_system);
    compare_images(
        Path::new(&path),
        Path::new(&format!("{}/expected.png", snap_dir(name))),
        Path::new(&format!("{}/diff.png", snap_dir(name))),
    );
}

fn compare_images(actual: &Path, expected: &Path, diff: &Path) {
    let a = image::open(actual).unwrap_or_else(|e| {
        panic!("failed to open {}: {e}", actual.display());
    });

    // No golden yet (first run or new scene): accept the actual frame.
    if !expected.exists() {
        let _ = std::fs::create_dir_all(expected.parent().unwrap());
        std::fs::copy(actual, expected).unwrap();
        println!(
            "created snapshot: {} (verify it visually, then re-run to compare)",
            expected.display()
        );
        return;
    }

    let e = image::open(expected).unwrap_or_else(|err| {
        panic!("failed to open {}: {err}", expected.display());
    });

    let (aw, ah) = a.dimensions();
    let (ew, eh) = e.dimensions();
    assert_eq!(
        (aw, ah),
        (ew, eh),
        "size mismatch: actual {}x{} vs expected {}x{}",
        aw,
        ah,
        ew,
        eh
    );

    let a_rgba = a.to_rgba8();
    let e_rgba = e.to_rgba8();

    let mut diff_count = 0usize;
    let mut diff_img = image::RgbaImage::new(aw, ah);

    for y in 0..ah {
        for x in 0..aw {
            let ap = a_rgba.get_pixel(x, y);
            let ep = e_rgba.get_pixel(x, y);
            if ap != ep {
                diff_count += 1;
                diff_img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            } else {
                diff_img.put_pixel(x, y, *ap);
            }
        }
    }

    if diff_count > 0 {
        let _ = std::fs::create_dir_all(diff.parent().unwrap());
        diff_img.save(diff).ok();
        panic!(
            "{} differs from {}: {diff_count} pixels differ (diff written to {})",
            actual.display(),
            expected.display(),
            diff.display()
        );
    }
}
