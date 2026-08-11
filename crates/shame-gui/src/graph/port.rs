//! The unified port abstraction: one [`Port`] type reads/writes a typed
//! value `D` from a *state* `S`. The global app state and every collection
//! element are the same kind of thing — a `#[derive(DagStruct)]` struct —
//! so the same `Port` works for both.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::graph::element::DagStructRef;
use crate::graph::state::BuiltinState;
use crate::material::GpuBufferSlot;

/// Types that can flow through DAG ports.
///
/// Implemented for primitives (`f32`, `u32`, `i32`, `bool`, `usize`,
/// `String`, `()`), for `Vec<RectEntry>` / `Vec<TextObject>` (the render
/// outputs), for `HashMap<K, V>`, and automatically for any struct deriving
/// [`DagStruct`](crate::DagStruct) (e.g. `Vec2`, `Rect`, or user state
/// structs).
pub trait PortValue: Clone + Default + 'static {}
impl PortValue for f32 {}
impl PortValue for u32 {}
impl PortValue for i32 {}
impl PortValue for bool {}
impl PortValue for usize {}
impl PortValue for String {}
impl PortValue for () {}
impl PortValue for Vec<crate::shader::RectEntry> {}
impl PortValue for Vec<crate::text::TextObject> {}
impl PortValue for Vec<u8> {}
impl PortValue for Option<Arc<GpuBufferSlot>> {}
impl PortValue for Option<Arc<wgpu::BindGroup>> {}

impl<K: Clone + Eq + std::hash::Hash + 'static, V: Clone + 'static> PortValue
    for std::collections::HashMap<K, V>
{
}

impl<I: crate::instance::GpuStruct + 'static> PortValue for crate::material::InstanceBuffer<I> {}

impl<M: crate::material::Material> PortValue for M {}

/// Identifies a field within a state `S`. Two states (e.g. the global
/// [`BuiltinState`] and a map `Element`) each number their own fields from zero, so
/// a `PortId` is only meaningful relative to its state type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId(u64);

impl PortId {
    /// Wraps a raw field index. Prefer obtaining ids from generated ports.
    pub fn new(index: u64) -> Self {
        Self(index)
    }

    /// The underlying field index.
    pub fn index(&self) -> u64 {
        self.0
    }
}

/// A typed handle that reads/writes a value `D` stored as a field of state
/// `S`. Multiple `Port` handles can reference the same field — "connection"
/// is just sharing the same accessor.
///
/// `S` defaults to the built-in [`BuiltinState`], so a bare `Port<f32>` means
/// `Port<f32, BuiltinState>`.
pub struct Port<D: PortValue, S = BuiltinState> {
    id: PortId,
    read: fn(&S) -> &D,
    write: fn(&mut S, D),
    read_mut: fn(&mut S) -> &mut D,
    _marker: PhantomData<fn() -> (S, D)>,
}

// Manual Clone + Copy — the accessors are fn pointers, always Copy.
impl<D: PortValue, S> Clone for Port<D, S> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<D: PortValue, S> Copy for Port<D, S> {}

impl<D: PortValue, S> Port<D, S> {
    /// Wraps the raw accessors for one field of `S`. Used by the
    /// `#[derive(DagStruct)]`-generated `{S}::ports()`.
    pub fn new(
        id: PortId,
        read: fn(&S) -> &D,
        write: fn(&mut S, D),
        read_mut: fn(&mut S) -> &mut D,
    ) -> Self {
        Self {
            id,
            read,
            write,
            read_mut,
            _marker: PhantomData,
        }
    }

    /// The field index this handle reads/writes.
    pub fn id(&self) -> PortId {
        self.id
    }

    /// Read the value through a guarded state reference (the node/eval API).
    pub fn read<'r>(&self, r: &'r DagStructRef<'_, S>) -> &'r D {
        (self.read)(r.inner())
    }

    /// Write a value through a guarded state reference. Marks the port dirty
    /// in the graph.
    pub fn write(&self, r: &mut DagStructRef<'_, S>, value: D) {
        (self.write)(r.inner_mut(), value);
        r.mark_dirty(self.id);
    }

    /// Borrow the value mutably through a guarded state reference. Marks the
    /// port dirty in the graph.
    pub fn read_mut<'r>(&self, r: &'r mut DagStructRef<'_, S>) -> &'r mut D {
        r.mark_dirty(self.id);
        (self.read_mut)(r.inner_mut())
    }

