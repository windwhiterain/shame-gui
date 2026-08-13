# Feature draft: nested `HashMap<K, E>` fan-out (recursive dirty state)

Status: **implemented 2026-08-14 (core + depth-2 nested node, see §7); this
draft remains the design record.**

## 1. Goal

Today `Graph::add_map_node` fans out over **one layer** of `HashMap<K, E>`
(`crates/shame-gui/src/graph/graph.rs:228-349`):

```rust
graph.add_map_node(map, global_in, global_out, elem_in, elem_out, |gref, gpu, key, e| { ... });
```

We want to support

```rust
HashMap<K, E>   where   E  is a `#[derive(DagStruct)]` state that may itself
                         contain a `HashMap` field
```

so the fan-out recurses: `HashMap<A, E>` → per `E` → `E.children: HashMap<B, Leaf>` →
per `Leaf`, and so on to arbitrary depth.

**The model (agreed):**

- **The graph stays flat.** `Graph<S>` remains a single `Vec<NodeEntry<S>>`
  with no parent/children structure. One map node represents **all** elements
  of a HashMap — that is true at every level. A nested level is just another
  flat node whose eval iterates the outer map and fans out per element.
- **No structural diff.** `known_keys` baselines disappear. `Port::insert` /
  `Port::remove` record the specific key into the dirty records, so the dirty
  system alone decides which elements (and which nested keys) reprocess.
- **Only the dirty state becomes nested.** Everything else stays as is.

---

## 2. Current single-layer mechanism (recap)

Dirty-tracking types in `crates/shame-gui/src/graph/element.rs`:

```rust
pub(crate) struct ElemDirty {
    pub full: bool,                                  // whole element mutated (MapEntry::read_mut)
    pub ports: Rc<RefCell<HashSet<PortId>>>,         // element ports dirtied by the element eval's writes
}

pub(crate) struct MapDirty<K> {
    pub full: bool,                                  // whole map mutated (Port::write / read_mut)
    pub keys: HashMap<K, ElemDirty>,                 // per-key records
}
```

The graph's shared store (`graph.rs:70`, `element.rs:72`):

```rust
map_dirty: Rc<RefCell<HashMap<PortId, Box<dyn Any>>>>,   // PortId → MapDirty<K> (type-erased)
```

`add_map_node` runs four phases per tick (`graph.rs:250-335`):

- **A — decide:** `snapshot_map_dirty::<K>(map_id)`; if `md.full` or a dirty
  global input → reprocess every key; else **structural diff** of
  `known_keys: HashSet<K>` (captured in the closure) to find new/removed keys,
  plus per-key `ElemDirty` records whose `ports` overlap `elem_in`.
- **B — process:** for each changed key, `ensure_elem_ports::<K>(map_id, key)`
  returns the shared element-port set; an element `DagStructRef<E>` is built
  with that set plus a **throwaway** `fired`/`map_dirty`; the single `eval`
  runs; the element is written back.
- **C — remove:** dropped keys are marked so downstream readers re-run.
- **D — refresh** `known_keys`, then `mark_dirty(map_id)` to fan the change
  out to non-map readers.

Why the structural diff exists today: `Port::insert` / `remove`
(`port.rs:252-265`) only call `mark_dirty` (a coarse "this map changed" flag)
without recording *which* key, so `known_keys` is the only way the map node can
tell an insert from a remove. Key-recording (§4.2) removes that need.

---

## 3. Why nesting fails today

**B1 — `map_dirty` is keyed by bare `PortId`, which is scoped to one state
type.** `PortId` numbers fields from 0 per state (`port.rs:62-75`). `S`, the
element `E`, and any inner element `E2` each have their own overlapping
`PortId` space, so a flat `HashMap<PortId, Box<dyn Any>>` cannot express "the
`children` map (id 0 within `E`) of outer key 5 has inner key 3 dirty" — there
is no notion of *which outer key* an inner map belongs to.

**B2 — the element `DagStructRef<E>` is built with a throwaway `map_dirty`
each tick.** `graph.rs:309-312`:

```rust
let emd: Rc<RefCell<HashMap<PortId, Box<dyn Any>>>> =
    Rc::new(RefCell::new(HashMap::new()));
