//! [`Color`]: an sRGB authoring color with a one-way conversion to the
//! GPU's linear working space.

use crate::math::Vec4;

/// An sRGB color for authoring. It is a **construction-time helper**: call
/// `to_linear()` in place when building instance data — instances
/// hold linear `Vec4` colors directly, so `Color` never survives
/// into them.
///
/// CPU-only: `Color` deliberately does NOT implement `GpuStruct` — it never
/// crosses into instance buffers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    /// Red, 0.0–1.0 in sRGB.
    pub r: f32,
    /// Green, 0.0–1.0 in sRGB.
    pub g: f32,
    /// Blue, 0.0–1.0 in sRGB.
    pub b: f32,
    /// Alpha, 0.0–1.0 (linear; passes through [`Color::to_linear`] unchanged).
    pub a: f32,
}

impl Color {
    /// A color from its sRGB components and alpha.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque color from its sRGB components.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::new(r, g, b, 1.0)
    }

    /// Fully opaque white.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// Fully opaque black.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// sRGB → linear (the GPU's working space; the surface converts back).
    /// Alpha is linear already and passes through unchanged.
    pub fn to_linear(self) -> Vec4 {
        Vec4::new(
            srgb_to_linear(self.r),
            srgb_to_linear(self.g),
            srgb_to_linear(self.b),
            self.a,
        )
    }
}

/// Standard sRGB transfer function (IEC 61966-2-1).
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_and_black_are_identity() {
        assert_eq!(Color::WHITE.to_linear(), Vec4::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(Color::BLACK.to_linear(), Vec4::new(0.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn srgb_mid_gray_encodes_to_linear() {
        let linear = Color::rgb(0.5, 0.5, 0.5).to_linear();
        // 0.5 sRGB → 0.214 linear
        assert!((linear.x - 0.2140).abs() < 0.001);
    }

    #[test]
    fn alpha_passes_through_unchanged() {
        let color = Color::new(0.2, 0.4, 0.6, 0.35);
        let linear = color.to_linear();
        assert_eq!(linear.w, 0.35);
    }

    #[test]
    fn low_values_use_linear_segment() {
        // 0.01 sRGB → 0.01 / 12.92 linear (below the 0.04045 knee)
        let linear = Color::rgb(0.01, 0.01, 0.01).to_linear();
        assert!((linear.x - 0.01 / 12.92).abs() < 1e-6);
    }
}
