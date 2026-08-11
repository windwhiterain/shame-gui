//! The guarded state reference: [`DagStructRef`].
//!
//! Nodes receive a `&mut DagStructRef<S>` instead of a raw `&mut S`, so they
//! can only touch fields through typed [`Port`](crate::graph::Port) handles —
//! the inner state (and any raw `PortId`) is never exposed. The global
//! [`BuiltinState`](crate::graph::BuiltinState) and every collection element are wrapped in
//! the same [`DagStructRef`], keeping them symmetric.
//!
//! Dirty and fired tracking live behind [`Rc<RefCell>`] handles shared with
//! the [`Graph`](crate::graph::Graph): a [`Port::write`](crate::graph::Port::write)
//! / `read_mut` marks the port dirty, and [`Port::fire`](crate::graph::Port::fire)
//! marks it fired. A standalone [`DagStructRef::new`] carries fresh, isolated
//! sets (useful for widget unit tests and custom widget code).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::graph::port::PortId;

/// A guarded mutable reference to a state `S`. The inner value is private:
/// code holding a `DagStructRef` can only read/write fields through
/// [`Port`](crate::graph::Port) accessors (`port.read(&r)` /
/// `port.write(&mut r, v)`).
pub struct DagStructRef<'a, S> {
    inner: &'a mut S,
    dirty: Rc<RefCell<HashSet<PortId>>>,
    fired: Rc<RefCell<HashSet<PortId>>>,
}

impl<'a, S> DagStructRef<'a, S> {
    /// Wraps a mutable reference with fresh, isolated dirty/fired tracking.
    /// Use this for widget unit tests and any code that drives a state
    /// outside a [`Graph`](crate::graph::Graph) tick.
    pub fn new(inner: &'a mut S) -> Self {
        Self {
            inner,
            dirty: Rc::new(RefCell::new(HashSet::new())),
            fired: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    /// Wraps a mutable reference, sharing the graph's dirty/fired sets.
    pub(crate) fn new_with(
        inner: &'a mut S,
        dirty: Rc<RefCell<HashSet<PortId>>>,
        fired: Rc<RefCell<HashSet<PortId>>>,
    ) -> Self {
        Self {
            inner,
            dirty,
            fired,
        }
    }

    /// Immutable access to the underlying state (used by `Port::read`).
    pub(crate) fn inner<'r>(&'r self) -> &'r S {
        &*self.inner
    }

    /// Mutable access to the underlying state (used by `Port::write`).
    pub(crate) fn inner_mut<'r>(&'r mut self) -> &'r mut S {
        &mut *self.inner
    }

    /// Marks a port dirty (called by `Port::write` / `read_mut`).
    pub(crate) fn mark_dirty(&mut self, id: PortId) {
        self.dirty.borrow_mut().insert(id);
    }

    /// Marks a condition port fired (called by `Port::fire`).
    pub(crate) fn mark_fired(&mut self, id: PortId) {
        self.fired.borrow_mut().insert(id);
    }

    /// Whether a port is dirty (used by `Graph::tick`).
    pub(crate) fn is_dirty(&self, id: PortId) -> bool {
        self.dirty.borrow().contains(&id)
    }
}