let mut eref = DagStructRef::new_with(&mut e2, ports_rc, fired, emd);
```

Any nested-map dirty marks made inside the eval land in a store that is
dropped at the end of the eval, so a nested level can never observe them.

**B3 — the map-node machinery is hardwired to one level.** The fan-out phases
live inside one node's eval over `DagStructRef<S>`. There is no way to run the
same phases over a nested map *inside* an element, and insert/remove are not
recorded per key, so a nested level has no incremental signal at all.

Note: `PortValue` already has a blanket `impl<K,V> PortValue for HashMap<K,V>`
(`port.rs:47-53`), so `HashMap<u32, HashMap<u32, Leaf>>` already *compiles* as
a value type — the blocker is fan-out, not the value type.

---

## 4. Proposed model

Three changes, all local to `graph/`:

1. **Recursive dirty records** — an element carries its own nested-map dirty
   store (fixes B1 + B2).
2. **Key-recording** — `insert`/`remove` record the specific key; the
   `known_keys` structural diff is deleted ("no diff, dirty system handles
   everything").
3. **Nested fan-out as a flat node** — a nested level is one more `NodeEntry<S>`
   whose eval iterates the outer map and drives the (shared) fan-out phases
   per element (fixes B3). No tree, no handles, no per-element graphs.

### 4.1 Recursive dirty records

Extend `ElemDirty` (`element.rs:24-41`) so each element carries the dirty
state of its *own* nested maps:

```rust
pub(crate) struct ElemDirty {
    pub full: bool,                                  // whole element mutated
    pub added: bool,                                 // key newly inserted this tick (§4.2)
    pub removed: bool,                               // key removed this tick (§4.2)
    pub ports: Rc<RefCell<HashSet<PortId>>>,         // element ports dirtied by the element eval's writes
    /// This element's own nested-map dirty records, keyed by the nested
    /// map's `PortId` within `E`. Recursive: each nested `ElemDirty` may
    /// carry a further `nested`, so depth N needs no new key type and no
    /// cross-key `PortId` collision (scope is structural — the inner `PortId`
    /// lives *inside* the outer key's `ElemDirty`).
    pub nested: Rc<RefCell<HashMap<PortId, Box<dyn Any>>>>,
}
```

`nested` has exactly the same shape as the top-level `map_dirty`, scoped to one
element. The element's `DagStructRef<E>` points at it instead of the throwaway
in B2:

```rust
// inside the map node's Phase B, for element `k`:
let nested = gref.ensure_elem_nested::<K>(map_id, key.clone());
let mut eref = DagStructRef::new_with(&mut e2, ports_rc, fired, nested);
```

Because `DagStructRef::map_dirty` is *already* `Rc<RefCell<HashMap<PortId,
Box<dyn Any>>>>` (`element.rs:72`), `note_full_write` for `HashMap<K,V>`
(`port.rs:47-53`) and `mark_map_full` / `snapshot_map_dirty` /
`ensure_elem_ports` (`element.rs:144-187`) keep working unchanged against the
nested store — they just run one level down.

New `DagStructRef` helper mirroring `ensure_elem_ports`:

```rust
pub(crate) fn ensure_elem_nested<K>(&mut self, id: PortId, key: K)
    -> Rc<RefCell<HashMap<PortId, Box<dyn Any>>>>
```

walks `map_dirty[id] → MapDirty<K>.keys[key] → ElemDirty.nested`, creating
records as needed.

### 4.2 Key-recording (no structural diff)

`Port::insert` / `remove` on a `Port<HashMap<K, V>, S>` record the key in the
dirty record instead of only flagging the map:

```rust
// port.rs — insert: new key → added; overwrite → full (the element is
// replaced wholesale, and its old nested records must not leak).
pub fn insert(&self, r: &mut DagStructRef<'_, S>, key: K, value: V) {
    let existed = self.read_mut_state(r.inner_mut()).insert(key.clone(), value).is_some();
    r.with_map_dirty::<K>(self.id(), |m| {
        let ed = m.keys.entry(key).or_insert_with(ElemDirty::new);
        if existed { ed.full = true } else { ed.added = true }
    });
    r.mark_dirty(self.id);
}

