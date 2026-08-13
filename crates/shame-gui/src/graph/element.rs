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

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::graph::port::PortId;

/// Dirty tracking for a single element (`key`) of a map port.
#[derive(Clone)]
pub(crate) struct ElemDirty {
    /// The whole element was mutated (`MapEntry::read_mut()`, or an
    /// overwriting `Port::insert`) → reprocess it regardless of which element
    /// ports the node reads.
    pub full: bool,
    /// The key was newly inserted via `Port::insert` → process the new
    /// element once.
    pub added: bool,
    /// The key was removed via `Port::remove` → no reprocessing, but
    /// downstream readers must re-run.
    pub removed: bool,
    /// Element ports dirtied by an element eval's `Port::write`/`read_mut`.
    /// Shared with the element eval's [`DagStructRef`] so writes inside the
    /// eval land here and are visible to downstream map nodes.
    pub ports: Rc<RefCell<HashSet<PortId>>>,
    /// This element's own nested-map dirty records, keyed by the nested
    /// map's `PortId` within the element state. Recursive: each nested
    /// `ElemDirty` may carry a further `nested`, so arbitrary depth needs no
    /// new key type and no cross-state `PortId` collision (scope is
    /// structural — the inner `PortId` lives inside the outer key's
    /// `ElemDirty`).
    pub nested: Rc<RefCell<HashMap<PortId, Box<dyn Any>>>>,
}

impl ElemDirty {
    pub(crate) fn new() -> Self {
        Self {
            full: false,
            added: false,
            removed: false,
            ports: Rc::new(RefCell::new(HashSet::new())),
            nested: Rc::new(RefCell::new(HashMap::new())),
        }
    }
}

/// Per-port map-level dirty record. Stored as a type-erased `Box<dyn Any>`
/// inside the graph's shared dirty state; `Graph::add_map_node` downcasts it
/// back to `MapDirty<K>` for the map's key type.
#[derive(Clone)]
pub(crate) struct MapDirty<K> {
    /// The whole map was mutated (`Port::write`/`read_mut`) → reprocess every
    /// element.
    pub full: bool,
    /// Per-key dirty records (key-level mutation, or element-port writes).
    pub keys: HashMap<K, ElemDirty>,
}

impl<K> MapDirty<K> {
    pub(crate) fn new() -> Self {
        Self {
            full: false,
            keys: HashMap::new(),
        }
    }
}

/// A guarded mutable reference to a state `S`. The inner value is private:
/// code holding a `DagStructRef` can only read/write fields through
/// [`Port`](crate::graph::Port) accessors (`port.read(&r)` /
/// `port.write(&mut r, v)`).
pub struct DagStructRef<'a, S> {
    inner: &'a mut S,
    dirty: Rc<RefCell<HashSet<PortId>>>,
    fired: Rc<RefCell<HashSet<PortId>>>,
    map_dirty: Rc<RefCell<HashMap<PortId, Box<dyn Any>>>>,
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
            map_dirty: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Wraps a mutable reference, sharing the graph's dirty/fired sets.
    pub(crate) fn new_with(
        inner: &'a mut S,
        dirty: Rc<RefCell<HashSet<PortId>>>,
        fired: Rc<RefCell<HashSet<PortId>>>,
        map_dirty: Rc<RefCell<HashMap<PortId, Box<dyn Any>>>>,
    ) -> Self {
        Self {
            inner,
            dirty,
            fired,
            map_dirty,
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

    /// Ensures a [`MapDirty<K>`] record exists for `id` and calls `f` with it.
    /// The `RefMut` temporary is confined to this method, so `f` can mutate
    /// but not escape the record.
    pub(crate) fn with_map_dirty<K: 'static>(
        &mut self,
        id: PortId,
        f: impl FnOnce(&mut MapDirty<K>),
    ) {
        let mut map = self.map_dirty.borrow_mut();
        let entry = map
            .entry(id)
            .or_insert_with(|| Box::new(MapDirty::<K>::new()));
        f(entry
            .downcast_mut::<MapDirty<K>>()
            .expect("map dirty record type mismatch"));
    }

    /// Marks the whole map port dirty (called by `Port::write`/`read_mut` on a
    /// `Port<HashMap<K, V>>`). Fails the type check if a caller routes a
    /// non-map type here.
    pub(crate) fn mark_map_full<K: 'static>(&mut self, id: PortId) {
        self.with_map_dirty::<K>(id, |m| m.full = true);
    }

    /// Clones the accumulated dirty record for a map port (empty if none yet).
    /// Used by `Graph::add_map_node` to decide which keys to reprocess; the
    /// record itself is cleared at end of tick.
    pub(crate) fn snapshot_map_dirty<K>(&self, id: PortId) -> MapDirty<K>
    where
        K: Clone + 'static,
    {
        self.map_dirty
            .borrow()
            .get(&id)
            .and_then(|r| r.downcast_ref::<MapDirty<K>>())
            .cloned()
            .unwrap_or_else(MapDirty::new)
    }

    /// Returns (creating if absent) the shared element-port dirty set for a
    /// specific key of a map port. The element eval's [`DagStructRef`] shares
    /// this set so its `Port::write`/`read_mut` marks land here.
    pub(crate) fn ensure_elem_ports<K>(
        &mut self,
        id: PortId,
        key: K,
    ) -> Rc<RefCell<HashSet<PortId>>>
    where
        K: Clone + Eq + std::hash::Hash + 'static,
    {
        let mut map = self.map_dirty.borrow_mut();
        let entry = map
            .entry(id)
            .or_insert_with(|| Box::new(MapDirty::<K>::new()));
        let record = entry
            .downcast_mut::<MapDirty<K>>()
            .expect("map dirty record type mismatch");
        record
            .keys
            .entry(key)
            .or_insert_with(ElemDirty::new)
            .ports
            .clone()
    }

    /// Returns (creating if absent) the shared nested-map dirty store for a
    /// specific element of a map port. An element eval's [`DagStructRef`]
    /// uses this as its `map_dirty`, so nested-map writes inside the eval
    /// land here and are visible to nested fan-out nodes.
    pub(crate) fn ensure_elem_nested<K>(
        &mut self,
        id: PortId,
        key: K,
    ) -> Rc<RefCell<HashMap<PortId, Box<dyn Any>>>>
    where
        K: Clone + Eq + std::hash::Hash + 'static,
    {
        let mut map = self.map_dirty.borrow_mut();
        let entry = map
            .entry(id)
            .or_insert_with(|| Box::new(MapDirty::<K>::new()));
        let record = entry
            .downcast_mut::<MapDirty<K>>()
            .expect("map dirty record type mismatch");
        record
            .keys
            .entry(key)
            .or_insert_with(ElemDirty::new)
            .nested
            .clone()
    }

    /// Clones the shared map-dirty record map, so a `MapEntry` can record its
    /// key as dirty after its `&mut V` borrow ends (Drop).
    pub(crate) fn map_dirty_rc(&self) -> Rc<RefCell<HashMap<PortId, Box<dyn Any>>>> {
        self.map_dirty.clone()
    }

    /// Clones the shared dirty set, so a `MapEntry` can mark its port dirty
    /// after its `&mut V` borrow ends (Drop).
    pub(crate) fn dirty_rc(&self) -> Rc<RefCell<HashSet<PortId>>> {
        self.dirty.clone()
    }
}
