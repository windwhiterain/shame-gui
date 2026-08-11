//! Traits for writing single-source functions that compile to both CPU
//! (native `f32` / `Vec2`) and GPU (shame tracing EDSL `f32x1` / `f32x2`).
//!
//! ## How it works
//!
//! [`ScalarLike<const GPU: bool>`] abstracts a scalar (`f32` on CPU,
//! `sm::vec<f32, x1>` on GPU). [`Vec2Like<const GPU: bool>`] abstracts a
//! 2D vector. Write a generic function `fn foo<V: Vec2Like<GPU>, const GPU:
//! bool>(...)` and it monomorphizes into both a CPU and a GPU version.
//!
//! ## Usage
//!
//! ```ignore
//! use shame_gui::dual::{ScalarLike, Vec2Like, pixel_to_ndc};
//!
//! // CPU side — concrete Vec2, uses native f32 arithmetic.
//! let (ndc_pos, ndc_size) = pixel_to_ndc::<Vec2, false>(pos, size, fb);
//!
//! // GPU side — inside a shame pipeline encoding, uses shame tracing types.
//! let (ndc_pos, ndc_size) = pixel_to_ndc::<sm::f32x2, true>(pos, size, fb);
//! ```
//!
//! ## `const GPU: bool` vs separate traits
//!
//! A `const` generic lets a single generic function signature serve both
//! paths. The bool itself is not branched on at runtime — it only selects
//! the associated types (`Scalar`, `Vec2`) at monomorphization time.
//!
//! ## Limitations
//!
//! - GPU impls require an **active shame encoding context** (inside
//!   `Material::build` or equivalent). Calling `ScalarLike::from_f32` on
//!   the GPU path outside an encoding produces invalid nodes.
//! - `Vec2Like` returns field accessors **by value** (Copy), which works
//!   because both `Vec2` and `sm::vec<_, _>` are `Copy`.

use crate::math::Vec2;
use crate::rect::RectLike;
use crate::sm;
use crate::sm::ToGpuType;

// ---------------------------------------------------------------------------
// ScalarLike
// ---------------------------------------------------------------------------

/// A numeric scalar that knows how to make a GPU twin from an `f32` literal.
///
/// `const GPU: bool` = `false` → CPU (`f32`); `true` → GPU (`sm::vec<f32, x1>`).
///
/// # Safety / context requirement
///
/// GPU-side `from_f32()` calls `.to_gpu()` internally, which **requires an
/// active shame encoding context**. Calling it outside pipeline construction
/// produces invalid `Any` nodes.
pub trait ScalarLike<const GPU: bool>: Copy {
    /// Build a scalar from an `f32` literal.
    fn from_f32(v: f32) -> Self;
}

// CPU
impl ScalarLike<false> for f32 {
    #[inline]
    fn from_f32(v: f32) -> Self {
        v
    }
}

// GPU
impl ScalarLike<true> for sm::f32x1 {
    #[inline]
    fn from_f32(v: f32) -> Self {
        v.to_gpu()
    }
}

// ---------------------------------------------------------------------------
// FloatScalar — bundles ScalarLike + arithmetic
// ---------------------------------------------------------------------------

/// A [`ScalarLike`] that also supports `+`, `-`, `*`, `/`.
///
/// Blanket-implemented for any `T: ScalarLike<GPU> + Add + Sub + Mul + Div`.
pub trait FloatScalar<const GPU: bool>:
    ScalarLike<GPU>
    + std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Mul<Output = Self>
    + std::ops::Div<Output = Self>
{
}

impl<const GPU: bool, T> FloatScalar<GPU> for T where
    T: ScalarLike<GPU>
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>
{
}

// ---------------------------------------------------------------------------
// Vec2Like
// ---------------------------------------------------------------------------

/// A 2D vector whose components are [`FloatScalar<GPU>`].
///
/// `const GPU: bool` = `false` → CPU (`crate::Vec2`); `true` → GPU (`sm::f32x2`).
pub trait Vec2Like<const GPU: bool>: Copy {
    /// The component scalar type (supports arithmetic + from_f32).
    type Scalar: FloatScalar<GPU>;