// port.rs — remove: record the removal so downstream readers re-run, no
// reprocessing (the element is already gone).
pub fn remove(&self, r: &mut DagStructRef<'_, S>, key: K) -> Option<V> {
    let removed = self.read_mut_state(r.inner_mut()).remove(&key);
    if removed.is_some() {
        r.with_map_dirty::<K>(self.id(), |m| {
            m.keys.entry(key).or_insert_with(ElemDirty::new).removed = true;
        });
        r.mark_dirty(self.id);
    }
    removed
}
```

`MapEntry::read_mut` keeps setting `full` (`port.rs:198-201`), and whole-map
`Port::write` / `read_mut` keep setting `md.full` via `note_full_write`
(`port.rs:47-53`). Every mutation path now ends in a precise dirty record.

`add_map_node` Phase A then needs **no `known_keys` and no diff**:

- one-shot `started: bool` captured in the closure — first run processes every
  current key (preserves today's first-tick behavior, e.g. seeding via
  `state_mut()` in `tests/snapshot_map_batched_indirect.rs`);
- `md.full` or a dirty global input → process every current key;
- else: `removed` keys → mark `wrote` (downstream re-runs); `added` / `full`
  keys → process; port-dirty keys → process. Phase D (refresh `known_keys`)
  is deleted.

`started` is per map node, not per element, so it never nests.

### 4.3 Nested fan-out as a flat node

Registering a nested level adds **one flat `NodeEntry<S>`** — the graph stays
`Vec<NodeEntry<S>>`, exactly like today:

```rust
pub fn add_map_node_nested<A, E, B, L>(
    &mut self,
    outer_map: Port<HashMap<A, E>, S>,      // the outer map, already owned by a map node
    inner_map:  Port<HashMap<B, L>, E>,     // a map field of the element state E
    elem_in:  impl PortGroup<L>,            // inner element read ports
    elem_out: impl PortGroup<L>,            // inner element write ports
    eval: impl FnMut(&mut DagStructRef<E>, Option<&sm::Gpu>, &B, &mut DagStructRef<L>) + 'static,
) where A: ..., E: Clone + DagStruct + 'static, B: ..., L: Clone + 'static
```

The node's eval (generated by the framework, one shared closure representing
**all** elements at both levels) is the same A/B/C fan-out machinery applied
twice, driven entirely by the dirty records:

1. `snapshot_map_dirty::<A>(outer_map.id())`. The clone **shares** each
   element's `Rc` handles (`ElemDirty` derives `Clone`), so
   `md_outer.keys[a].nested[inner_map_id]` reads the *live* nested records.
2. For each current outer key `a`:
   - **fresh** (`md_outer.full`, or `keys[a].added`/`full`, or the node's own
     first run) → full inner pass: process every inner key of `a`;
   - else, if `keys[a].nested[inner_map_id]` has changes → incremental inner
     pass: process the inner record's `added`/`full`/port-dirty keys, mark
     removed ones;
   - else skip `a`.
3. Processing one inner key: clone `a`, wrap the clone in `DagStructRef<E>`
   sharing `keys[a].ports` + `keys[a].nested`, run the inner phases with the
   shared `eval` (exactly Phase B of `add_map_node`, one level down), write
   the element back.
4. After touching any element: the inner fan-out itself marks `inner_map.id()`
   in `keys[a].ports` (via `run_map_fanout`'s write-back mark), so the outer
   map node's eval re-runs for `a` **in the same tick** — provided the nested
   node is registered **before** the outer map node (map↔map topo edges are
   skipped, so insertion order governs; see §8). The nested node also marks
   `mark_dirty(outer_map.id())` so non-map readers re-derive in the same tick.

Fresh elements need no explicit nested records — the fresh rule *is* the
nested equivalent of `started`, derived from the outer record rather than
stored.

The machinery is factored into a generic helper so `add_map_node` (over
`DagStructRef<S>`) and the nested node (over `DagStructRef<E>`) are the same
code path with different state types; the node list never grows a tree.

---

## 5. API sketch — two-level fan-out

```rust
use std::collections::HashMap;
use shame_gui::DagStruct;
use shame_gui::graph::{DagStructRef, Graph};
use shame_gui::state;

#[derive(Clone, Default, DagStruct)]
struct Leaf { x: f32, y: f32 }

#[derive(Clone, Default, DagStruct)]
struct Group { children: HashMap<u32, Leaf>, total: f32 }

#[state]
#[derive(Clone, Default, DagStruct)]
struct AppState { groups: HashMap<u32, Group> }

let mut graph = Graph::<AppState>::new();
let p = AppState::ports();
let g = Group::ports();
let l = Leaf::ports();

// Flat node #1 — one node, all groups. Sums each group's leaves.
graph.add_map_node(
    p.groups, (), (), g.children, g.total,
    |_gref, _gpu, _key, group: &mut DagStructRef<Group>| {
        let sum: f32 = g.children.read(group).values().map(|c| c.y).sum();
        g.total.write(group, sum);
    },
);

