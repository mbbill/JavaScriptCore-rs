# Structures as GC cells — keystone design

Status: **ratifiable draft**. Not yet implemented. Scope: make `Structure` a
GC-managed cell in the S4 arena, closing the biggest remaining unbounded
population (every property transition mints a `Structure` that today lives
forever in `StructureIdTable::structures: Vec<Structure>`).

This document is written *within* three decisions the orchestrator has already
ratified from a completed scoping audit — they are not re-litigated here:

- **R1** — dedicated Structure space: its own directory/kind in the existing
  `MarkedSpace` machinery (`gc/heap/marked_space.rs`, `directories: HashMap<usize,
  BlockDirectory>`), mirroring JSC's `structureSpace` `IsoSubspace`
  (`heap/Heap.h:160`) — **not** `CoreObjectStore::space` (the object arena).
- **R2** — `StructureId` keeps its registry-handle public API (Vec-index-
  shifted-by-1 + nuke bit, `object/structure_cell.rs:140-158`); only the
  *backing* changes so the same `SlotVisitor`/`is_arena_cell` machinery can mark
  Structures. The handle→arena-address mapping is this document's to draft.
- **R3** — the `structure.rs` / `structure_cell.rs` fork resolves toward the
  live concept: `structure_cell::Structure` survives; `object/structure.rs` is
  retired, harvesting its arena-cell scaffolding where genuinely useful.

All C++ citations are from the local `WebKit/Source/JavaScriptCore` checkout
(`/Users/bytedance/Dev/WebKit`). All Rust citations are from this worktree.
`mcts_mem/javascriptcore/object-model/structure-shapes.md` (+ its `.alt/`
siblings) is read-only JSC authority and is cited where it settles a "why."

---

## 0. What "Structure cell" plugs into today (orientation)

Three facts, established by reading the live R4 arena and its first three
tenants, control every choice below:

1. **Marking is address-global, not space-scoped.** `test_and_set_marked` /
   `is_marked` / `block_for` (`gc/heap/marked_block.rs:266,356,370`) take a bare
   `usize` and derive the owning `MarkedBlock` by `addr & BLOCK_MASK`; they read
   the block's own embedded mark bitmap. They take no `&MarkedSpace`. This means
   a **second, independent `MarkedSpace` instance can be marked by the same
   `SlotVisitor` in the same drain** as the first — mark state lives in block
   headers, not in the `MarkedSpace` struct. This is what makes "own directory,
   own space" (R1) compatible with "one fixpoint over the whole live graph."
2. **A `MarkedSpace` is a plain, independently-instantiable value** (`gc/heap/
   marked_space.rs:398-413`, `MarkedSpace::new()`/`Default`). `CoreObjectStore`
   owns one (`space: MarkedSpace`, `object_store.rs:108`). Nothing prevents a
   second store from owning a second instance — that is the literal shape R1
   asks for.
3. **Every existing arena-cell kind is POD (no `Drop`)** — `CoreObjectCell`
   (`object_store.rs:775-778`), `CoreStringCell`/`CoreRopeStringCell`
   (`interpreter/string_store.rs:199-202`) all carry a `needs_drop::<T>() ==
   false` compile-time assert, because `MarkedSpace::sweep_all_object_blocks`
   (`marked_space.rs:972-981`) reclaims a dead cell's atoms with **no destructor
   call** — cleanup is done ahead of sweep by store-owned `reconcile_dead_*`
   methods that free out-of-line slab slots while the cell's bytes are still
   intact (`object_store.rs:3840-3916`, `string_store.rs:635`). Any large,
   variable, heap-owning payload is relocated off-cell to a store-owned slab
   reached by a `Copy` handle — the established "SD-4" pattern (used for
   RegExp/Promise/BoundFunction/Map/Set on `CoreObjectCell`, and for the whole
   `StringImpl` payload on `CoreStringCell`).

`Structure` today (`object/structure_cell.rs:284-349`) owns a `PropertyTable`
(`Vec<u32>` + `Vec<PropertyTableEntry>`, `object/property_table.rs:225-241`)
and a `StructureTransitionTable` (a `HashMap` once promoted past the single
slot, `object/structure_transition_table.rs`) — **not POD, not `Copy`**. So
Structure cannot become an arena cell by embedding `Structure` verbatim; it
must follow the same SD-4 relocation the other three kinds already use. This
one fact anchors the layout in §1.

---

## 1. The Structure cell layout

### 1.1 Fixed POD cell (lives in the arena)

```rust
#[repr(C)]
pub(crate) struct StructureArenaCell {
    structure_id: gc::StructureId,   // @0  (4 bytes) — see §4
    js_type: JsType,                 // @4  (1 byte)  — JsType::Structure (new variant, §1.3)
    // bytes 5,6: reserved (mirrors CoreObjectCell/CoreStringCell padding)
    // byte 7: MARKER-OWNED — SlotVisitor::set_cell_state writes m_cellState
    //         here (slot_visitor.rs:144-153); no cell struct may claim it.
    own_handle: u32,                 // @8  (4 bytes) — this Structure's OWN registry handle
    // bytes 12..16: padding to the 16-byte (1-atom) size class
}
const _: () = assert!(std::mem::size_of::<StructureArenaCell>() == 16);
const _: () = assert!(std::mem::offset_of!(StructureArenaCell, structure_id) == 0);
const _: () = assert!(std::mem::offset_of!(StructureArenaCell, js_type) == 4);
const _: () = assert!(std::mem::offset_of!(StructureArenaCell, own_handle) == 8);
const _: () = assert!(!std::mem::needs_drop::<StructureArenaCell>());
```

This is a direct copy of the established convention, not a new one:
`structure_id@0` / `js_type@4` / byte-7-reserved-for-the-marker is exactly
`CoreStringCell`'s layout (`interpreter/string_store.rs:83-118,164-202`), which
in turn mirrors `CoreObjectCell`'s (`object_store.rs:564-582,814-822`) and the
real C++ prefix (`m_structureID@0` `u32`, then the `m_indexingTypeAndMisc/
m_type/m_flags/m_cellState` blob at `4..8`, `runtime/JSCell.h:293-302`). 16
bytes = 1 `MarkedBlock` atom (`ATOM_SIZE == 16`, `marked_block.rs:99`) — the
same size class `CoreStringCell` uses, in a *different* `MarkedSpace` instance
(different block pool; no collision, see §0.2).

**Why `own_handle` and not nothing.** C++ needs no such field: `StructureID`
*is* a masked, truncated Structure heap address (`StructureID::encode`,
`runtime/StructureID.h:90-97` — Structures live in a dedicated ~4 GiB address
reservation so a 32-bit offset addresses any of them). R2 keeps the port's
registry-handle model instead (Vec-index-shifted-by-1), which is a *second*,
independent numbering scheme from the arena address. Something must translate
address→handle for the one consumer that walks by address, not handle: the
tracer (§3). Every other arena-cell kind in this port avoids that problem by
having only ONE address-shaped identity; Structure is the first kind with two,
which is why it is the first kind needing this extra field. Storing the
handle *in* the cell (a back-pointer) is the cheapest translation — O(1), no
side table — and matches how `CoreObjectCell` stores its own out-of-line
handles (`butterfly: ButterflyHandle`, `regexp_source: AuxiliaryHandle`, …)
directly in the cell rather than through a side `HashMap`.