    /// Construct from two scalars.
    fn new(x: Self::Scalar, y: Self::Scalar) -> Self;

    /// The `x` component (by value, both CPU and GPU types are `Copy`).
    fn x(self) -> Self::Scalar;

    /// The `y` component (by value).
    fn y(self) -> Self::Scalar;
}

// CPU
impl Vec2Like<false> for Vec2 {
    type Scalar = f32;

    #[inline]
    fn new(x: f32, y: f32) -> Self {
        Vec2::new(x, y)
    }
    #[inline]
    fn x(self) -> f32 {
        self.x
    }
    #[inline]
    fn y(self) -> f32 {
        self.y
    }
}

// GPU
impl Vec2Like<true> for sm::f32x2 {
    type Scalar = sm::f32x1;

    #[inline]
    fn new(x: Self::Scalar, y: Self::Scalar) -> Self {
        sm::vec!(x, y)
    }
    #[inline]
    fn x(self) -> Self::Scalar {
        self.x
    }
    #[inline]
    fn y(self) -> Self::Scalar {
        self.y
    }
}

// ---------------------------------------------------------------------------
// reinterpret — raw pointer cast for same-type bridging (zero-copy)
// ---------------------------------------------------------------------------

/// `&Src` → `&Dst`. No copy — same memory, different type.
///
/// # Safety
/// `Src` and `Dst` must be the same concrete type (size, layout, validity).
#[inline]
pub unsafe fn reinterpret_ref<Dst, Src>(src: &Src) -> &Dst {
    unsafe { &*(src as *const Src as *const Dst) }
}

/// `&mut Src` → `&mut Dst`. No copy — same memory, different type.
///
/// # Safety
/// `Src` and `Dst` must be the same concrete type.
#[inline]
pub unsafe fn reinterpret_mut<Dst, Src>(src: &mut Src) -> &mut Dst {
    unsafe { &mut *(src as *mut Src as *mut Dst) }
}

/// `Src` → `Dst` (by value). Bitwise read, original is forgotten.
///
/// # Safety
/// `Src` and `Dst` must be the same concrete type.
#[inline]
pub unsafe fn reinterpret_val<Dst, Src>(src: Src) -> Dst {
    let val = unsafe { std::ptr::read(&src as *const Src as *const Dst) };
    std::mem::forget(src);
    val
}

// ---------------------------------------------------------------------------
// Shared functions
// ---------------------------------------------------------------------------

/// Pixel-to-NDC conversion: `(pos, size)` in physical-pixel space (y-down)
/// → NDC space (y-up). Works for both CPU and GPU.
///
/// Three independent type parameters so callers don't need to prove
/// `Pos == Size == Fb`. Internally bridges via [`reinterpret_val`].
///
/// ```text
/// ndc_pos.x = pos.x / fb.x * 2 - 1
/// ndc_pos.y = 1 - (pos.y + size.y) / fb.y * 2
/// ndc_size  = size / fb * 2
/// ```
pub fn pixel_to_ndc<VPos, VSize, VFb, const GPU: bool>(
    pos: VPos,
    size: VSize,
    fb: VFb,
) -> (VPos, VSize)
where
    VPos: Vec2Like<GPU>,
    VSize: Vec2Like<GPU>,
    VFb: Vec2Like<GPU>,
{
    // Bridge size and fb into VPos world — same concrete type per derive impl.
    let size_p: VPos = unsafe { reinterpret_val(size) };
    let fb_p: VPos = unsafe { reinterpret_val(fb) };

    let two = VPos::Scalar::from_f32(2.0);
    let one = VPos::Scalar::from_f32(1.0);

    let ndc_pos_x = pos.x() / fb_p.x() * two - one;
    let ndc_pos_y = one - (pos.y() + size_p.y()) / fb_p.y() * two;
    let ndc_size_x = size_p.x() / fb_p.x() * two;
    let ndc_size_y = size_p.y() / fb_p.y() * two;

    let ndc_pos = VPos::new(ndc_pos_x, ndc_pos_y);
    let ndc_size_p = VPos::new(ndc_size_x, ndc_size_y);
    let ndc_size: VSize = unsafe { reinterpret_val(ndc_size_p) };

    (ndc_pos, ndc_size)
}

