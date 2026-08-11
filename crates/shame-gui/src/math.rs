//! GPU-compatible vector types.
//!
//! All four types are `#[repr(C)]` with alignment matching their WGSL
//! counterparts (`vec2<f32>` is align 8, `vec4<f32>` is align 16), derive
//! [`DagStruct`](crate::DagStruct) so they can flow through ports, and
//! implement `bytemuck::Pod`/`Zeroable` so they can be copied into
//! instance buffers. They also implement basic arithmetic operators and
//! `From` conversions to/from plain arrays.

use std::ops::{Add, Div, Mul, Sub};

/// 2D vector, GPU-compatible (`#[repr(C, align(8))]` — matches WGSL
/// `vec2<f32>` alignment, so `offset_of!` agrees with the derive's WGSL
/// offset math; `bytemuck::Pod` + `Zeroable`).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, shame_gui_derive::DagStruct)]
pub struct Vec2 {
    /// The x component.
    pub x: f32,
    /// The y component.
    pub y: f32,
}

/// 4D vector (positions, colors, rects, transforms), GPU-compatible.
/// `align(16)` matches WGSL `vec4<f32>` alignment.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, PartialEq, shame_gui_derive::DagStruct)]
pub struct Vec4 {
    /// The x component.
    pub x: f32,
    /// The y component.
    pub y: f32,
    /// The z component.
    pub z: f32,
    /// The w component.
    pub w: f32,
}

/// 2D unsigned integer vector (pixel sizes, counts), GPU-compatible.
/// `align(8)` matches WGSL `vec2<u32>` alignment.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, shame_gui_derive::DagStruct)]
pub struct Vec2u {
    /// The x component.
    pub x: u32,
    /// The y component.
    pub y: u32,
}

/// 2D signed integer vector (pixel offsets), GPU-compatible.
/// `align(8)` matches WGSL `vec2<i32>` alignment.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, shame_gui_derive::DagStruct)]
pub struct Vec2i {
    /// The x component.
    pub x: i32,
    /// The y component.
    pub y: i32,
}

// ── Vec2 impls ──────────────────────────────────────────────────────────────

impl Vec2 {
    /// Creates a vector from its components.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// A vector with both components set to `v`.
    pub const fn splat(v: f32) -> Self {
        Self { x: v, y: v }
    }

    /// Creates a vector from a `[x, y]` array.
    pub const fn from_array(a: [f32; 2]) -> Self {
        Self { x: a[0], y: a[1] }
    }

    /// Converts into a `[x, y]` array.
    pub const fn to_array(self) -> [f32; 2] {
        [self.x, self.y]
    }
}

impl From<[f32; 2]> for Vec2 {
    fn from(a: [f32; 2]) -> Self {
        Self::from_array(a)
    }
}

impl From<Vec2> for [f32; 2] {
    fn from(v: Vec2) -> Self {
        v.to_array()
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

unsafe impl bytemuck::Pod for Vec2 {}
unsafe impl bytemuck::Zeroable for Vec2 {}

// ── Vec4 impls ──────────────────────────────────────────────────────────────

impl Vec4 {
    /// Creates a vector from its components.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// A vector with all four components set to `v`.
    pub const fn splat(v: f32) -> Self {
        Self {
            x: v,
            y: v,
            z: v,
            w: v,
        }
    }

    /// Creates a vector from a `[x, y, z, w]` array.
    pub const fn from_array(a: [f32; 4]) -> Self {
        Self {
            x: a[0],
            y: a[1],
            z: a[2],
            w: a[3],
        }
    }

    /// Converts into a `[x, y, z, w]` array.
    pub const fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

impl From<[f32; 4]> for Vec4 {
    fn from(a: [f32; 4]) -> Self {
        Self::from_array(a)
    }
}

impl From<Vec4> for [f32; 4] {
    fn from(v: Vec4) -> Self {
        v.to_array()
    }
}

impl Add for Vec4 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(
            self.x + rhs.x,
            self.y + rhs.y,
            self.z + rhs.z,
            self.w + rhs.w,
        )
    }
}

impl Sub for Vec4 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(
            self.x - rhs.x,
            self.y - rhs.y,
            self.z - rhs.z,
            self.w - rhs.w,
        )
    }
}

impl Mul<f32> for Vec4 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs, self.w * rhs)
    }
}

impl Div<f32> for Vec4 {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs, self.w / rhs)
    }
}

unsafe impl bytemuck::Pod for Vec4 {}
unsafe impl bytemuck::Zeroable for Vec4 {}

// ── Vec2u impls ─────────────────────────────────────────────────────────────

impl Vec2u {
    /// Creates a vector from its components.
    pub const fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }

    /// A vector with both components set to `v`.
    pub const fn splat(v: u32) -> Self {
        Self { x: v, y: v }
    }

    /// Creates a vector from a `[x, y]` array.
    pub const fn from_array(a: [u32; 2]) -> Self {
        Self { x: a[0], y: a[1] }
    }

    /// Converts into a `[x, y]` array.
    pub const fn to_array(self) -> [u32; 2] {
        [self.x, self.y]
    }
}

impl From<[u32; 2]> for Vec2u {
    fn from(a: [u32; 2]) -> Self {
        Self::from_array(a)
    }
}

impl From<Vec2u> for [u32; 2] {
    fn from(v: Vec2u) -> Self {
        v.to_array()
    }
}

impl From<Vec2u> for Vec2 {
    fn from(v: Vec2u) -> Self {
        Self::new(v.x as f32, v.y as f32)
    }
}

impl Add for Vec2u {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2u {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<u32> for Vec2u {
    type Output = Self;
    fn mul(self, rhs: u32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<u32> for Vec2u {
    type Output = Self;
    fn div(self, rhs: u32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

unsafe impl bytemuck::Pod for Vec2u {}
unsafe impl bytemuck::Zeroable for Vec2u {}

// ── Vec2i impls ─────────────────────────────────────────────────────────────

impl Vec2i {
    /// Creates a vector from its components.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// A vector with both components set to `v`.
    pub const fn splat(v: i32) -> Self {
        Self { x: v, y: v }
    }

    /// Creates a vector from a `[x, y]` array.
    pub const fn from_array(a: [i32; 2]) -> Self {
        Self { x: a[0], y: a[1] }
    }

    /// Converts into a `[x, y]` array.
    pub const fn to_array(self) -> [i32; 2] {
        [self.x, self.y]
    }
}

impl From<[i32; 2]> for Vec2i {
    fn from(a: [i32; 2]) -> Self {
        Self::from_array(a)
    }
}

impl From<Vec2i> for [i32; 2] {
    fn from(v: Vec2i) -> Self {
        v.to_array()
    }
}

impl From<Vec2i> for Vec2 {
    fn from(v: Vec2i) -> Self {
        Self::new(v.x as f32, v.y as f32)
    }
}

impl Add for Vec2i {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2i {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<i32> for Vec2i {
    type Output = Self;
    fn mul(self, rhs: i32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<i32> for Vec2i {
    type Output = Self;
    fn div(self, rhs: i32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

unsafe impl bytemuck::Pod for Vec2i {}
unsafe impl bytemuck::Zeroable for Vec2i {}