**Why not embed more fields (avoid a slab for small structures).** C++'s
`Structure` is intentionally *not* size-compacted — it is one `IsoSubspace`
sized to `sizeof(Structure)` (`FOR_EACH_JSC_STRUCTURE_ISO_SUBSPACE`,
`heap/Heap.h:160`, `destructibleCellHeapCellType` — C++ *does* run a
destructor at sweep for Structure, unlike the port's POD-only sweep, see §0.3).
Growing the port's fixed cell to fit every field would mean either (a) making
the whole space `Drop`-aware, which no existing sweep path supports, or (b)
keeping `PropertyTable`/`StructureTransitionTable` `Copy`, which they
structurally cannot be. Reusing the proven SD-4 slab (§1.2) costs one raw
u64-sized indirection and reuses machinery already miri-verified for three
other kinds — the cheapest path that stays inside the existing sweep contract.

### 1.2 Off-cell record slab

`StructureIdTable` (`object/structure_cell.rs:588-590`) already owns
`structures: Vec<Structure>`, handle-indexed (`structures[handle.raw() - 1]`).
That Vec **is** the record slab — it does not need a new type. What changes is
only that each `Structure` in it also gets a companion arena cell (§2), and
that a live `Structure`'s *own* state is unchanged (same fields, same
methods: `property_table: Option<PropertyTable>`, `transition_table:
StructureTransitionTable`, `previous: Option<StructureHandle>`, `prototype:
PrototypePointer`, …, `object/structure_cell.rs:284-349`). No field on
`Structure` moves for this unit; the existing struct already *is* what C++
calls the out-of-line payload.

### 1.3 `JsType::Structure`

`runtime/js_type.rs` has no `Structure` variant yet. Add one at the *true*
C++ positional discriminant, per the file's own stated convention ("the u8
discriminants are the TRUE C++ positional values... so the `>= ObjectType`
predicate stays valid," `js_type.rs:19-23`):

```rust
/// JSC `StructureType` (runtime/JSType.h:33, second entry in FOR_EACH_JS_TYPE
/// after CellType==0, so StructureType == 1).
Structure = 1,
```

C++ evidence for the discriminant: `runtime/JSType.h:30-33` —
`FOR_EACH_JS_TYPE` lists `macro(CellType, ...)` first, `macro(StructureType,
...)` second; `CellType` is the base value 0, so `StructureType == 1`. This
keeps `Structure < String(2) < ... < Object(32)`, so `JsType::is_object()`
(`>= Object`) correctly excludes Structure, matching `TypeInfo::isObject`
being false for a Structure's own cell (C++ `Structure`'s `typeInfo().type()
== StructureType`, never `>= ObjectType`). Extend the `cell_type()` bridge
(`js_type.rs:64-70`) with `JsType::Structure => CellType::Structure` (the
`gc::CellType::Structure` variant already exists and is unused today, `gc/
cell.rs:54`).

---

## 2. Handle → address mapping

### 2.1 What changes, precisely

```rust
pub struct StructureIdTable {
    structures: Vec<Structure>,   // UNCHANGED — the record slab (§1.2)
    arena_addrs: Vec<usize>,      // NEW — parallel, handle-indexed: handle → arena cell address
    space: MarkedSpace,           // NEW — the dedicated Structure space (R1)
}
```

`arena_addrs` is populated 1:1 with `structures` by `register()`
(`structure_cell.rs:600-605`): every `register()` call additionally allocates
a 16-byte `StructureArenaCell` (`space.allocate_blob(..)`, mirroring
`CoreObjectStore::allocate_cell`'s use of `allocate_blob`,
`marked_space.rs:486-527`), writes `own_handle = <the handle just assigned>`
into it, and pushes the returned `CellPtr::addr()` onto `arena_addrs`. The two
Vecs stay in lockstep by construction (both grow only through `register`,
both are handle-indexed from the same base).

**`StructureId`'s public encode/decode surface does not change.** Two
distinct types currently share the name "StructureId," and only one of them
is load-bearing on the hot path — this is worth being precise about, because
R2's "keep the public API" is about the load-bearing one:

