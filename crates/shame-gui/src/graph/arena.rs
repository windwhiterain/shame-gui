//! Typed, append-only value storage shared by the DAG and the GUI.

use std::any::Any;
use std::collections::HashSet;

use crate::graph::port::{PortId, PortValue};

/// Central value storage. Every port value (widget data, DAG outputs,
/// render targets) lives here. PortId doubles as the index into `slots`.
///
/// Slots are never deleted — the arena is append-only.
pub struct StateArena {
    slots: Vec<Box<dyn Any>>,
    dirty: HashSet<PortId>,
}

impl StateArena {
    /// Creates an empty arena.
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            dirty: HashSet::new(),
        }
    }

    /// Allocate a new typed slot (default-initialized). New slots are
    /// born dirty so the first frame processes them.
    pub fn alloc<T: PortValue>(&mut self) -> PortId {
        self.alloc_with(T::default())
    }

    /// Allocate a typed slot with an initial value. New slots are
    /// born dirty so the first frame processes them.
    pub fn alloc_with<T: PortValue>(&mut self, value: T) -> PortId {
        let bx: Box<dyn Any> = Box::new(value);
        let id = PortId::new(self.slots.len() as u64);
        self.slots.push(bx);
        self.dirty.insert(id);
        id
    }

    /// Read a value by reference. Panics on invalid id or type mismatch.
    pub fn read<T: PortValue>(&self, id: PortId) -> &T {
        self.slots[id.index() as usize]
            .downcast_ref::<T>()
            .expect("StateArena::read: type mismatch")
    }

    /// Write a value into a slot and mark it dirty. Panics on invalid id
    /// or type mismatch.
    pub fn write<T: PortValue>(&mut self, id: PortId, value: T) {
        *self.slots[id.index() as usize]
            .downcast_mut::<T>()
            .expect("StateArena::write: type mismatch") = value;
        self.dirty.insert(id);
    }

    /// Returns a mutable reference to a slot value AND marks it dirty.
    /// Use this for in-place mutation (e.g. DAG nodes that update GPU
    /// resources). Panics on invalid id or type mismatch.
    pub fn read_mut<T: PortValue>(&mut self, id: PortId) -> &mut T {
        self.dirty.insert(id);
        self.slots[id.index() as usize]
            .downcast_mut::<T>()
            .expect("StateArena::read_mut: type mismatch")
    }

    /// Mark a slot as dirty. The graph's next tick will evaluate nodes
    /// reading from this slot, and Canvas will re-draw any render
    /// bindings pointing at this slot.
    pub(crate) fn mark_dirty(&mut self, id: PortId) {
        self.dirty.insert(id);
    }

    /// True if the slot is dirty.
    pub(crate) fn is_dirty(&self, id: PortId) -> bool {
        self.dirty.contains(&id)
    }

    /// Drain dirty flags after all consumers (graph + canvas) have
    /// processed the frame.
    pub(crate) fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Number of allocated slots.
    pub fn len(&self) -> usize {
        self.slots.len()
    }
}

impl Default for StateArena {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for StateArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateArena")
            .field("slots", &self.slots.len())
            .field("dirty", &self.dirty.len())
            .finish()
    }
}