/// Pixel-to-NDC conversion at the [`RectLike`] level. Calls
/// [`pixel_to_ndc`] with the three independent field types, then wraps
/// back into a rect.
pub fn rect_to_ndc<R: RectLike<GPU>, const GPU: bool>(rect: R, fb: R::Pos) -> R
where
    R::Pos: Vec2Like<GPU>,
    R::Size: Vec2Like<GPU>,
{
    let pos = rect.pos();
    let size = rect.size();
    let (ndc_pos, ndc_size) = pixel_to_ndc::<R::Pos, R::Size, R::Pos, GPU>(pos, size, fb);
    R::new(ndc_pos, ndc_size)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Vec2, Vec2u};
    use crate::rect::Rect;

    #[test]
    fn cpu_matches_rect_to_ndc() {
        // Compare pixel_to_ndc::<Vec2, false> against the existing
        // Rect::to_ndc for a few representative inputs.
        let cases: &[(Vec2u, Rect)] = &[
            (
                Vec2u::new(800, 600),
                Rect::new(Vec2::new(0.0, 0.0), Vec2::new(800.0, 600.0)),
            ),
            (
                Vec2u::new(1920, 1080),
                Rect::new(Vec2::new(100.0, 200.0), Vec2::new(300.0, 400.0)),
            ),
            (
                Vec2u::new(256, 256),
                Rect::new(Vec2::new(32.0, 64.0), Vec2::new(128.0, 96.0)),
            ),
        ];

        for (fb, rect) in cases {
            let expected = rect.to_ndc(*fb);
            let fb_f = Vec2::new(fb.x as f32, fb.y as f32);
            let (pos, size) = pixel_to_ndc::<Vec2, Vec2, Vec2, false>(rect.pos, rect.size, fb_f);
            let result = Rect::new(pos, size);
            // float comparison with small epsilon
            let eps = 1e-5;
            assert!(
                (result.pos.x - expected.pos.x).abs() < eps,
                "pos.x mismatch: {result:?} vs {expected:?}"
            );
            assert!(
                (result.pos.y - expected.pos.y).abs() < eps,
                "pos.y mismatch: {result:?} vs {expected:?}"
            );
            assert!(
                (result.size.x - expected.size.x).abs() < eps,
                "size.x mismatch: {result:?} vs {expected:?}"
            );
            assert!(
                (result.size.y - expected.size.y).abs() < eps,
                "size.y mismatch: {result:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn cpu_rect_to_ndc_matches() {
        // Verify rect_to_ndc matches Rect::to_ndc on CPU.
        let cases: &[(Vec2u, Rect)] = &[
            (
                Vec2u::new(800, 600),
                Rect::new(Vec2::new(0.0, 0.0), Vec2::new(800.0, 600.0)),
            ),
            (
                Vec2u::new(1920, 1080),
                Rect::new(Vec2::new(100.0, 200.0), Vec2::new(300.0, 400.0)),
            ),
            (
                Vec2u::new(256, 256),
                Rect::new(Vec2::new(32.0, 64.0), Vec2::new(128.0, 96.0)),
            ),
        ];

        for (fb, rect) in cases {
            let expected = rect.to_ndc(*fb);
            let fb_f = Vec2::new(fb.x as f32, fb.y as f32);
            let result = crate::dual::rect_to_ndc::<Rect, false>(*rect, fb_f);
            let eps = 1e-5;
            assert!(
                (result.pos.x - expected.pos.x).abs() < eps,
                "pos.x: {result:?} vs {expected:?}"
            );
            assert!(
                (result.pos.y - expected.pos.y).abs() < eps,
                "pos.y: {result:?} vs {expected:?}"
            );
            assert!(
                (result.size.x - expected.size.x).abs() < eps,
                "size.x: {result:?} vs {expected:?}"
            );
            assert!(
                (result.size.y - expected.size.y).abs() < eps,
                "size.y: {result:?} vs {expected:?}"
            );
        }
    }
}