- `crate::gc::StructureId` (`gc/cell.rs:20-34`, bare `pub struct
  StructureId(pub u32)`, `INVALID = Self(0)`) is what `StructureIdTable`
  actually hands out (`register`'s `StructureHandle::new((self.structures
  .len() + 1) as u32)`, `structure_cell.rs:601`) and what every consumer
  carries: `CoreObjectCell::structure_id` (`object_store.rs:567`), `bytecode/
  ic.rs`'s `base_structure`/`holder_structure`/`new_structure` fields,
  `runtime/realm.rs`'s `object_structure`/`function_structure`, etc. This is
  the type R2 means by "Vec-index (+1)" — it is the raw 1-based handle, no
  shift, no nuke bit.
- `object::structure_cell::StructureId` (`structure_cell.rs:92-158`, WITH
  `encode`/`decode`/`nuke`/`CELL_STRUCTURE_ID_OFFSET`) is a *separate*,
  already-written, C++-`StructureID.h`-faithful wrapper — the one R2's prompt
  cites at "140-158." Grep confirms it is **never called** anywhere in the
  live interpreter (`encode`/`decode`/`CELL_STRUCTURE_ID_OFFSET` have zero
  non-test references outside `structure_cell.rs`). It exists as a faithful
  leaf port of the encode/decode/nuke *shape*, ready for the day the port
  reserves a real nuke bit (JSC's concurrent structure/butterfly race guard,
  `runtime/JSObject.h` "nuked StructureID between structure-size and
  butterfly updates," StructureInlines commit `12e75c3d` in `mcts_mem`) —
  which this single-threaded STW collector does not need yet.

  This unit does not need to change or wire `structure_cell::StructureId`
  either — it is already correct and already unused; this design just leaves
  it alone. **The type that must keep its bit-shape and API is `gc::
  StructureId`: a plain 1-based `u32` Vec index**, and this design's backing
  swap (`Vec<Structure>` → `Vec<Structure>` + `Vec<usize>` + `MarkedSpace`)
  changes nothing about how that value is produced, compared, stored in
  `Option<StructureId>`, or read back — every one of the ~30 `self
  .structure_table.structure(handle)` call sites keeps its exact signature.

### 2.2 Cost on the hot path (bounded, and the bound is zero extra indirection)

Property lookup (`get_by_id`/`put_by_id`, the hottest structure consumer)
calls `StructureIdTable::structure(handle) -> &Structure`
(`structure_cell.rs:609-611`). Under this design that call is **UNCHANGED**:

```rust
pub fn structure(&self, handle: StructureHandle) -> &Structure {
    &self.structures[(handle.raw() - 1) as usize]   // exactly today's line
}
```

`arena_addrs` is written *by* `register`/mutation paths and read *only* by
GC code (marking, the reconcile/sweep pass, conservative-scan discovery) —
never by ordinary property/transition lookups. This is the deliberate
consequence of keeping `structures: Vec<Structure>` as the record slab
instead of routing every read through the arena (the two-hop "handle → addr →
raw-read-a-field → slab" design considered and rejected below). **The bound
the audit asked for is: zero added indirection on the read path that runs on
every property access** (`structure_table.structure(sid).property_table_or_
null()`, `object_store.rs:6647`,`6748`,`6817`, etc. all keep their current
cost). The only new cost lands on paths that are already GC-shaped: `register`
pays one more `allocate_blob` call (an existing, cheap, already-amortized
bump-allocator-style path), and the tracer pays one raw 4-byte read per
Structure visited during a collection (§3) — collections are infrequent
relative to property reads by construction (`BYTES_ALLOCATED_GC_THRESHOLD`,
`marked_space.rs:365-366`).

**Design alternative considered and rejected**: make the arena cell the
"real" storage — `arena_addrs` holds the *only* handle→address map, and
`structure()` derefs the arena cell, raw-reads `own_handle`... no — that's
circular; the real rejected alternative was **removing `structures: Vec<
Structure>` as a plain slab and reaching every field through `arena_addrs[h]
→ raw-read a slab-index field @ offset 8 → structure_records[idx]`** (the
`CoreStringStore` shape, where the cell carries no payload and everything
routes through the store). Rejected because: (a) it adds an `unsafe` raw
read to the single hottest Structure-reading path in the interpreter, where
the *existing* three arena tenants (String/Symbol/BigInt) are leaf-shaped and
comparatively cold next to property lookup; (b) `own_handle == slab index`
already (`structures[handle-1]`), so the raw read would recover a number the
caller already has for free — pure waste. Keeping `structures: Vec<
Structure>` as the plain record slab and treating `arena_addrs`/the cell as a
GC-only shadow index is strictly cheaper and is what §2.1 specifies.

### 2.3 Membership / cross-space marking

`is_arena_cell` (`marked_space.rs:754`) is a method on `MarkedSpace`, scoped
to *that instance's* directories/blocks/precise-set. With two independent
`MarkedSpace` instances (object, structure), a raw edge address must be
routed to the space that owns it before it can be dereferenced. This is a
real, new cross-cutting piece — not a hidden cost on the hot path (it only
runs during marking), but a genuine architecture point flagged for §6/§7.
JSC's `HeapUtil::isPointerGCObjectJSCell` (`heap/HeapUtil.h:54-79`) checks a
**single** Heap-wide membership structure spanning every subspace; the port's
per-store `blocks`/`precise_set` fields are already a divergence from that
(pre-dating this unit — `CoreObjectStore::space` is already the only
membership authority for the object arena). This design does not fix that
pre-existing divergence; it extends it by one more store, which is bounded
(exactly 2 probes: try `object_store.space.is_arena_cell`, else `structure_
table.space.is_arena_cell`) and documented as an open question in §8 for
whether a combined membership index is worth building before a third space
appears.

---

## 3. `trace_structure` edges

### 3.1 The C++ body being ported

`Structure::visitChildrenImpl` (`runtime/Structure.cpp:1401-1458`):

```cpp
Base::visitChildren(thisObject, visitor);              // JSCell base: the m_structureID edge
visitor.append(thisObject->m_realm);                    // STRONG
if (!thisObject->isObject()) {
    thisObject->m_cachedPrototypeChain.clear();
} else {
    visitor.append(thisObject->m_prototype);             // STRONG, object structures only
    visitor.append(thisObject->m_cachedPrototypeChain);   // STRONG, object structures only
}
visitor.append(thisObject->m_previousOrRareData);        // STRONG

if (isPinnedPropertyTable() || protectPropertyTableWhileTransitioning())
    visitor.append(m_propertyTableUnsafe);                // STRONG, conditional
else if (isAnalyzingHeap())
    visitor.append(m_propertyTableUnsafe);                // STRONG, heap-snapshot only
else if (m_propertyTableUnsafe)
    m_propertyTableUnsafe.clear();                        // CLEAR (drop the reference)

// variant-specific children (Branded/WebAssemblyGC) — not modeled in this port

if (!(collectionScope() == CollectionScope::Full))
    if (auto* transition = m_transitionTable.trySingleTransition())
        visitor.appendUnbarriered(transition);            // conditionally-strong, eden only