    /// Read the value from a bare state reference (framework/internal; does
    /// not touch dirty tracking).
    pub fn read_state<'s>(&self, s: &'s S) -> &'s D {
        (self.read)(s)
    }

    /// Write a value into a bare state reference (framework/internal; does
    /// not touch dirty tracking).
    pub fn write_state(&self, s: &mut S, value: D) {
        (self.write)(s, value);
    }

    /// Borrow mutably from a bare state reference (framework/internal; does
    /// not touch dirty tracking).
    pub fn read_mut_state<'s>(&self, s: &'s mut S) -> &'s mut D {
        (self.read_mut)(s)
    }
}

impl<S> Port<bool, S> {
    /// Fires this port as a one-shot condition: writes `true`. The graph
    /// resets all condition ports to `false` at the end of the tick, so a
    /// fired condition is `true` for the duration of one tick only.
    pub fn fire(&self, r: &mut DagStructRef<'_, S>) {
        (self.write)(r.inner_mut(), true);
        r.mark_fired(self.id);
    }
}

impl<D: PortValue, S> std::fmt::Debug for Port<D, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Port").field("id", &self.id).finish()
    }
}

/// Structural group of ports sharing one state `S`. Used by `add_node`
/// (inputs + outputs) and generated `{S}Ports` groups.
pub trait PortGroup<S> {
    /// Number of leaf ports in this group.
    fn leaf_count(&self) -> usize;

    /// Extends `out` with all leaf `PortId`s (zero-allocation path).
    fn extend_ids(&self, out: &mut Vec<PortId>);

    /// All leaf `PortId`s, in order.
    fn ids(&self) -> Vec<PortId> {
        let mut v = Vec::new();
        self.extend_ids(&mut v);
        v
    }
}

// ── PortGroup impl for a single Port ─────────────────────────────────────

impl<D: PortValue, S> PortGroup<S> for Port<D, S> {
    fn leaf_count(&self) -> usize {
        1
    }
    fn extend_ids(&self, out: &mut Vec<PortId>) {
        out.push(self.id);
    }
}

// ── PortGroup impl for () — empty group ──────────────────────────────────

impl<S> PortGroup<S> for () {
    fn leaf_count(&self) -> usize {
        0
    }
    fn extend_ids(&self, _out: &mut Vec<PortId>) {}
}

/// Maps a state struct to its generated port group. Only structs used as a
/// *state* (the global app state, or a collection element) derive this —
/// primitive/leaf data types are just [`PortValue`].
pub trait DagStruct: PortValue {
    /// The generated `{Name}Ports` group (`Port<FieldTy, Self>` per field).
    type Ports: PortGroup<Self>;

    /// Constructs the port group with field accessors wired up.
    fn ports() -> Self::Ports;
}

/// A [`PortGroup`] backed by a plain `Vec<PortId>`. Used internally by
/// `Graph::add_node` to wrap output port lists.
pub struct IdGroup {
    pub ids: Vec<PortId>,
}

impl<S> PortGroup<S> for IdGroup {
    fn leaf_count(&self) -> usize {
        self.ids.len()
    }
    fn extend_ids(&self, out: &mut Vec<PortId>) {
        out.extend_from_slice(&self.ids);
    }
}

// ── PortGroup impls for tuples ───────────────────────────────────────────

impl<S, A: PortGroup<S>, B: PortGroup<S>> PortGroup<S> for (A, B) {
    fn leaf_count(&self) -> usize {
        self.0.leaf_count() + self.1.leaf_count()
    }
    fn extend_ids(&self, out: &mut Vec<PortId>) {
        self.0.extend_ids(out);
        self.1.extend_ids(out);
    }
}

impl<S, A: PortGroup<S>, B: PortGroup<S>, C: PortGroup<S>> PortGroup<S> for (A, B, C) {
    fn leaf_count(&self) -> usize {
        self.0.leaf_count() + self.1.leaf_count() + self.2.leaf_count()
    }
    fn extend_ids(&self, out: &mut Vec<PortId>) {
        self.0.extend_ids(out);
        self.1.extend_ids(out);
        self.2.extend_ids(out);
    }
}

impl<S, A: PortGroup<S>, B: PortGroup<S>, C: PortGroup<S>, D: PortGroup<S>> PortGroup<S>
    for (A, B, C, D)
{
    fn leaf_count(&self) -> usize {
        self.0.leaf_count() + self.1.leaf_count() + self.2.leaf_count() + self.3.leaf_count()
    }
    fn extend_ids(&self, out: &mut Vec<PortId>) {
        self.0.extend_ids(out);
        self.1.extend_ids(out);
        self.2.extend_ids(out);
        self.3.extend_ids(out);
    }
}