// Flat node #2 — one node, all (group, leaf) pairs. Squares x → y.
// Register BEFORE the outer map node: its per-element marks must reach the
// outer node's `elem_in` (children) in the same tick.
graph.add_map_node_nested(
    p.groups,                // outer map
    g.children,              // Port<HashMap<u32, Leaf>, Group>
    l.x, l.y,                // inner elem_in / elem_out over Leaf
    |_gref, _gpu, _leaf_key, leaf: &mut DagStructRef<Leaf>| {
        let x = *l.x.read(leaf);
        l.y.write(leaf, x * x);
    },
);
```

Deeper nesting is the same call against the level above; each level is one more
flat node.

---

## 6. Semantics (how each mutation cascades)

| operation | level | reprocessing |
|---|---|---|
| `Port::write` / `read_mut` on outer map | S | `md.full` → full refresh of every outer key, recursing into the nested node's full pass |
| `Port::insert` (new key) on outer map | S | `added` → that key's eval runs; nested node sees it fresh → full inner pass |
| `Port::insert` (overwrite) on outer map | S | `full` → reprocess, full inner pass |
| `Port::remove` on outer map | S | `removed` → no reprocess; downstream re-runs; element's nested records die with it |
| `MapEntry::read_mut` on an outer key | S→E | `full` → that element reprocesses, full inner pass |
| element eval writes an `E` port | E | element-port propagation via `keys[a].ports` (outer node's `elem_in`), plus outer non-map readers |
| `Port::write` / `read_mut` on a *nested* map | E | `nested[inner_map_id].full` → full refresh of that element's inner map only |
| `insert` / `remove` on a nested map | E | `nested[inner_map_id]` per-key `added`/`removed` → incremental inner pass |
| clean tick | — | no records → nothing reprocesses at any level |

Invariant to preserve: **removing a key removes all its descendant dirty
records** (they live inside the removed `ElemDirty`), and **`mark_dirty` at
each level fans the change to the next level up** (nested → outer map →
non-map readers), so a leaf edit still reaches a global render node.

---

## 7. Implementation status

**Done (commit `558bc2f` + nested commit):**

1. `element.rs` — `ElemDirty` extended with `added`/`removed`/`nested`;
   `with_map_dirty` is `pub(crate)`; `ensure_elem_nested` added. ✅
2. `port.rs` — `Port::insert` / `remove` record the key (§4.2). ✅
3. `graph.rs` — `known_keys` structural diff replaced with the `started` flag
   + record-driven Phase A; Phase D deleted; Phase B shares the nested store.
   Existing tests pass with zero edits. ✅
4. `graph.rs` — the A/B/C phases are factored into `run_map_fanout`, generic
   over the state type; `add_map_node` is a thin call from its node's eval. ✅
5. `graph.rs` — `add_map_node_nested` added (§4.3): one flat node; the eval
   iterates the outer map per the outer record, runs `process_nested_element`
   (the helper per element against `keys[a].nested`), writes back, marks the
   outer map dirty. ✅
6. `tests/nested_map.rs` — 6 tests: first-tick full fan-out (5 leaves, 2
   groups, same-tick totals), whole-element mutation → only that group
   reprocesses + total updates same-tick, whole-map write → full recursive
   refresh, outer insert → only the new group (fresh → full inner pass),
   outer remove → no reprocessing, clean tick → nothing. ✅

**Not done (future work):** nested batched rendering
(`register_map_render_objects_batched` with an element sub-level), depth > 2
(mechanical: thread the full map path through one more flat node).

---

## 8. Edge cases & open questions

- **Registration order — nested node FIRST, outer map node second.** Map↔map
  edges are skipped in topo sort (`graph.rs:109-119`), so order is insertion
  order. The nested node's per-element marks (`inner_map.id()` in
  `keys[a].ports`) must land **before** the outer node's Phase A runs, or the
  outer eval re-runs a tick late — and since dirty records are wiped at end
  of tick, "late" means never. Register the nested node before the outer map
  node; document this (or add a debug assert).
- **Same-tick totals.** With the correct order, a leaf change reprocesses the
  element's inner pass and the outer node's eval (e.g. `total`) in the same
  tick — verified by `tests/nested_map.rs::whole_element_mutation_...`.
- **Outer eval must not mutate the inner map through element ports.** Marks
  from the outer eval's `children` writes land after the nested node has run
  and are wiped at end of tick, so the nested node would never see them.
  Mutate through whole-element paths instead (`MapEntry::read_mut` /
  `Port::insert` on the outer map, which mark `full`/`added` → fresh → full
  inner pass).
- **Overwrite vs. new key.** `insert` on an existing key must set `full`, not
  `added`, so stale nested records from the replaced element can't mislead
  the nested pass.
- **Depth.** Depth 2 is implemented (`add_map_node_nested`). Deeper levels are
  the same pattern with the full map path threaded through (each level is one
  more flat node; the machinery generalizes mechanically). `Box<dyn Any>`
  downcasts are per-level-typed, so a mismatch is a hard panic (consistent
  with the codebase's panic-fast policy).
- **Element-port reads at the nested level.** The nested node only reacts to
  fresh elements and inner-map changes; a non-map `E` port being dirtied does
  not by itself re-run the inner pass (the inner eval can still read the new
  value when the next pass runs). Acceptable for v1; revisit if needed.

---

## 9. Non-goals / fallback

- **Per-element subgraphs and node trees** are explicitly **not** part of this
  design: the graph is flat, one node per map level, and the dirty system
  drives everything.
- **Manual flattening** (one `Graph<S>` node iterating `outer × inner` in a
  hand-written loop) already works today; it remains the escape hatch for
  ad-hoc cases but loses the incremental per-key bookkeeping.
- **Cross-element reduction / joins** (arbitrary DAG sharing between sibling
  elements) is out of scope: the proposal is tree-shaped fan-out following the
  `HashMap` nesting, matching the existing write-based dirty model.