```

### 3.2 What each edge maps to in the port, field by field

| C++ edge | Port field (`structure_cell.rs`) | Verdict for this unit |
|---|---|---|
| `Base::visitChildren` (own `m_structureID`) | `StructureArenaCell::structure_id` | **traced** — same mechanism as every other cell's base-class edge, §4 |
| `m_realm` | *not modeled* (`Structure` doc: "Fields not modeled here... m_realm/m_cachedPrototypeChain," `structure_cell.rs:265-271`) | **no-op today** — no field exists to trace. Flagged as a future-unit dependency, not invented here |
| `m_prototype` (if `isObject()`) | `prototype: PrototypePointer(usize)` | **traced, conditionally** — see §3.3 |
| `m_cachedPrototypeChain` | *not modeled* | **no-op today**, same as `m_realm` |
| `m_previousOrRareData` | `previous: Option<StructureHandle>` (`StructureRareData` explicitly out of scope, `structure_cell.rs:299-301`) | **traced, STRONG**, unconditionally (no rare-data branch to model) |
| `m_propertyTableUnsafe` (conditional) | `property_table: Option<PropertyTable>` | **conditional clear**, not a cell edge — see §3.4 |
| `m_transitionTable.trySingleTransition()` (eden-only) | `transition_table: StructureTransitionTable` | **never traced from here** — always weak, see §5 |

### 3.3 Prototype edge

C++ only traces `m_prototype` when `thisObject->isObject()` — i.e., when
*this Structure's own* `TypeInfo` says the values carrying it are objects
(`Structure.h`, `TypeInfo::isObject`). Non-object structures (the ones
`vm.stringStructure`/`vm.symbolStructure`-equivalents would be, if the port
modeled them) have no meaningful `[[Prototype]]` slot to protect. The port
already carries the equivalent bit: `Structure::type_info_blob().ty()` is a
raw `JSType` byte (`structure_cell.rs:186-196` doc: "the type byte is a raw
`JSType` value... carried as `u8`"). The trace body:

```rust
// Structure::visitChildrenImpl (Structure.cpp:1420-1429), the prototype half.
if JsType::from_raw(structure.type_info_blob().ty()).is_object() {
    if let Some(addr) = as_traceable_addr(structure.prototype) {
        visitor.append_unbarriered(addr); // routes to whichever store owns addr
    }
}
```

`prototype: PrototypePointer(usize)` is currently documented as carrying "the
prototype object's pointer rep... The write barrier is a no-op here"
(`structure_cell.rs:231-237`). Promoting it to a real traced edge is exactly
the harvestable idea from `object/structure.rs`'s `Trace for Structure`
(`tracer.visit_cell(prototype)`, `object/structure.rs:169-175`) — see §7 for
why the *code* there is not reusable (wrong header/visitor types) even though
the *requirement* (prototype must be a real edge) is.

### 3.4 PropertyTable: conditional clear, not a cell edge

The port's `PropertyTable` is owned **inline by value**
(`property_table: Option<PropertyTable>`, `structure_cell.rs:334-339`), not
behind a `WriteBarrier<PropertyTable>` pointer to a separately-allocated
JSCell as in C++ (`runtime/PropertyTable.h:85`, `class PropertyTable final :
public JSCell`). This is a pre-existing, already-documented divergence
(`structure_cell.rs` module doc explicitly integrates `PropertyTable` "as the
materialized lookup table" directly, not as a cell). This unit does not
promote `PropertyTable` to its own arena cell — that is a distinct, separable
future unit (own `IsoSubspace`-analog, own `Drop`/finalize wiring), out of
scope here, and the mcts_mem tree's own history explains *why* C++ did it
that way: `property-table-heap-allocated` (`mcts_mem/javascriptcore/
object-model/structure-shapes.alt/property-table-heap-allocated.md`,
`2013-02-26 f7da71f2`) — "Unpinned Structure property tables were never freed
while the Structure was alive... making PropertyTable a GC-managed JSCell
allows `Structure::visitChildren` to null out `m_propertyTable` for unpinned
tables so the GC can collect them," a **measured 14 MB save on Membuster3**.

What *is* in scope, and cheap, is the **effect** of that change: the
conditional clear. The port doesn't need a separate cell to get the same
memory win, because the table is inline-owned — clearing it is just dropping
the `Vec`s directly:

```rust
// Structure::visitChildrenImpl (Structure.cpp:1443-1449). Not modeled:
// protectPropertyTableWhileTransitioning() (concurrent-compiler-only) and
// isAnalyzingHeap() (heap-snapshot feature) — neither exists in this port, so
// the faithful reduction is: pinned tables survive, everything else is
// evicted every mark cycle and rebuilt lazily by materialize_property_table's
// replay (structure_cell.rs:800-855) on next access.
fn trace_structure_property_table(structure: &mut Structure) {
    if !structure.is_pinned_property_table() {
        structure.property_table = None; // drop the Vecs; steal-semantics unaffected
    }
    // (a pinned table is never cleared — it is the dictionary's owned copy)
}
```

This is a genuine, C++-faithful, historically-justified behavior change (not
merely "faithful for its own sake") — the mcts_mem `Facts` note it directly:
"making PropertyTable a GC-managed cell allowed unpinned Structure tables to
be collected and removed a 14 MB waste." Because `materialize_property_table`
already exists and is already tested to replay exactly (`structure_cell.rs`
tests `materialize_replays`), turning this on costs one `if` in the trace
body and zero new machinery. Flagged in §6 as its own migration step because
it is an observable *performance* behavior change (more table rebuilds after
a GC) worth its own probe before landing, even though it requires no new
types.

### 3.5 Where this plugs into the existing dispatch shape

The existing `VisitChildren`/`SlotVisitor` machinery already has the exact
shape needed — `ObjectGraphMarker` (`object_store.rs:3189-3241`) dispatches
on a membership-gated header read (`arena_cell_kind_at`,
`object_store.rs:3175-3181`) to either `trace_cell` (Object) or
`trace_leaf_cell` (String/Symbol/BigInt), and `trace_leaf_cell`
(`object_store.rs:3368-3410`) further sub-dispatches on `js_type` to decide
whether a leaf kind carries edges (only ropes do, via `string_cell_fibers`).
`trace_structure` is a new sibling body at that same level: given a
membership-gated `StructureArenaCell` address (proved by `structure_table.
space.is_arena_cell`), read `own_handle`, index `structures[own_handle - 1]`,
and walk §3.2's edges through ordinary safe `&Structure` field access (not
raw byte offsets — unlike the rope fiber trace, Structure's edges are typed
Rust fields once the address→handle step is done, so no further `unsafe`
is needed past that one read). See §6 for the *combined* dispatcher this
requires once two spaces exist.

---

## 4. The base-class edge: `structure_id@0` on every cell

C++: `JSCell::visitChildrenImpl` is one line — `visitor.appendUnbarriered
(cell->structure())` (`runtime/JSCellInlines.h:130-134`) — **every** JSCell,
with no exceptions, traces its own structure. This is the "base-class edge"
every subtype's `visitChildrenImpl` starts with (`Base::visitChildren
(thisObject, visitor)`, seen verbatim in `Structure::visitChildrenImpl`
itself, `Structure.cpp:1406`).

### 4.1 What every Structure cell's OWN `structure_id` actually holds

Traced from ctor to ctor, this is fully determined, not a design choice:

- `Structure::Structure(VM&, JSGlobalObject*, JSValue prototype, ...)`
  (`runtime/Structure.cpp`, the ordinary non-bootstrap ctor) initializes
  `: JSCell(vm, vm.structureStructure.get())` — i.e. **every ordinary
  Structure's own header `m_structureID` points at `vm.structureStructure`**,
  the single well-known "structure of structures" (JSC's meta-shape).
- `JSCell::JSCell(VM&, Structure* structure)` (`runtime/JSCellInlines.h:61-65`)
  sets `m_structureID = structure->id()` from that argument — confirming the
  above is where the value comes from, uniformly, for every cell kind
  including Structure.
- The **bootstrap** exception: `Structure::createStructure(VM&)`
  (`runtime/StructureCreateInlines.h:82-88`) constructs `vm.structureStructure`
  itself via the `Structure(VM&, CreatingEarlyCellTag)` ctor, which calls
  `JSCell(CreatingEarlyCellTag)` (`runtime/JSCellInlines.h:55-59`) — this
  sub-ctor does **not** set `m_structureID` at all. `finishCreation(vm,
  CreatingEarlyCell)` then runs (`StructureCreateInlines.h:59-64` →
  `JSCell::finishCreation(vm, structure, CreatingEarlyCellTag)`,
  `JSCellInlines.h:112-127`), which does `m_structureID = structure->id()`
  where **`structure` is the early-cell object itself** (`this`) — so
  `vm.structureStructure` ends up **self-referential**: its own
  `m_structureID` encodes its own address (`StructureID::encode(this)`,
  `runtime/StructureID.h:90-97`), computed *after* the cell is already
  allocated at that address, so `id()` can name itself.

### 4.2 Port mapping

This is directly portable with the registry-handle model, and gives a clean
answer to the audit's open question ("decide what C++ does"):

- **Bootstrap**: allocate/`register()` the meta-structure first, obtaining
  handle `H_meta`. Then set that Structure's own `StructureArenaCell
  ::structure_id = gc::StructureId(H_meta)` — self-referential, exactly
  mirroring C++'s two-phase "allocate, then point at self."
- **Every other Structure**: `StructureArenaCell::structure_id =
  gc::StructureId(H_meta)` unconditionally — every Structure's own header
  edge points at the one meta-structure handle, held wherever the port keeps
  `vm.structureStructure`'s analog (today nothing plays that role yet; this
  unit's `register()` needs a `meta_structure_handle: StructureHandle` field
  on `StructureIdTable`, set once at first bootstrap and reused for every
  subsequent `register()` call — the direct Rust shape of `vm.
  structureStructure`).

### 4.3 Leaf cells: do they get a real `structure_id`, or stay `INVALID`?

**C++ has no leaf-cell exception.** Every `JSCell` ctor requires a
`Structure*` (`JSCell(VM&, Structure*)`, `JSCellInlines.h:61`); `JSString`'s
ctor is no different (`runtime/JSString.h:157,163` both flow through the same
base). `vm.stringStructure`/`vm.symbolStructure` are real, ordinary Structures
that every `JSString`/`Symbol` cell's `m_structureID` names — there is no
"invalid structure" state for a live cell anywhere in C++.

The port's `CoreStringCell`/`CoreRopeStringCell` carry `structure_id:
StructureId` typed but explicitly documented as **INVALID** — "JSString uses
`vm.stringStructure`; the port does not model a string Structure... so this
is INVALID" (`string_store.rs:84-86`). **This is a real, standing divergence
from C++**, not something this unit invents or needs to fix to land Structure-
as-cell — `is_object()`/type dispatch in this port key off the *port's own*
`js_type` byte directly (`arena_cell_kind_at`), never off a decoded
`structure()`, so nothing currently reads a leaf cell's `structure_id` as
live data. Once Structure is a real, traceable cell (this unit), the
`INVALID` sentinel in leaf cells becomes slightly more visible (any future
code path that calls `.decode()`/looks up `structures[handle-1]` on a leaf's
`structure_id` would panic/index out of bounds on the `INVALID` sentinel), so
this design **flags but does not fix** wiring real `vm.stringStructure`/
`vm.symbolStructure`/`vm.bigIntStructure` analogs as a natural, small,
separately-scoped follow-up now that a real Structure arena exists to hold
them — see §8.

### 4.4 Where this plugs into `trace_cell`/`trace_leaf_cell`

Every existing `trace_*` body (`object_store.rs:2925` `trace_cell`,
`object_store.rs:3368` `trace_leaf_cell`) currently traces **zero** base-class
edges — `CoreObjectCell`'s own `structure_id` field is read by the interpreter
for property dispatch but never appended to the `SlotVisitor` worklist, and
the module doc for `trace_cell` doesn't mention it (grep of `trace_cell`'s
body shows it walks `inline_storage`/`butterfly`/aux-slab RuntimeValue edges
only). **Wiring `structure_id` as a traced edge on every cell is this unit's
second concrete Rust change beyond the Structure cell itself** — one line
added near the top of `trace_cell` and `trace_leaf_cell`:

```rust
// JSCell::visitChildrenImpl (JSCellInlines.h:130-134): every cell traces its
// own structure. New as of this unit — previously untraced (structures lived
// forever in a plain Vec, so no cell needed to protect its structure by
// marking). Route through the combined dispatcher (§6) since structure_id
// names an address in the STRUCTURE space, never the object space.
combined.append_unbarriered_by_addr(cell.structure_id.as_arena_addr());
```

This is *load-bearing* the moment Structure cells become collectable: without
it, an object whose Structure is otherwise unreferenced (no live parent/child
transition edge holding it) would have its shape reclaimed out from under it.
This is exactly the ordering hazard §6 phases around (structures must not be
collectable before every referencing cell traces this edge).

---

## 5. The two-tier weak transition table (within GC-U7)

### 5.1 The C++ conditional, evaluated for THIS collector

```cpp
if (!(visitor.heap()->collectionScope() == CollectionScope::Full))
    if (auto* transition = m_transitionTable.trySingleTransition())
        visitor.appendUnbarriered(transition);
```

(`Structure.cpp:1449-1453`.) This strongly marks the transition table's
single inline child **only during an eden (young-generation) collection** —
during a full collection it is left unmarked here, so it only survives if
reachable some other way; `StructureTransitionTable::finalizeUnconditionally`
(`runtime/StructureTransitionTable.h:512-518`,
`runtime/StructureInlines.h:608-616` in this checkout) then nulls the slot if
its target didn't survive: `if (auto* transition = trySingleTransition()) if
(!vm.heap.isMarked(transition)) m_data = UsingSingleSlotFlag;`.

**Verified against this port's collector, not assumed**: `CollectionScope`
does not appear anywhere in the live mark/sweep path
(`object_store.rs`/`slot_visitor.rs` — confirmed by grep, zero hits outside
the unrelated, unwired `gc/phase.rs` scaffold). The live collector
(`force_collect`, `object_store.rs:4052-4079`) is unconditionally a full STW
collection; there is no eden concept. Evaluating C++'s guard under "scope is
always Full" makes `!(scope == Full)` **unconditionally false** — so the
correct, *faithful* reduction for this port is: **`trace_structure` never
executes the strong-mark arm at all.** This is not an approximation or a
simplification adopted for convenience; it is what the C++ conditional
itself evaluates to once the port's actual `collectionScope()` is substituted
in. The audit's "conservative always-weak single-slot divergence" is exactly
this, now confirmed rather than assumed, and the design comment at the code
site should say so explicitly (cite `Structure.cpp:1449` + "this collector's
`collectionScope()` is always `Full`, so this arm is dead by evaluation, not
by choice").

The `WeakGCMap`-promoted (multi-transition) case
(`StructureTransitionTable::isUsingSingleSlot() == false`,
`runtime/StructureTransitionTable.h:264-267`) is already, in C++, a
`WeakGCMap` — every entry is weak regardless of collection scope. The port's
`TransitionData::Map(HashMap<Key, TransitionStructure>)` arm
(`structure_transition_table.rs:332-341` `TransitionData` enum) is therefore
*already* the right shape to be "always weak" — no change needed there beyond
the finalize wiring below.

### 5.2 Plugging into the landed GC-U7 finalize seam

`CoreObjectStore::finalize_unconditional_finalizers`
(`object_store.rs:4105-4193`) is the ported `Heap::finalizeUnconditionalFinalizers`
seam, run at the exact C++ position — after `endMarking`, before sweep
(`heap/Heap.cpp:1705,1750,1754`, cited verbatim in the Rust doc comment,
`object_store.rs:4081-4104`) — and it is **explicitly documented as shared**:
"THE SEAM IS SHARED (GC-U7.0, ratified): future end-of-cycle weak processing
plugs into THIS step... `CodeBlock::finalizeUnconditionally`'s IC
weak-structure reset" (`object_store.rs:4098-4104`). Structure's transition
table finalize is precisely another such consumer:

```rust
// StructureTransitionTable::finalizeUnconditionally (StructureTransitionTable.h
// :512-518 / structure_transition_table.rs, new method): for every MARKED
// Structure with a materialized single-slot transition, drop the slot if its
// target is unmarked. The multi-transition Map arm needs the identical
// `retain`-by-liveness shape the WeakMap/WeakSet arms already use
// (object_store.rs:4150 `entries.retain(|&(key, _)| weak_collection_key_is_marked(..))`).
fn finalize_structure_transitions(&mut self) {
    for (h, addr) in self.arena_addrs.iter().enumerate() {
        if !self.space.is_addr_marked(*addr) { continue; } // dead Structure: nothing to finalize
        let structure = &mut self.structures[h];
        structure.transition_table.finalize_unconditionally(|target_handle| {
            self.arena_addrs.get(target_handle.raw() as usize - 1)
                .is_some_and(|&a| self.space.is_addr_marked(a))
        });
    }
}
```

This is a new method on `StructureTransitionTable`
(`object/structure_transition_table.rs`), called from a new step in whatever
orchestrates the cross-store collection (§6) at the same position
`finalize_unconditional_finalizers` runs today. It is **not** a change to
`finalize_unconditional_finalizers` itself — that method is scoped to
`CoreObjectStore::space` and stays that way; Structure's finalize is a sibling
call at the same *phase*, not an addition to that function's *body*.

---

## 6. Migration order + compatibility

Each step lands independently, is individually gated, and — critically —
**no step before the last changes observable behavior**, so each is
low-risk and revertible on its own.

### Step 1 — Cell + arena plumbing, registry-rooted (no collectability yet)

- Add `StructureArenaCell` (§1.1), `JsType::Structure` (§1.3).
- `StructureIdTable` grows `arena_addrs: Vec<usize>` + `space: MarkedSpace`
  (§2.1); `register()` allocates the shadow cell.
- **No trace wiring, no finalize wiring.** The Structure space is never swept
  (or: is "swept" but its collector never runs / its cells are additionally,
  unconditionally strong-rooted by iterating `arena_addrs` as extra roots on
  every `force_collect` — either framing is behavior-equivalent to "never
  collected," matching how R3/R4a's own POD cutover started with the arena
  as an *additional* home before flipping identity, `gc-r4.md`).
- **Compatibility**: bit-for-bit identical program behavior. `StructureId`
  values, `structure()` lookups, transition/materialize logic — nothing
  observable changes; only bookkeeping grows.
- **Oracle**: `cargo test --lib` (existing `structure_cell.rs` unit tests
  unchanged) + a new test asserting `arena_addrs.len() == structures.len()`
  after a sequence of `add_property_transition` calls, and that every
  `arena_addrs[i]` is `space.is_arena_cell(..)`-admitted.
  Miri run on the new `allocate_blob` call site (mirrors the existing R1 S4
  miri gate).
- **Rollback**: delete the two new fields and the `allocate_blob` call; zero
  blast radius (nothing reads them yet).

### Step 2 — Base-class edge (§4): every cell traces its `structure_id`

- Wire the `structure_id` edge into `trace_cell`/`trace_leaf_cell` (§4.4).
- Requires the **combined dispatcher** (§2.3/§3.5): a `VisitChildren`
  implementor that tries `object_store.space.is_arena_cell` then `structure_
  table.space.is_arena_cell` before dereferencing an edge target — this is a
  genuine, new, *serial* piece (crosses the `CoreObjectStore`/`StructureIdTable`
  boundary) that must be authored once, here, not improvised per-caller.
  Concretely: a new `CombinedGraphMarker<'a> { objects: &'a CoreObjectStore,
  structures: &'a StructureIdTable }` implementing `VisitChildren`, structured
  as `ObjectGraphMarker` (`object_store.rs:3193-3241`) already is, widened to
  two membership probes.
- Because Step 1 kept Structures unconditionally alive (registry-rooted),
  this step is safe to land *without* Step 3/4: tracing an edge to an
  always-live cell changes nothing observable — it only proves the wiring
  before anything depends on it for correctness.
- **Oracle**: a targeted test allocating an object + a property transition,
  running `mark_live_set_from_addrs` (or the future combined equivalent) from
  the object alone, and asserting the Structure's arena address ends up
  marked — i.e., the edge fires. Full `cargo test --lib`.
- **Rollback**: revert the one-line `append_unbarriered` addition in each
  `trace_*`; the combined dispatcher can stay dormant (unused) with no effect.

### Step 3 — `trace_structure` instance edges (§3): prototype + previous

- Add the `trace_structure` body (§3.2, §3.3), still registry-rooted (Step 1
  ordering unchanged) — so this step is *also* observation-neutral: it adds
  edges from an always-live Structure to other always-live Structures/
  objects, which changes nothing about what survives a collection.
- This is the step that also lands the PropertyTable conditional clear
  (§3.4) — **flagged separately for its own probe** because, unlike the
  pure-edge-tracing parts of this step, it *does* change behavior the moment
  any collection actually runs (a materialized table gets dropped and
  rebuilt lazily). Land it gated behind the same "collections don't run yet
  for real reclaim" umbrella as Step 1, or split into 3a (edges only) / 3b
  (the clear), whichever the implementer finds easier to gate independently.
- **Oracle**: `materialize_replays`-style test (already exists,
  `structure_cell.rs` tests) re-run *after* forcing a mark cycle with the
  new clear wired, asserting the materialized result is unchanged (replay
  is provably idempotent under the existing tests — this just proves the
  clear-then-replay round-trips). A perf probe (not required to land, but
  recommended before broad exposure) measuring rebuild frequency on a
  property-transition-heavy micro-benchmark, since this is the one step with
  a real, C++-shared performance shape.

### Step 4 — Weak transitions flip collectability on (§5)

- Wire `finalize_structure_transitions` (§5.2) into the same cross-store
  collection orchestration Step 2 introduced.
- **Remove the Step 1 unconditional strong-rooting of `arena_addrs`.** This
  is the flag-day moment: from here on, a Structure with no surviving strong
  edge (no live object's `structure_id`, no live parent's `previous`/
  transition edge) is real garbage.
- **Do not enable handle reuse yet** (free-listing dead `StructureId` slots
  for a new `register()`) — see the open question in §8 (the `bytecode/ic.rs`
  raw-`StructureId`-cache hazard). Land Step 4 as: dead Structures' arena
  *cells* are reclaimed (atoms freed by the normal sweep, `record` dropped
  from `structures[h]` via `mem::take`-to-a-tombstone), but **the handle
  itself is never reissued** — `structures`/`arena_addrs` may carry
  tombstones, bounded by however many Structures die, which is strictly safe
  (no ABA) at the cost of some unreclaimed Vec capacity. This mirrors the
  existing, precedented pattern (`slab_alloc`/free-list reuse,
  `object_store.rs:68-76`) *minus* the reuse half, specifically because reuse
  is not yet safe here (see §8) — reuse is a **separate, explicitly gated**
  follow-up once the IC hazard is resolved.
- **Oracle**: a test that builds a transition chain, drops every strong
  reference to a child (no live object uses it, no surviving sibling
  transition target references it), forces a collection, and asserts (a) the
  dead Structure's slot is tombstoned/its arena cell atom is freed, (b) the
  transition table entry on the surviving parent is gone
  (`try_single_transition()` returns `None` / the `Map` entry is absent),
  and (c) every *other* live Structure (including ones sharing the same
  transition-sibling convergence, `sibling_transitions_converge` test) is
  unaffected. Full `cargo test --lib` + miri on the sweep path (mirrors the
  R4b-sweep miri gate already run for the object arena).
- **Rollback**: re-enable Step 1's unconditional rooting (one-line revert);
  everything built in Steps 2/3 keeps working (it was already correct under
  permanent liveness, and stays correct under real liveness — the trace
  bodies don't change, only what calls them for real).

### Why this order and not another

Steps 1→2→3→4 is a strictly increasing-risk ladder where every step before
the last is behavior-neutral by construction (an always-live population
makes edge-tracing and clearing pure no-ops for correctness, only proving
wiring), and the one step that *is* behaviorally load-bearing (Step 4) is
last and smallest (toggle rooting + wire one finalize call) precisely because
everything it depends on (cell layout, address translation, edge walking,
finalize seam) was independently proven first. This mirrors the R4a/R4b
object-cell cutover's own shape (arena-as-additional-home → real identity
flip → mark → finalize → reconcile → sweep, `docs/design/gc-r4.md`), reusing
a sequencing pattern this codebase has already executed successfully once.

---

## 7. Fork retirement plan (R3)

### 7.1 What `object/structure.rs` actually is

Reading it end to end (`object/structure.rs:1-181` for the cell-shaped part):
it is a **complete, self-consistent, but entirely disconnected** second
design for an arena-ready Structure, built against a *different* layer of
scaffolding than the one the live R4 arena uses:

- `header: JsCellHeader` (`object/structure.rs:56`) is `crate::gc::cell::
  JsCellHeader` (`gc/cell.rs:567-595`) — **not** `marked_block::JsCellHeader`
  (`marked_block.rs:145-154`) the live arena actually uses. The two are
  incompatible: `gc::cell::JsCellHeader` is `{structure_id: StructureId,
  cell_type: CellType (repr(u16)), state: CellState, flags:
  CellHeaderFlags(u32)}` — no fixed 8-byte C++ offset layout at all, whereas
  `marked_block::JsCellHeader` is the byte-exact `{u32, u8, u8, u8, u8}` the
  real arena and `SlotVisitor` depend on (`marked_block.rs:20-27`).
- `Trace for Structure` / `TraceCell` (`object/structure.rs:169-181`) are
  `crate::gc::trace::{Trace, Tracer, TraceCell}` (`gc/trace.rs:11-24,640-642`)
  — a `dyn Tracer` visitor-object design, not the live `SlotVisitor`/
  `VisitChildren`/`CellEdgeVisitor` concrete-type dispatch
  (`gc/heap/slot_visitor.rs:89-93`, `object_store.rs:3252-3273`) the real
  collector uses.
- `WriteBarrier<JsCell>` (`object/structure.rs:57`) is `gc::barrier::
  WriteBarrier` — confirmed by grep to have **zero** other callers anywhere
  in the crate outside `gc/barrier.rs` itself and this one file.
- Its `PropertyTable`/`WatchpointSet`/`StructureTransitionMetadata`/
  `StructureDictionaryKind`/`StructurePrototypeStorage` types
  (`object/structure.rs:8-11`, imported from `object::property`/`object::
  watchpoint`) are a **separate type family** from `structure_cell.rs`'s
  `PropertyTable`/`StructureTransitionTable` (`object/property_table.rs`,
  `object/structure_transition_table.rs`) — different modules, different
  APIs, not interchangeable.

In short: `gc::cell`/`gc::trace`/`gc::barrier`/`object::structure` are a
mutually-consistent, pre-S4 design generation (header/visitor/barrier
abstractions authored before the arena pivoted to the raw-address/
`UnsafeCell` model, `marked_block.rs`'s C1-C6 contract). They were never
wired to `marked_block.rs`/`slot_visitor.rs`/`object_store.rs`. This *is* the
fork R3 names, and "genuinely useful" harvesting from it means harvesting
**requirements it got right conceptually**, not literal code, since the
literal code targets an abandoned header/visitor shape.

### 7.2 What is harvested vs. deleted

**Harvested (concept, re-expressed against the live types):**

- *"A cell has a fixed header prefix, and the rest is the payload"* — already
  the live convention (§1.1); `object/structure.rs` independently arrived at
  the same shape, which is corroborating evidence the shape is right, not a
  source of new code.
- *"The prototype must be a real traced write-barrier edge, not a bare
  pointer"* (`object/structure.rs:139-143,171-173`) — directly informs §3.3;
  the live `structure_cell::Structure::prototype: PrototypePointer` field
  keeps its name and representation (a `usize`, `0 == null`, already exactly
  what `StructureTransitionTable`'s `PointerKey::from_object` keys on,
  `structure_cell.rs:236-238`) — only its *treatment during trace* changes
  from "no-op" to "conditionally traced," per §3.3. No type from
  `object/structure.rs` is reused; only the requirement.
- *`IndexingMode` enum* (`object/structure.rs:16-30`) — this is genuinely
  reusable **content** (a faithful list of JSC indexing-mode variants with
  helper predicates `has_indexed_properties`/`is_copy_on_write`/
  `needs_slow_put`), but it already has a live twin:
  `object::indexing_type::IndexingType` is explicitly documented as the
  live replacement — `object/indexing_type.rs:235`: "`object::structure
  ::IndexingMode` (structure.rs:16-30) is the pre-arena [predecessor]." So
  this is **already harvested** (the live type exists and is used by
  `structure_cell.rs`'s `TypeInfoBlob`); `object/structure.rs`'s copy is
  redundant, not a source to pull from.

**Deleted, not harvested (evidence both models were compared, per R3):**

- `Structure` struct itself (`object/structure.rs:55-181`) — superseded by
  `structure_cell::Structure`, which is more complete (real transition table
  with sibling convergence, real property-table materialize/steal
  semantics, real `StructureIdTable` registry) and is the one this whole
  document builds on. `object/structure.rs`'s version has no transition
  table at all beyond a `describe_transition`/`StructureTransitionPlan`
  descriptor pair (`object/structure.rs:156-162`) — it never became a
  working shape-transition graph.
- `gc::cell::{JsCellHeader, CellType, TypeInfo, JsCell, TraceCell, ...}` and
  `gc::trace::{Trace, Tracer, MarkingPlanGraph, ...}` and `gc::barrier::
  WriteBarrier` **as consumed by `object/structure.rs`** — these stay in the
  tree (deleting the whole `gc/cell.rs`/`gc/trace.rs`/`gc/barrier.rs` module
  is out of scope for this unit and has consumers this design does not
  audit, e.g. `object/identity.rs`'s own use of `gc::cell::` symbols per the
  earlier grep), but `object/structure.rs`'s *specific instantiation* of
  them is deleted with the file.
- `StructureDictionaryKind`/`StructurePrototypeStorage`/
  `StructureTransitionMetadata`/`WatchpointSet` (as used by `object/
  structure.rs`) — deleted with it. `structure_cell.rs` already has its own,
  independently-evolved handling of dictionaries (`create_dictionary_from`,
  `structure_cell.rs:895-919`) and does not need these.

### 7.3 Fixing the one real external consumer

`vm/runtime.rs:7,48-73` holds `object_structure`/`function_structure`/
`global_object_structure: Option<Root<Structure>>` fields (`Root<T>` from
`gc/refs.rs:155`, another pre-arena rooting abstraction). Grepping for these
three field names outside `vm/runtime.rs` finds **zero** readers/writers —
they are dead accessors with no caller. The **live** equivalents already
exist and are wired: `runtime/state.rs:94`'s `function_structure:
Option<StructureId>` and `runtime/realm.rs:155-157`'s `object_structure/
function_structure/host_function_structure: Option<StructureId>`, both using
`crate::gc::StructureId` (the real, load-bearing handle type, §2.1) — these
are unaffected by this retirement; they are what the interpreter actually
uses today and already compose correctly with everything in this design (a
`StructureId` handle is exactly what `StructureIdTable::structure()` expects).
Retiring `object/structure.rs` therefore means: delete the three dead fields
+ their six dead accessor methods from `vm/runtime.rs`, delete the `mod
structure;`/`pub use structure::{...}` block in `object/mod.rs`
(`object/mod.rs:22,78-85`), and let `cargo check --lib` catch anything this
survey missed (a fast, cheap, complete verification given the small,
self-contained blast radius established above).

### 7.4 `Clone`, one more time — a precedent already set

`StructureIdTable`/`Structure` currently derive `Clone`
(`structure_cell.rs:283,587`), used by "the interpreter's `CoreObjectStore`
snapshot/test path" (module doc, `structure_cell.rs:278-282`). Once
`StructureIdTable` owns a `MarkedSpace` (§2.1), it can no longer derive
`Clone` — a `MarkedSpace` is `!Clone` by construction (one set of exposed raw
pages, `object_store.rs:485`). **This is not a new problem being introduced
here; it is the identical problem the object arena already solved once**:
"`impl Clone for CoreObjectStore` is DELETED. It was test-only... and
re-pinned every cell to a NEW `Box` address — fundamentally incompatible with
arena-ADDRESS identity" (`object_store.rs:481-487`, "gc-r4 R4a decision C").
This design's Step 1 (§6) should apply the identical fix at the identical
moment: drop `#[derive(Clone)]` from `StructureIdTable`, find and rewrite/
delete its `#[cfg(test)]` consumers (mirroring "the 3 `#[cfg(test)]` clone
tests were rewritten/removed," `object_store.rs:486`), citing R4a decision C
directly as precedent rather than re-deriving the reasoning.

---

## 8. Open questions (could not be settled from source)

1. **StructureId handle reuse safety.** `bytecode/ic.rs` holds bare
   `StructureId` values (`base_structure`/`holder_structure`/`new_structure`,
   confirmed by grep across `StructureStubInfo`-shaped structs) that are
   **not** GC roots and are **not** currently invalidated when a Structure
   dies (`reset_by_gc` is documented "INERT today, deliberately NOT wired,"
   `object_store.rs:4102-4104`). This design (§6 Step 4) deliberately avoids
   handle reuse to sidestep this, but does not resolve it — a future unit
   that (a) wires `reset_by_gc` to actually invalidate IC records referencing
   a dead Structure, or (b) roots IC-cached `StructureId`s, is needed before
   handle reuse can be enabled safely. **Evidence needed**: a full audit of
   every long-lived `StructureId`/`Option<StructureId>` field in the crate
   (IC records, watchpoint records — `CoreStructureTransitionWatchpointRecord`
   at `object_store.rs:489-492` is another one — and any DFG/profiling
   structure-shape cache) classified as either GC-rooted-transitively or not,
   to bound exactly how large the "not yet safe to reuse" surface is.
2. **Cross-space membership cost at scale.** §2.3 accepts "2 probes" as
   bounded for 2 spaces. If a third dedicated space appears later (e.g.
   `PropertyTable`-as-cell, §3.4's flagged follow-up, or per-kind spaces for
   Executable/CodeBlock), the linear membership probe chain grows. **Evidence
   needed**: whether JSC's actual `HeapUtil::isPointerGCObjectJSCell`
   (`heap/HeapUtil.h:54-79`) membership check, which spans a single Heap-wide
   structure, is cheap enough in this port's address-range-based
   `MarkedBlockSet`/`precise_set` model to just union across N stores'
   `MarkedSpace`s into one shared membership index — measuring this needs
   the second real consumer (this unit) to exist first; premature to build
   before then.
3. **`m_realm`/`m_cachedPrototypeChain`.** Explicitly out of scope (§3.2) since
   the fields don't exist on the port's `Structure` yet. **Evidence needed**:
   when `JSGlobalObject`/`StructureChain` cells are ported, whether they
   land in the object arena or need their own space too (JSC's
   `JSGlobalObject` is an ordinary `JSObject` subtype, so almost certainly
   the object arena) — determines whether `trace_structure`'s realm edge is
   a same-space or cross-space append.
4. **Concurrent/`protectPropertyTableWhileTransitioning`/`isAnalyzingHeap`.**
   Confirmed out of scope because neither concurrent compilation nor the
   heap-snapshot feature exists in this port (§3.4) — not re-verified beyond
   "grep finds nothing," which is treated as sufficient given both are
   whole, currently-absent subsystems, not narrow gaps.
5. **Should `PropertyTable` become its own cell in a follow-up, given the
   mcts_mem-documented 14 MB measured win came specifically from that (not
   merely from clearing the reference)?** §3.4 argues the port gets the same
   *effect* (drop the Vecs) without a separate cell, since ownership is
   inline. **Evidence needed**: whether JSC's 14 MB win was from freeing the
   `PropertyTable` object's *own* allocation header/GC bookkeeping overhead
   (which a separate cell has and an inline `Option<PropertyTable>` does
   not need at all) — if so, the port's inline-clear may already
   *exceed* C++'s savings per table, making a separate cell purely a
   fidelity exercise, not a performance one. Not verifiable without a
   memory-footprint measurement this design doc cannot run.
6. **Exact threshold/geometry for the Structure space's size classes if a
   future variant (branded/dictionary-heavy) needs a second size class.**
   This design specifies exactly one fixed 16-byte cell (§1.1) because the
   only variant currently modeled is `TransitionKind`-differentiated, not
   layout-differentiated (`StructureVariant::Branded`/`WebAssemblyGC` are not
   modeled, `structure_cell.rs:369-372` doc). **Evidence needed**: none yet —
   flagged only so a future BrandedStructure port doesn't assume the space
   is single-size-class by accident.
