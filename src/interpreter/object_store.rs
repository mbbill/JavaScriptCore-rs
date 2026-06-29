//! `CoreObjectStore` — the live JSObject/JSArray/JSFunction cell store, its cells,
//! property/transition/IC observation types, and the StructureID allocator.
//!
//! Phase E B4: extracted verbatim from `interpreter/mod.rs` by pure code-motion
//! (no body changed; only module placement and visibility keywords). This is the
//! largest runtime-class store and the Structure-wiring + R3 cutover target.
//! Faithful TARGET on the C++ side: Source/JavaScriptCore/runtime/JSObject.* +
//! JSArray.* + JSFunction.* + object/Structure (StructureID/transition table).
//!
//! Object-internal free helpers (megamorphic-property validators, typed-array
//! kind tables, PropertyOffset math, generated-property-load probes) move WITH the
//! store and stay private — they have no callers outside this module.

use super::*;

// gc-r4 GAP A: the live-path marking sink consumes `RuntimeValue`'s cell
// projection (`value/repr.rs CellValue`), the SD-1 / GAP-D value type — NOT the
// skeleton `gc::Tracer`/`GcRef<JsCell>` path (the wrong value type). `CellValue`
// is not in the `interpreter` glob (`use super::*`), so it is imported here.
use crate::value::CellValue;

pub(crate) fn can_use_get_by_id_megamorphic_property_name(text: &str) -> bool {
    !matches!(text, "length" | "name" | "prototype" | "__proto__")
        && parse_array_index_name(text).is_none()
}

#[allow(dead_code)]
pub(crate) fn can_use_in_by_id_megamorphic_property_name(text: &str) -> bool {
    can_use_get_by_id_megamorphic_property_name(text)
}

pub(crate) fn can_use_put_by_id_megamorphic_property_name(text: &str) -> bool {
    text != "__proto__" && parse_array_index_name(text).is_none()
}

#[derive(Debug, Default)]
pub(crate) struct CoreObjectStore {
    pub(crate) objects: Vec<Pin<Box<CoreObjectCell>>>,
    // gc-r4 B1a: the store-owned slab of live butterfly allocations, the home of
    // each object's out-of-line property+element region over `RuntimeValue`
    // (object/butterfly_handle.rs `ButterflyAllocation`; C++ `Butterfly`,
    // Butterfly.h:134-150). A `ButterflyHandle` is an index into this Vec. C++
    // allocates each butterfly from the GC Auxiliary subspace
    // (Heap::tryAllocateButterfly); the store-owned slab is the pre-R4 analog
    // (the raw arena butterfly pointer arrives at R4). ADDITIVE this batch: the
    // slab + API exist but no cell field or call site uses them yet (the cutover
    // wires `storage_ptr`/`out_of_line_storage`/`elements` onto this slab and
    // deletes the per-cell `properties` HashMap in a later batch).
    #[allow(dead_code)]
    pub(crate) butterflies: Vec<ButterflyAllocation>,
    // gc-r4 POD-ification (BoundFunction unit): the store-owned slab of bound-function
    // [[BoundArguments]] value arrays. C++ JSC JSBoundFunction::m_boundArgs is an
    // out-of-line value array (runtime/JSBoundFunction.h:133); the per-cell
    // `bound_args: Vec<RuntimeValue>` field is relocated here so the cell carries only a
    // POD `AuxiliaryHandle` (an index into this Vec) instead of a Drop-bearing Vec —
    // exactly mirroring the `butterflies` slab + `ButterflyHandle` pattern, but allocated
    // ONLY by `allocate_bound_function` (not every cell, unlike the butterfly). C++
    // allocates m_boundArgs from the GC Auxiliary subspace; this slab is the pre-R4
    // analog (a raw Auxiliary pointer arrives at R4). Each inner array still holds
    // `RuntimeValue` GC edges — a later collector trace MUST visit this backing
    // (gc-r4 GAP A); no trace wiring lands in this unit.
    pub(crate) bound_args_backings: Vec<Vec<RuntimeValue>>,
    // gc-r4 R4 POD-ification (JSFunction-captures unit): the store-owned slab of
    // closure captured-variable value arrays. A JSFunction's captured variables are the
    // closure's free-variable values (faithfully a JSLexicalEnvironment reached via the
    // scope chain, JSLexicalEnvironment.h:56-80 / JSCallee::m_scope). gc-r4 SD-2 accepts
    // the aux-value-slab POD EXPEDIENT now (the faithful scope-chain relocation is a
    // DEFERRED correctness batch): the per-cell `captures: Vec<RuntimeValue>` is relocated
    // here so the cell carries only a POD `AuxiliaryHandle` (an index into this Vec)
    // instead of a Drop-bearing Vec — exactly mirroring the `bound_args_backings` slab.
    // Allocated for EVERY function at `allocate_function_with_construct_ability` (even an
    // empty capture set, like `allocate_bound_args`), so a Function cell's handle is always
    // real. The arrays are immutable after creation (closure-variable WRITES mutate the
    // separate closure CELL the value points at, not this Vec), so this is write-once like
    // bound_args. Each array holds `RuntimeValue` GC edges — a later collector trace MUST
    // visit this backing (gc-r4 GAP A); no trace wiring lands in this unit.
    pub(crate) captures_backings: Vec<Vec<RuntimeValue>>,
    // gc-r4 R4 POD-ification (JSFunction-captures unit): the store-owned slab of a class
    // constructor's instance-field-initializer records (the `[[Fields]]` a `class { x = e }`
    // installs on each instance). gc-r4 SD-2 accepts the aux-slab POD expedient now (the
    // faithful class-field init is a DEFERRED correctness batch). The per-cell
    // `instance_fields: Vec<CoreInstanceField>` is relocated here; crucially `CoreInstanceField`
    // carries a `CorePropertyKey` whose `String` variant is Drop-bearing, so the slab stores
    // the POD `CoreInstanceFieldRecord` instead — the key is interned to a `Copy` `AtomId`
    // uid via `intern_property_uid` (the SAME `UniquedStringImpl*`-identity uniquing C++ keys
    // PropertyTable on) and recovered through `property_keys_by_uid` on read, so NO Rust
    // `String` lives on the cell path. Lazily allocated on the first `add_instance_field`
    // (most cells never call it) like `promise_reaction_lists`; the cell holds a POD
    // `AuxiliaryHandle` (`INVALID` until first field). The records hold `RuntimeValue`
    // initializer GC edges — a later collector trace MUST visit this backing (gc-r4 GAP A).
    pub(crate) instance_field_lists: Vec<Vec<CoreInstanceFieldRecord>>,
    // gc-r4 R4 POD-ification (Promise unit): the store-owned slab of pending
    // promise reaction-record lists, the home of each pending promise's
    // out-of-line reaction records (C++ JSPromise `[[PromiseFulfillReactions]]`/
    // `[[PromiseRejectReactions]]`, JSPromise.h:35). A `PromiseReactionsHandle` is an
    // index into this Vec; the slot is lazily allocated on first enqueue
    // (`push_promise_reaction`) and drained on settle (`take_promise_reactions`).
    // Relocated OUT of the per-cell `promise_reactions: Vec<..>` so `CoreObjectCell`
    // sheds another `Drop` field (the records are `Copy`, but the `Vec` was not).
    // C++ allocates these records from the GC heap; the store-owned slab is the
    // pre-R4 analog (a later collector trace visits the slab to mark the `Copy`
    // GC-edge bundles it holds).
    pub(crate) promise_reaction_lists: Vec<Vec<CorePromiseReaction>>,
    // gc-r4 R4 POD-ification (RegExp unit): the store-owned slab of RegExp pattern
    // strings. C++ JSC `RegExp::m_patternString` (runtime/RegExp.h:219) is an
    // out-of-line `String` (a ref-counted `StringImpl*`) hanging off the RegExp
    // cell; relocating it OUT of `CoreObjectCell` into this store-owned slab keyed
    // by a Copy `AuxiliaryHandle` (object/auxiliary.rs) makes the cell's RegExp
    // source field POD (no `Drop`), so the cell stays sweep-eligible. An
    // `AuxiliaryHandle` is an index into this Vec; only RegExp cells hold a real
    // one (every other cell carries `AuxiliaryHandle::INVALID`). Write-once at
    // `allocate_regexp` (a RegExp's pattern is immutable after creation, exactly as
    // `m_patternString` is). NOT the R4 leak fix — like `butterflies`, this slab
    // still needs its own Auxiliary-subspace trace+sweep at R4 (gc-r4 SD-4).
    pub(crate) regexp_sources: Vec<String>,
    // gc-r4 R4 POD-ification (ArrayBuffer unit): the store-owned slab of
    // ArrayBuffer/typed-array byte backings. C++ JSC `ArrayBufferContents::m_data`
    // (runtime/ArrayBuffer.h:126) is a raw `void*` byte buffer of `sizeInBytes`
    // hanging off the ArrayBuffer; relocating the per-cell `array_buffer_data:
    // Vec<u8>` into this store-owned slab keyed by a Copy `AuxiliaryHandle`
    // (object/auxiliary.rs) makes the cell's backing field POD (no `Drop`), so the
    // cell stays sweep-eligible. An `AuxiliaryHandle` is an index into this Vec;
    // only ArrayBuffer cells hold a real one (every other cell carries
    // `AuxiliaryHandle::INVALID`). Allocated at `allocate_array_buffer`; the bytes
    // are mutated in place by typed-array/DataView stores. UNLIKE the bound_args /
    // promise-reaction / butterfly slabs, these are raw bytes, NOT `RuntimeValue`
    // GC edges — so NO write barrier on store, and the R4 collector trace need NOT
    // visit this slab (it still needs its own Auxiliary-subspace sweep, like
    // `regexp_sources`).
    pub(crate) array_buffer_backings: Vec<Vec<u8>>,
    // gc-r4 R4 POD-ification (Map/Set unit): the store-owned slabs of Map/WeakMap
    // insertion-ordered (key,value) entries and Set/WeakSet insertion-ordered values.
    // C++ JSC keeps these in a `JSOrderedHashTable::Storage` (a `JSCellButterfly`
    // held by `m_storage`, JSOrderedHashTable.h:164) hanging off the collection cell;
    // relocating the per-cell `map_entries: Vec<..>` / `set_values: Vec<..>` into these
    // store-owned slabs keyed by a Copy `AuxiliaryHandle` makes the cell's collection
    // field POD (no `Drop`), so the cell stays sweep-eligible. An `AuxiliaryHandle` is
    // an index into the matching Vec; only Map/WeakMap cells hold a `map_entries`
    // handle and only Set/WeakSet cells hold a `set_values` handle (every other cell
    // carries `AuxiliaryHandle::INVALID`). Allocated eagerly at the collection's
    // `allocate_*` site (an empty backing, like every JSObject gets a butterfly), not
    // lazily — the handle is valid for the cell's whole life.
    //
    // DIVERGENCE (POD expedient, gc-r4 rank-5): these are PLAIN insertion-ordered Vecs,
    // NOT the faithful `JSOrderedHashTable` (which gives O(1) keyed lookup via a hash
    // index over a JSCellButterfly-backed ordered table). The semantics preserved here
    // are EXACTLY the prior per-cell Vec behavior (insertion order, linear SameValueZero
    // / strict-equality keyed lookup, has/get/set/delete/forEach/size/clear); only the
    // storage moved off the cell. The faithful ordered-hash port is a DEFERRED
    // correctness/perf batch (Map/Set is not Octane-hot).
    //
    // Each entry/value still holds `RuntimeValue` GC edges (Map keys+values, Set
    // values) — a later collector trace MUST visit BOTH slabs (gc-r4 GAP A); no trace
    // wiring lands in this unit.
    pub(crate) map_entry_lists: Vec<Vec<(RuntimeValue, RuntimeValue)>>,
    pub(crate) set_value_lists: Vec<Vec<RuntimeValue>>,
    // VM-internal payload-bits -> object-slot index; keyed by interpreter pointer-bits,
    // never JS/adversary-controlled, so it needs no SipHash DoS resistance. Use the
    // in-tree FxIntBuildHasher (gc/fast_hash.rs, WTF IntHash/PtrHash family); the swap is
    // semantically inert (get/insert/contains/clear/len are BuildHasher-independent).
    pub(crate) object_indices_by_payload: HashMap<usize, usize, FxIntBuildHasher>,
    // C++ JSC: the per-VM Structure registry (`VM::structureIDTable`, in C++ implicit
    // in the Structure heap address). gc-r4 Batch 2 mounts the ported faithful
    // `StructureIdTable` (object/structure_cell.rs) as the SINGLE structure-id AND
    // property-offset authority, replacing the former per-cell `property_offsets`
    // HashMap + `next_property_offset` allocator (the load-bearing divergence: C++
    // keeps the property->offset map in Structure::PropertyTable, per-SHAPE, never
    // per-object). A cell's `structure_id` IS a `StructureIdTable` handle; the offset
    // of a named property is read from that structure's PropertyTable (owned, or
    // materialized-on-miss by replaying the transition chain, Structure.cpp:456).
    // addPropertyTransition (Structure.cpp:561) lives inside the table and makes two
    // same-shape objects converge on ONE successor structure (and ONE offset).
    pub(crate) structure_table: StructureIdTable,
    // CorePropertyKey -> uniqued uid adapter. C++ keys Structure::PropertyTable and
    // m_transitionTable by the property name's `UniquedStringImpl*` identity; the
    // interpreter's CorePropertyKey already encodes identity, so this interns each
    // distinct named-offset key to a stable `AtomId` table slot (the uid the ported
    // PropertyTable/transition table key on). Injective over the named-offset key set
    // (Identifier + non-index String), so the structure graph keys by JSC identity.
    pub(crate) property_uids: HashMap<CorePropertyKey, AtomId>,
    // Reverse of `property_uids`: the uniqued uid -> the `CorePropertyKey` it interns.
    // C++ keys a Structure::PropertyTable entry by `UniquedStringImpl*`, and recovering
    // the name from an entry is just dereferencing that pointer; the Rust port keeps an
    // explicit reverse map so enumeration over a structure's PropertyTable entries
    // (`structure_property_keys`, the post-flip replacement for the per-cell
    // `property_order`) can map each entry's uid back to its key. Injective with
    // `property_uids` (both updated in lockstep by `intern_property_uid`).
    pub(crate) property_keys_by_uid: HashMap<AtomId, CorePropertyKey>,
    // Monotonic allocator for fresh property uids (slot 0 reserved == AtomId::UNASSIGNED).
    pub(crate) next_property_uid: u32,
    // Per-(kind, prototype) empty-shape ROOT structure handle, the analog of the empty
    // Structure JSGlobalObject hands every fresh object of a class+prototype so sibling
    // objects begin from ONE shared root id and their first add-property transition
    // converges. Values are `structure_table` root handles (create_root).
    pub(crate) structure_seed_roots: HashMap<(CoreObjectKind, CorePrototypeIdentity), StructureId>,
    structure_transition_watchpoints:
        HashMap<WatchpointSetId, CoreStructureTransitionWatchpointRecord>,
    pub(crate) structure_transition_watchpoints_by_structure:
        HashMap<StructureId, Vec<WatchpointSetId>>,
    pub(crate) fired_watchpoint_events: Vec<WatchpointFireEvent>,
    pub(crate) structure_chain_invalidation_events: Vec<StructureChainInvalidationEvent>,
    pub(crate) object_prototype: Option<RuntimeValue>,
    pub(crate) function_prototype: Option<RuntimeValue>,
    pub(crate) array_prototype: Option<RuntimeValue>,
    pub(crate) string_prototype: Option<RuntimeValue>,
    pub(crate) number_prototype: Option<RuntimeValue>,
    pub(crate) boolean_prototype: Option<RuntimeValue>,
    pub(crate) error_prototype: Option<RuntimeValue>,
    pub(crate) type_error_prototype: Option<RuntimeValue>,
    pub(crate) reference_error_prototype: Option<RuntimeValue>,
    pub(crate) range_error_prototype: Option<RuntimeValue>,
    pub(crate) map_prototype: Option<RuntimeValue>,
    pub(crate) set_prototype: Option<RuntimeValue>,
    pub(crate) weak_map_prototype: Option<RuntimeValue>,
    pub(crate) weak_set_prototype: Option<RuntimeValue>,
    pub(crate) regexp_prototype: Option<RuntimeValue>,
    pub(crate) promise_prototype: Option<RuntimeValue>,
    pub(crate) date_prototype: Option<RuntimeValue>,
    pub(crate) bigint_prototype: Option<RuntimeValue>,
    pub(crate) symbol_prototype: Option<RuntimeValue>,
    pub(crate) array_buffer_prototype: Option<RuntimeValue>,
    // One prototype object per typed-array element kind, mirroring C++ JSC where
    // each JSGenericTypedArrayView<Adaptor> has its own prototype (Int8Array.
    // prototype, Int32Array.prototype, ...). Indexed by typed_array_kind_index;
    // replaces the former single uint8_array_prototype field.
    pub(crate) typed_array_prototypes: [Option<RuntimeValue>; TYPED_ARRAY_KIND_COUNT],
    pub(crate) data_view_prototype: Option<RuntimeValue>,
}

/// Number of typed-array element kinds tracked for per-kind prototype storage.
/// Matches the wired Number-content constructor set plus a slot per kind in
/// TypedArrayElementKind so indexing by discriminant is total.
const TYPED_ARRAY_KIND_COUNT: usize = 12;

/// Stable index for a typed-array element kind into per-kind prototype storage,
/// mirroring the FOR_EACH_TYPED_ARRAY_TYPE ordering in C++ TypedArrayType.h.
pub(crate) fn typed_array_kind_index(kind: TypedArrayElementKind) -> usize {
    match kind {
        TypedArrayElementKind::Int8 => 0,
        TypedArrayElementKind::Uint8 => 1,
        TypedArrayElementKind::Uint8Clamped => 2,
        TypedArrayElementKind::Int16 => 3,
        TypedArrayElementKind::Uint16 => 4,
        TypedArrayElementKind::Int32 => 5,
        TypedArrayElementKind::Uint32 => 6,
        TypedArrayElementKind::Float16 => 7,
        TypedArrayElementKind::Float32 => 8,
        TypedArrayElementKind::Float64 => 9,
        TypedArrayElementKind::BigInt64 => 10,
        TypedArrayElementKind::BigUint64 => 11,
    }
}

/// Class name of a typed-array element kind, mirroring C++ JSGenericTypedArrayView
/// info().className (e.g. "Int8Array"). Used for Object.prototype.toString tags
/// and the global binding name.
pub(crate) fn typed_array_kind_name(kind: TypedArrayElementKind) -> &'static str {
    match kind {
        TypedArrayElementKind::Int8 => "Int8Array",
        TypedArrayElementKind::Uint8 => "Uint8Array",
        TypedArrayElementKind::Uint8Clamped => "Uint8ClampedArray",
        TypedArrayElementKind::Int16 => "Int16Array",
        TypedArrayElementKind::Uint16 => "Uint16Array",
        TypedArrayElementKind::Int32 => "Int32Array",
        TypedArrayElementKind::Uint32 => "Uint32Array",
        TypedArrayElementKind::Float16 => "Float16Array",
        TypedArrayElementKind::Float32 => "Float32Array",
        TypedArrayElementKind::Float64 => "Float64Array",
        TypedArrayElementKind::BigInt64 => "BigInt64Array",
        TypedArrayElementKind::BigUint64 => "BigUint64Array",
    }
}

/// The CoreNativeFunction constructor variant for a typed-array element kind.
/// Float16/BigInt kinds are not wired (no Octane consumer); they fall back to
/// the Uint8 constructor so the mapping stays total without inventing variants.
pub(crate) fn typed_array_constructor_native_function(
    kind: TypedArrayElementKind,
) -> CoreNativeFunction {
    match kind {
        TypedArrayElementKind::Int8 => CoreNativeFunction::Int8ArrayConstructor,
        TypedArrayElementKind::Uint8 => CoreNativeFunction::Uint8ArrayConstructor,
        TypedArrayElementKind::Uint8Clamped => CoreNativeFunction::Uint8ClampedArrayConstructor,
        TypedArrayElementKind::Int16 => CoreNativeFunction::Int16ArrayConstructor,
        TypedArrayElementKind::Uint16 => CoreNativeFunction::Uint16ArrayConstructor,
        TypedArrayElementKind::Int32 => CoreNativeFunction::Int32ArrayConstructor,
        TypedArrayElementKind::Uint32 => CoreNativeFunction::Uint32ArrayConstructor,
        TypedArrayElementKind::Float32 => CoreNativeFunction::Float32ArrayConstructor,
        TypedArrayElementKind::Float64 => CoreNativeFunction::Float64ArrayConstructor,
        TypedArrayElementKind::Float16
        | TypedArrayElementKind::BigInt64
        | TypedArrayElementKind::BigUint64 => CoreNativeFunction::Uint8ArrayConstructor,
    }
}

/// Inverse of typed_array_constructor_native_function for the wired Number-content
/// constructor variants. Returns None for non-typed-array native functions.
pub(crate) fn typed_array_constructor_kind(
    function: CoreNativeFunction,
) -> Option<TypedArrayElementKind> {
    match function {
        CoreNativeFunction::Int8ArrayConstructor => Some(TypedArrayElementKind::Int8),
        CoreNativeFunction::Uint8ArrayConstructor => Some(TypedArrayElementKind::Uint8),
        CoreNativeFunction::Uint8ClampedArrayConstructor => {
            Some(TypedArrayElementKind::Uint8Clamped)
        }
        CoreNativeFunction::Int16ArrayConstructor => Some(TypedArrayElementKind::Int16),
        CoreNativeFunction::Uint16ArrayConstructor => Some(TypedArrayElementKind::Uint16),
        CoreNativeFunction::Int32ArrayConstructor => Some(TypedArrayElementKind::Int32),
        CoreNativeFunction::Uint32ArrayConstructor => Some(TypedArrayElementKind::Uint32),
        CoreNativeFunction::Float32ArrayConstructor => Some(TypedArrayElementKind::Float32),
        CoreNativeFunction::Float64ArrayConstructor => Some(TypedArrayElementKind::Float64),
        _ => None,
    }
}

/// The wired Number-content typed-array element kinds, in FOR_EACH_TYPED_ARRAY_TYPE
/// order, that get a global constructor binding. Excludes Float16/BigInt kinds.
const WIRED_TYPED_ARRAY_KINDS: [TypedArrayElementKind; 9] = [
    TypedArrayElementKind::Int8,
    TypedArrayElementKind::Uint8,
    TypedArrayElementKind::Uint8Clamped,
    TypedArrayElementKind::Int16,
    TypedArrayElementKind::Uint16,
    TypedArrayElementKind::Int32,
    TypedArrayElementKind::Uint32,
    TypedArrayElementKind::Float32,
    TypedArrayElementKind::Float64,
];

impl Clone for CoreObjectStore {
    fn clone(&self) -> Self {
        let mut cloned = Self {
            objects: self.objects.clone(),
            // gc-r4 Butterfly-values: deep-clone the whole butterfly slab by index so
            // every `ButterflyHandle` stays valid across the snapshot AND the cloned
            // store owns an INDEPENDENT slab. `objects.clone()` copies each cell's
            // handle shallow; cloning the slab here in lockstep is exactly what makes
            // that sound (the clone's handle indexes the clone's slab, never the
            // source's) — the per-cell clone-independence the cell `Clone` relies on.
            butterflies: self.butterflies.clone(),
            // gc-r4 POD-ification (BoundFunction): deep-clone the bound-args slab in
            // lockstep with `objects` so every cell's `bound_args` AuxiliaryHandle stays
            // valid AND the cloned store owns an INDEPENDENT slab — the same soundness
            // argument as `butterflies` above (the cell `Clone` copies the handle shallow;
            // cloning the slab here is what makes that sound).
            bound_args_backings: self.bound_args_backings.clone(),
            // gc-r4 JSFunction-captures unit: deep-clone the captures + instance-field slabs
            // in lockstep with `objects`, exactly like `bound_args_backings`, so every
            // `AuxiliaryHandle` a cloned cell copied shallow indexes the CLONE's own slab
            // (never the source's) AND the cloned store owns INDEPENDENT slabs — the same
            // clone-independence invariant the cell's shallow handle-copy in
            // `CoreObjectCell::clone` relies on.
            captures_backings: self.captures_backings.clone(),
            instance_field_lists: self.instance_field_lists.clone(),
            // gc-r4 R4 POD-ification (Promise unit): deep-clone the reaction-list slab
            // by index in lockstep with `objects`, exactly like `butterflies`, so every
            // `PromiseReactionsHandle` a cloned cell copied shallow indexes the CLONE's
            // own slab (never the source's). This lockstep clone is what makes the
            // cell's shallow handle-copy in `CoreObjectCell::clone` sound.
            promise_reaction_lists: self.promise_reaction_lists.clone(),
            // gc-r4 RegExp unit: deep-clone the pattern-string slab in lockstep with
            // `objects` (each cell's `AuxiliaryHandle` is copied shallow) so every
            // handle stays valid AND the cloned store owns an INDEPENDENT slab — the
            // same clone-independence invariant the butterfly slab relies on.
            regexp_sources: self.regexp_sources.clone(),
            // gc-r4 ArrayBuffer unit: deep-clone the byte-backing slab by index in
            // lockstep with `objects` (each cell's `AuxiliaryHandle` is copied shallow)
            // so every handle stays valid AND the cloned store owns an INDEPENDENT slab
            // — the same clone-independence invariant as the butterfly/regexp slabs.
            array_buffer_backings: self.array_buffer_backings.clone(),
            // gc-r4 Map/Set unit: deep-clone both ordered-storage slabs in lockstep
            // with `objects` (each collection cell's `AuxiliaryHandle` is copied
            // shallow) so every handle stays valid AND the cloned store owns an
            // INDEPENDENT slab — the same clone-independence invariant the butterfly
            // slab relies on.
            map_entry_lists: self.map_entry_lists.clone(),
            set_value_lists: self.set_value_lists.clone(),
            object_indices_by_payload: HashMap::default(),
            // structure_table is keyed by StructureId handle (stable Vec slots across
            // clone), so every cloned cell's structure_id stays valid and the offset
            // graph is preserved. property_uids interns CorePropertyKey -> uid; cloning
            // it keeps interned identities stable. structure_seed_roots is keyed by each
            // prototype cell's pinned pointer payload (FIX 2); clone re-pins `objects`
            // to new addresses, so seed lookups for the re-pinned prototypes may miss
            // and fall back to fresh roots — conservative (IC misses, never wrong reads),
            // and clone is a snapshot/test path, not the hot path.
            structure_table: self.structure_table.clone(),
            property_uids: self.property_uids.clone(),
            property_keys_by_uid: self.property_keys_by_uid.clone(),
            next_property_uid: self.next_property_uid,
            structure_seed_roots: self.structure_seed_roots.clone(),
            structure_transition_watchpoints: self.structure_transition_watchpoints.clone(),
            structure_transition_watchpoints_by_structure: self
                .structure_transition_watchpoints_by_structure
                .clone(),
            fired_watchpoint_events: self.fired_watchpoint_events.clone(),
            structure_chain_invalidation_events: self.structure_chain_invalidation_events.clone(),
            object_prototype: self.object_prototype,
            function_prototype: self.function_prototype,
            array_prototype: self.array_prototype,
            string_prototype: self.string_prototype,
            number_prototype: self.number_prototype,
            boolean_prototype: self.boolean_prototype,
            error_prototype: self.error_prototype,
            type_error_prototype: self.type_error_prototype,
            reference_error_prototype: self.reference_error_prototype,
            range_error_prototype: self.range_error_prototype,
            map_prototype: self.map_prototype,
            set_prototype: self.set_prototype,
            weak_map_prototype: self.weak_map_prototype,
            weak_set_prototype: self.weak_set_prototype,
            regexp_prototype: self.regexp_prototype,
            promise_prototype: self.promise_prototype,
            date_prototype: self.date_prototype,
            bigint_prototype: self.bigint_prototype,
            symbol_prototype: self.symbol_prototype,
            array_buffer_prototype: self.array_buffer_prototype,
            typed_array_prototypes: self.typed_array_prototypes,
            data_view_prototype: self.data_view_prototype,
        };
        cloned.rebuild_object_indices();
        cloned
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreStructureTransitionWatchpointRecord {
    pub(crate) structure: StructureId,
    pub(crate) set: WatchpointSet,
}

// gc-r4 Batch 2: the per-cell offset allocator `CoreStructureIdAllocator` and the
// ad-hoc `TransitionRecord` edge cache were retired. Structure-id allocation and the
// property-addition transition graph (with offsets) now live in `structure_table`
// (the ported `StructureIdTable`), the faithful analog of C++'s Structure registry.

/// Stable identity of a stored prototype for the structure seed key.
///
/// C++ JSC: Structure identity incorporates the stored prototype (Structure.h
/// keeps m_prototype as part of the structure), so two objects with different
/// prototypes never share a Structure even with identical own-property shape.
/// The Rust seed map must mirror that, so we key the root structure by the
/// prototype's pinned pointer payload bits and distinguish absent vs
/// explicit-null prototypes.
///
/// FIX 2 divergence note: the prior keying used the prototype's CellId, but that
/// id is assigned LAZILY at heap-publish (bind_object_to_heap, ~:8385). Two
/// distinct but still-unpublished prototypes both carry CellId::default(), so
/// they collapsed into one seed bucket and their instances wrongly shared a root
/// structure. The pinned pointer payload bits (same value find()/find_mut() key
/// on, never reused while a cell is live) are unique and stable from allocation,
/// matching C++ where each distinct prototype object is a distinct m_prototype.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CorePrototypeIdentity {
    None,
    Null,
    Cell(usize),
}

// C++ JSC: a JSCell begins with m_structureID at structureIDOffset()==0
// (runtime/JSCell.h:236,293) and a JSObject's Butterfly pointer lives in a fixed
// header slot read at a constant displacement (runtime/JSObject.h:1572-1577). The
// batch-3 machine-code GET_BY_ID must reach those by absolute byte layout, so this
// cell is `#[repr(C)]` with a front-loaded, layout-asserted header:
//   - structure_id FIRST  => STRUCTURE_ID_OFFSET == 0 (JSCell::m_structureID).
//   - butterfly SECOND     => BUTTERFLY_SLOT_DISP == 8 (the JSObject `m_butterfly`
//     slot, JSObject.h:1167/1572-1577). gc-r4 Butterfly-values: this slot held a
//     cached self-referential `*const RuntimeValue` into the cell's own
//     `out_of_line_storage` (the R4 UB hazard under Stacked/Tree Borrows). It is now
//     a `ButterflyHandle` — an index into the STORE-OWNED `butterflies` slab (a
//     SEPARATE allocation), so the cell no longer points into itself. The codegen
//     contract is preserved at offset 8: the JIT GET_BY_ID DataIC loads
//     [base + BUTTERFLY_SLOT_DISP] then [storage_base + offset*8]. The bits there are
//     HANDLE bits NOW (R3-reversible, interpreter-resolved — the interpreter maps the
//     handle to the slab; the machine-code deref of these bits is not yet wired live)
//     and become the raw machine-dereffable arena butterfly POINTER at R4.
// DIVERGENCE: Clone is hand-written (see impl Clone) to copy the butterfly HANDLE
// shallow. That is sound ONLY because the cell's Clone is reached EXCLUSIVELY through
// `CoreObjectStore::clone()`, which deep-clones the whole `butterflies` slab ALONGSIDE
// `objects` (so each cloned store owns an independent slab and handles stay valid); no
// site clones a single cell into the SAME store. Default is hand-written because a
// `ButterflyHandle` sentinel (INVALID) is installed until `allocate_cell` assigns a
// real slab handle at the single allocation chokepoint.
#[derive(Debug)]
#[repr(C)]
pub(crate) struct CoreObjectCell {
    // C++ JSC JSCell::m_structureID (runtime/JSCell.h:236) at structureIDOffset()==0.
    // MUST stay the first declared field; STRUCTURE_ID_OFFSET asserts it is at 0.
    pub(crate) structure_id: StructureId,
    // C++ JSC JSCell::m_type (runtime/JSCell.h:298), the one-byte in-header JSType
    // tag read via JSCell::type() (runtime/JSCell.h:154) to decide a cell's kind
    // BEFORE downcasting it. Lands at offset 4 here, filling the 4-byte pad between
    // the 4-byte structure_id and the 8-byte-aligned storage_ptr; offset_of! asserts
    // it is at 4 on every cell kind (the fixed, kind-consistent offset a future
    // type-check-before-deref step relies on).
    //
    // DIVERGENCE: C++ places m_type at BYTE 5 of the header (byte 4 is
    // m_indexingTypeAndMisc, byte 6 m_flags, byte 7 m_cellState — the union/blob at
    // runtime/JSCell.h:294-302). The port does not yet carry m_indexingTypeAndMisc as
    // a header byte (array/indexing shape lives in CoreObjectKind + elements), so
    // m_type sits at byte 4 here. The load-bearing guarantee is OFFSET CONSISTENCY
    // across all cell kinds (asserted ==4), not byte-5 parity; exact byte-5 parity is
    // deferred until an m_indexingTypeAndMisc header byte is modeled.
    pub(crate) js_type: JsType,
    // C++ JSC: the JSObject Butterfly pointer slot (`m_butterfly`,
    // runtime/JSObject.h:1167 / 1572-1577). gc-r4 Butterfly-values: a
    // `ButterflyHandle` index into the STORE-OWNED `butterflies` slab (a separate
    // allocation), NOT a self-referential interior pointer. MUST stay the second
    // declared field; BUTTERFLY_SLOT_DISP asserts it is at byte 8 (after the 4-byte
    // structure_id + 4-byte pad to pointer alignment). `ButterflyHandle` is a
    // `#[repr(transparent)] usize`, so it occupies the same 8 bytes the raw butterfly
    // pointer will occupy at R4. The handle is set in `allocate_cell` via
    // `allocate_butterfly()`; until then it is `ButterflyHandle::INVALID` (sentinel).
    pub(crate) butterfly: ButterflyHandle,
    pub(crate) cell_id: CellId,
    pub(crate) kind: CoreObjectKind,
    pub(crate) prototype: Option<RuntimeValue>,
    pub(crate) function_index: Option<u32>,
    pub(crate) native_function: Option<CoreNativeFunction>,
    pub(crate) construct_ability: ConstructAbility,
    pub(crate) super_base: Option<RuntimeValue>,
    pub(crate) super_constructor: Option<RuntimeValue>,
    pub(crate) is_default_derived_constructor: bool,
    // C++ JSC: a class constructor's instance-field initializers (`[[Fields]]`, the
    // `class { x = e }` per-instance fields installed by ClassExprNode / `op_..._field`).
    // gc-r4 R4 POD-ification (SD-2 expedient): the `Vec<CoreInstanceField>` — whose
    // `CorePropertyKey::String` key made it Drop-bearing — is relocated to the store-owned
    // `instance_field_lists` slab as POD `CoreInstanceFieldRecord`s (the key interned to a
    // `Copy` `AtomId`). This field is now a POD `Copy` `AuxiliaryHandle` index, lazily
    // assigned by `add_instance_field` (`INVALID` until the first field). The faithful
    // class-field init is a DEFERRED correctness batch; only the storage moved.
    pub(crate) instance_fields: AuxiliaryHandle,
    // C++ JSC: a closure's captured free-variable values (faithfully the variables of a
    // JSLexicalEnvironment reached through the scope chain, JSLexicalEnvironment.h:56-80 /
    // JSCallee::m_scope). gc-r4 R4 POD-ification (SD-2 expedient): the
    // `Vec<RuntimeValue>` is relocated to the store-owned `captures_backings` slab; this
    // field is now a POD `Copy` `AuxiliaryHandle` index into it (so the cell sheds a Drop
    // field), exactly like the `bound_args` handle. Every Function cell gets a real handle
    // at `allocate_function_with_construct_ability` (even an empty set). The faithful
    // scope-chain relocation is a DEFERRED correctness batch; only the storage moved. The
    // backing holds `RuntimeValue` GC edges — a later collector trace visits the slab.
    pub(crate) captures: AuxiliaryHandle,
    pub(crate) binding_value: RuntimeValue,
    // C++ JSC has NO per-object property map: a JSObject's named property VALUE lives in
    // inline storage or the Butterfly out-of-line region, and the
    // property->offset/attributes/kind mapping lives PER-SHAPE in Structure::PropertyTable.
    // gc-r4 B-iv (DONE): the per-cell `properties` HashMap (named-property VALUE authority),
    // `property_order` (enumeration order), and the vestigial `deleted_offsets` are DELETED.
    // The Structure (offset + attributes, via `structure_table`) is the offset/attribute/
    // presence authority; the butterfly slab `props` side (keyed by the structure-assigned
    // offset) is the SOLE VALUE authority — a data slot holds the value, an accessor slot
    // holds `from_cell(GetterSetter)` (mirroring C++ `getDirect(offset)`). Reads
    // reconstruct a `CoreProperty` via `own_property_from_shape`; enumeration order comes
    // from `structure_property_keys` (the PropertyTable entry order); freed-offset recycling
    // is owned by `PropertyTable::m_deletedOffsets`. (The cell is still NOT POD — Map/Set/
    // RegExp/Promise/ArrayBuffer/Bound Drop fields remain; the `needs_drop` assert flips
    // only after those relocate, so it is NOT added here.)
    // C++ JSC: indexed elements live on the RIGHT side of the Butterfly
    // (Butterfly::contiguous(), Butterfly.h:196). gc-r4 Butterfly-values: the indexed
    // element storage is now the store-owned slab's `elements` side, reached through
    // `butterfly` (the handle above); the SOLE authority for indexed values (it had no
    // HashMap mirror). All access routes through the `butterfly_elem_*` store API.
    // C++ JSC `JSOrderedHashMap`/`JSOrderedHashSet` reach their insertion-ordered
    // entries through `m_storage` (a `JSOrderedHashTable::Storage` JSCellButterfly,
    // JSOrderedHashTable.h:164). gc-r4 R4 POD-ification (Map/Set unit): the per-cell
    // `map_entries: Vec<..>` / `set_values: Vec<..>` were RELOCATED to the store-owned
    // `map_entry_lists` / `set_value_lists` slabs; the cell holds only these POD `Copy`
    // `AuxiliaryHandle` indexes (so the fields are no longer `Drop`), exactly as the
    // `butterfly` slot holds a `ButterflyHandle`. `AuxiliaryHandle::INVALID` until the
    // owning collection's `allocate_*` site installs a real handle; a Map/WeakMap cell
    // carries `map_entries` and a Set/WeakSet cell carries `set_values`. Access routes
    // through the store's `map_entries_*` / `set_values_*` API.
    // POD expedient (NOT the faithful JSOrderedHashTable) — see the slab field comment.
    pub(crate) map_entries: AuxiliaryHandle,
    pub(crate) set_values: AuxiliaryHandle,
    // C++ JSC `RegExp::m_patternString` (runtime/RegExp.h:219), the out-of-line
    // pattern string. gc-r4 R4 POD-ification: the `String` is relocated to the
    // store-owned `regexp_sources` slab; the cell holds only this POD `Copy`
    // `AuxiliaryHandle` index (so the field is no longer `Drop`), exactly as the
    // `butterfly` slot holds a `ButterflyHandle`. `AuxiliaryHandle::INVALID` until
    // `allocate_regexp` installs a real slab handle; only RegExp cells carry one.
    pub(crate) regexp_source: AuxiliaryHandle,
    pub(crate) regexp_flags: RegexFlags,
    // C++ JSC stores NO flags string on the RegExp: the flags text is DERIVED on
    // demand from the flag bits via `Yarr::flagsString` (yarr/YarrFlags.cpp:62),
    // which backs `regExpProtoGetterFlags`. gc-r4 R4 POD-ification: the formerly
    // stored `regexp_flags_text: String` (a Drop field) is DELETED; every reader
    // recomputes the canonical-order text from the POD `regexp_flags` bits via
    // `regexp_canonical_flags_string` (the single canonical-order helper).
    pub(crate) promise_state: PromiseState,
    pub(crate) promise_result: RuntimeValue,
    // C++ JSC JSPromise `[[PromiseFulfillReactions]]`/`[[PromiseRejectReactions]]`
    // (JSPromise.h:35): a pending promise's out-of-line reaction records. gc-r4 R4
    // POD-ification (Promise unit): the `Vec<CorePromiseReaction>` was RELOCATED into
    // the store-owned `promise_reaction_lists` slab; this is now a POD (`Copy`)
    // `PromiseReactionsHandle` index into it (`INVALID` until the first reaction is
    // enqueued). Dropping the `Vec` here removes a `Drop` field so the cell is one
    // step closer to sweep-eligible POD. Access routes through the store's
    // `push_promise_reaction`/`take_promise_reactions`.
    pub(crate) promise_reactions: PromiseReactionsHandle,
    pub(crate) promise_resolving_kind: Option<CorePromiseResolvingKind>,
    pub(crate) native_bound_promise: Option<RuntimeValue>,
    pub(crate) native_bound_proxy: Option<RuntimeValue>,
    /// C++ JSC: NumberObject/BooleanObject/StringObject internal value.
    /// Mirrors JSC's NumberObject::internalValue() / BooleanObject::internalValue().
    pub(crate) primitive_value: Option<RuntimeValue>,
    pub(crate) date_value: f64,
    // C++ JSC `ArrayBufferContents::m_data` (runtime/ArrayBuffer.h:126), the raw
    // byte buffer backing an ArrayBuffer. gc-r4 R4 POD-ification: the `Vec<u8>` is
    // relocated to the store-owned `array_buffer_backings` slab; the cell holds only
    // this POD `Copy` `AuxiliaryHandle` index (so the field is no longer `Drop`),
    // exactly as the `butterfly` slot holds a `ButterflyHandle`. `AuxiliaryHandle::
    // INVALID` until `allocate_array_buffer` installs a real slab handle; only
    // ArrayBuffer cells carry one. The bytes are raw (NOT GC edges), so reads/writes
    // route through the store's `array_buffer_bytes`/`array_buffer_bytes_mut` with no
    // write barrier.
    pub(crate) array_buffer_data: AuxiliaryHandle,
    pub(crate) view_buffer: Option<RuntimeValue>,
    pub(crate) view_byte_offset: usize,
    pub(crate) view_byte_length: usize,
    pub(crate) view_length: usize,
    // C++ JSC JSArrayBufferView is parameterized by one TypedArrayType; the Rust
    // mirror keeps a single CoreObjectKind::Uint8Array view variant and carries
    // the element kind here (size + store/load coercion). Only meaningful when
    // kind == CoreObjectKind::Uint8Array; defaults to Int8 for other cells.
    pub(crate) view_element_kind: TypedArrayElementKind,
    pub(crate) proxy_target: Option<RuntimeValue>,
    pub(crate) proxy_handler: Option<RuntimeValue>,
    /// C++ JSC JSBoundFunction: [[BoundTargetFunction]], [[BoundThis]], and
    /// [[BoundArguments]]. Only populated for CoreObjectKind::BoundFunction.
    /// `bound_target`/`bound_this` are already POD (`Option<RuntimeValue>`/`RuntimeValue`).
    pub(crate) bound_target: Option<RuntimeValue>,
    pub(crate) bound_this: RuntimeValue,
    /// C++ JSC JSBoundFunction::m_boundArgs ([[BoundArguments]]) is an out-of-line value
    /// array (runtime/JSBoundFunction.h:133). gc-r4 POD-ification: the value array is
    /// relocated to the store-owned `bound_args_backings` slab; this field carries only a
    /// POD `AuxiliaryHandle` (Copy slab index) into it — mirroring the `butterfly`
    /// handle, so the cell stays `Drop`-free (sweepable). `INVALID` for any non-bound
    /// cell; `allocate_bound_function` installs a real handle via `allocate_bound_args`.
    /// The backing array still holds `RuntimeValue` GC edges (a later collector trace
    /// visits the slab; no trace wiring in this unit).
    pub(crate) bound_args: AuxiliaryHandle,
    /// C++ JSC runtime/GetterSetter.h:132-133: GetterSetter::m_getter / m_setter, an
    /// accessor's getter and setter functions. Only meaningful when
    /// kind == CoreObjectKind::GetterSetter; a null getter/setter is `None`
    /// (GetterSetter.h treats the missing half as the undefined sentinel). These are
    /// POD (`Copy` `Option<RuntimeValue>`), so they do NOT add a `Drop` field — the
    /// cell stays sweep-eligible for R4 (gc-r4 B-ii).
    pub(crate) getter_value: Option<RuntimeValue>,
    pub(crate) setter_value: Option<RuntimeValue>,
}

// gc-r4 R4 POD-ification COMPLETE (final per-kind unit): every variable-size /
// Drop-bearing field — the property HashMap (B-iv), bound_args, promise_reactions,
// regexp_source, regexp_flags_text, array_buffer_data, map_entries, set_values,
// captures, instance_fields — has been relocated off the cell into store-owned
// auxiliary slabs reached by POD `Copy` handles (or deleted/recomputed). So
// CoreObjectCell is now POD. This compile-time assert is the ATOMIC sweepability
// proof: a MarkedBlock sweep for DestructionMode::DoesNotNeedDestruction
// (runtime/JSCell.h:105) runs NO destructors, so the cell MUST have none.
// Reintroducing ANY Drop-bearing field (String/Vec/Box/HashMap/...) fails the build
// HERE, before R3/R4 (arena cell identity) and the collector sweep rely on it.
const _: () = assert!(
    !std::mem::needs_drop::<CoreObjectCell>(),
    "CoreObjectCell must be POD (no Drop) for the R4 MarkedBlock sweep — a Drop field was reintroduced"
);

// C++ JSC JSCell::structureIDOffset()==0 (runtime/JSCell.h:293): the StructureID
// (a 4-byte id) is the first header word so a guard can `load32 [base+0]; cmp32`.
// The batch-3 assembler takes structure_id_offset as a parameter; this const is the
// value it must be given, and the assert pins the field at byte 0 so a silent
// field-reorder cannot desynchronize the codegen from the layout.
const STRUCTURE_ID_OFFSET: usize = std::mem::offset_of!(CoreObjectCell, structure_id);
// C++ JSC: the JSObject Butterfly pointer slot (`m_butterfly`,
// runtime/JSObject.h:1167 / 1572-1577) read at a constant displacement.
// BUTTERFLY_SLOT_DISP is the Rust analog displacement the codegen uses to fetch the
// storage base before the offset-indexed property load. gc-r4 Butterfly-values: the
// slot holds a `ButterflyHandle` (interpreter-resolved at R3) at the SAME offset 8 the
// raw arena butterfly pointer will occupy at R4, so the codegen contract is unchanged.
const BUTTERFLY_SLOT_DISP: usize = std::mem::offset_of!(CoreObjectCell, butterfly);

// Compile-time layout guards. These fail the build if the #[repr(C)] header order
// changes, if alignment padding shifts the butterfly slot, or if RuntimeValue stops
// being an 8-byte EncodedJsValue (the [storage_base + offset*8] stride assumption).
const _: () = assert!(
    STRUCTURE_ID_OFFSET == 0,
    "CoreObjectCell::structure_id must be at offset 0 (JSCell::structureIDOffset()==0)"
);
const _: () = assert!(
    BUTTERFLY_SLOT_DISP == 8,
    "CoreObjectCell::butterfly must be at byte 8 (JSObject m_butterfly slot analog)"
);
const _: () = assert!(
    std::mem::size_of::<ButterflyHandle>() == 8,
    "ButterflyHandle must be 8 bytes (occupies the raw butterfly-pointer slot at R4)"
);
const _: () = assert!(
    std::mem::size_of::<RuntimeValue>() == 8,
    "RuntimeValue must be 8 bytes (EncodedJsValue) for the [storage_base + offset*8] stride"
);
// C++ JSC JSCell::m_type analog (runtime/JSCell.h:298). The FIXED, kind-consistent
// offset of the in-cell JSType tag: it must be identical across every cell kind so a
// future type-check-before-downcast can read it blind. Pinned at 4 on all four cell
// kinds (object here; string/symbol/bigint asserts at their struct defs). See the
// js_type field comment for why offset 4 (not C++ byte 5) and why that is sound.
const _: () = assert!(
    std::mem::offset_of!(CoreObjectCell, js_type) == 4,
    "CoreObjectCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);

impl Default for CoreObjectCell {
    fn default() -> Self {
        // C++ has no exact analog (a fresh JSObject's Butterfly is null until the
        // allocator hands it one). The Rust analog installs the INVALID sentinel
        // handle; `allocate_cell` assigns a real store-owned slab handle via
        // `allocate_butterfly()` at the single allocation chokepoint, BEFORE the cell
        // is published and BEFORE its out-of-line storage is filled.
        Self {
            structure_id: StructureId::default(),
            // Default kind is Ordinary => FinalObject; allocate_cell overwrites this
            // from cell.kind.js_type() for every published cell, so the tag always
            // matches the final kind regardless of how the cell was built.
            js_type: JsType::FinalObject,
            butterfly: ButterflyHandle::INVALID,
            cell_id: CellId::default(),
            kind: CoreObjectKind::default(),
            prototype: None,
            function_index: None,
            native_function: None,
            construct_ability: ConstructAbility::default(),
            super_base: None,
            super_constructor: None,
            is_default_derived_constructor: false,
            // gc-r4 R4 POD-ification (captures unit): INVALID sentinels — a default cell
            // has no instance-field slab slot until `add_instance_field` lazily allocates
            // one, and no captures slab slot until `allocate_function_*` assigns one.
            instance_fields: AuxiliaryHandle::INVALID,
            captures: AuxiliaryHandle::INVALID,
            binding_value: RuntimeValue::default(),
            // gc-r4 Map/Set unit: the INVALID sentinel — a non-collection cell never
            // indexes the ordered-storage slabs. The owning collection's `allocate_*`
            // site overwrites the relevant field with a real handle.
            map_entries: AuxiliaryHandle::INVALID,
            set_values: AuxiliaryHandle::INVALID,
            regexp_source: AuxiliaryHandle::INVALID,
            regexp_flags: RegexFlags::default(),
            promise_state: PromiseState::default(),
            promise_result: RuntimeValue::default(),
            // gc-r4 R4 POD-ification (Promise unit): the INVALID sentinel — no
            // reaction-list slab slot exists until `push_promise_reaction` lazily
            // allocates one (C++ JSPromise's reaction fields start empty).
            promise_reactions: PromiseReactionsHandle::INVALID,
            promise_resolving_kind: None,
            native_bound_promise: None,
            native_bound_proxy: None,
            primitive_value: None,
            date_value: 0.0,
            // No byte backing for a default (non-ArrayBuffer) cell; the sentinel never
            // indexes the slab. allocate_array_buffer overwrites it with a real handle.
            array_buffer_data: AuxiliaryHandle::INVALID,
            view_buffer: None,
            view_byte_offset: 0,
            view_byte_length: 0,
            view_length: 0,
            view_element_kind: TypedArrayElementKind::default(),
            proxy_target: None,
            proxy_handler: None,
            bound_target: None,
            bound_this: RuntimeValue::default(),
            // No bound-args backing for a default (non-bound) cell; the sentinel never
            // indexes the slab. allocate_bound_function overwrites it with a real handle.
            bound_args: AuxiliaryHandle::INVALID,
            getter_value: None,
            setter_value: None,
        }
    }
}

impl Clone for CoreObjectCell {
    fn clone(&self) -> Self {
        // gc-r4 Butterfly-values: copy the `butterfly` HANDLE shallow (it is a plain
        // Copy slab index, no longer a self-referential pointer to RECOMPUTE). This is
        // sound because the cell's Clone is reached ONLY through
        // `CoreObjectStore::clone()`, which deep-clones the whole `butterflies` slab
        // ALONGSIDE `objects` — so the cloned store owns an independent slab and the
        // copied handle indexes the clone's own butterfly, never the source's. No site
        // clones a single cell into the SAME store (which WOULD alias the slab entry).
        Self {
            structure_id: self.structure_id,
            // Copy the type tag normally; a clone of an object cell is the same JSType.
            js_type: self.js_type,
            butterfly: self.butterfly,
            cell_id: self.cell_id,
            kind: self.kind,
            prototype: self.prototype,
            function_index: self.function_index,
            native_function: self.native_function.clone(),
            construct_ability: self.construct_ability,
            super_base: self.super_base,
            super_constructor: self.super_constructor,
            is_default_derived_constructor: self.is_default_derived_constructor,
            // gc-r4 R4 POD-ification (captures unit): copy the instance-field + captures
            // HANDLES shallow (plain Copy slab indices). Sound for the SAME reason as
            // `butterfly`/`bound_args`: cell Clone is reached ONLY through
            // `CoreObjectStore::clone`, which deep-clones `instance_field_lists` and
            // `captures_backings` in lockstep, so the copied handles index the clone's own
            // slabs, never the source's.
            instance_fields: self.instance_fields,
            captures: self.captures,
            binding_value: self.binding_value,
            // gc-r4 Map/Set unit: copy the ordered-storage HANDLES shallow (POD `Copy`
            // slab indexes). Sound for the SAME reason as `butterfly`: the cell Clone is
            // reached ONLY through `CoreObjectStore::clone()`, which deep-clones both
            // `map_entry_lists`/`set_value_lists` slabs in lockstep, so a copied handle
            // indexes the clone's own slab, never the source's.
            map_entries: self.map_entries,
            set_values: self.set_values,
            // Copy the pattern-string HANDLE shallow (POD `AuxiliaryHandle`); sound
            // for the same reason as `butterfly` — the cell's Clone is reached only
            // via `CoreObjectStore::clone()`, which deep-clones `regexp_sources`
            // alongside `objects`, so the copied handle indexes the clone's own slab.
            regexp_source: self.regexp_source,
            regexp_flags: self.regexp_flags,
            promise_state: self.promise_state,
            promise_result: self.promise_result,
            // gc-r4 R4 POD-ification (Promise unit): copy the reaction-list HANDLE
            // shallow (a plain Copy slab index). Sound for the SAME reason as
            // `butterfly`: cell Clone is reached ONLY through `CoreObjectStore::clone`,
            // which deep-clones `promise_reaction_lists` in lockstep, so the copied
            // handle indexes the clone's own slab, never the source's.
            promise_reactions: self.promise_reactions,
            promise_resolving_kind: self.promise_resolving_kind,
            native_bound_promise: self.native_bound_promise,
            native_bound_proxy: self.native_bound_proxy,
            primitive_value: self.primitive_value,
            date_value: self.date_value,
            // gc-r4 ArrayBuffer unit: copy the byte-backing HANDLE shallow (a plain
            // Copy slab index). Sound for the SAME reason as `butterfly`: cell Clone is
            // reached ONLY through `CoreObjectStore::clone`, which deep-clones
            // `array_buffer_backings` in lockstep, so the copied handle indexes the
            // clone's own slab, never the source's.
            array_buffer_data: self.array_buffer_data,
            view_buffer: self.view_buffer,
            view_byte_offset: self.view_byte_offset,
            view_byte_length: self.view_byte_length,
            view_length: self.view_length,
            view_element_kind: self.view_element_kind,
            proxy_target: self.proxy_target,
            proxy_handler: self.proxy_handler,
            bound_target: self.bound_target,
            bound_this: self.bound_this,
            // gc-r4 POD-ification: copy the bound-args HANDLE shallow (a plain Copy slab
            // index). Sound for the SAME reason as the `butterfly` handle above: the cell
            // Clone is reached ONLY through `CoreObjectStore::clone()`, which deep-clones
            // the whole `bound_args_backings` slab alongside `objects`, so the copied
            // handle indexes the clone's own backing, never the source's.
            bound_args: self.bound_args,
            getter_value: self.getter_value,
            setter_value: self.setter_value,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum CoreObjectKind {
    #[default]
    Ordinary,
    Array,
    Function,
    NativeFunction,
    // C++ JSC JSBoundFunction (runtime/JSBoundFunction.h): the object created by
    // Function.prototype.bind. Stores the target function, bound `this`, and
    // bound leading arguments in bound_target/bound_this/bound_args.
    BoundFunction,
    ClosureCell,
    Map,
    Set,
    WeakMap,
    WeakSet,
    RegExp,
    Promise,
    Date,
    ArrayBuffer,
    Uint8Array,
    DataView,
    Proxy,
    // C++ JSC runtime/GetterSetter.h:42: a GetterSetter is a fixed cell holding a
    // property's getter and setter functions (m_getter/m_setter, GetterSetter.h:
    // 132-133). gc-r4 B-ii: an accessor property's butterfly slot holds
    // `from_cell(GetterSetter)` exactly as C++ stores a `GetterSetter*`. It is NOT a
    // JSObject in C++ (GetterSetterType sits below ObjectType in runtime/JSType.h) and
    // is never a JS-visible value here — it lives only inside accessor butterfly slots
    // — so the collapse to JsType::Object in js_type() below is internal-only and never
    // reaches an is_object()/typeof JS check.
    GetterSetter,
}

impl CoreObjectKind {
    /// Faithful `JSCell::m_type` (runtime/JSCell.h:298) for an object cell of this
    /// kind. A plain ordinary `{}` object is JSC `FinalObjectType` (runtime/JSType.h:78);
    /// every other kind is mapped to the `ObjectType` object-range umbrella
    /// (runtime/JSType.h:77).
    ///
    /// DIVERGENCE / known under-modeling: C++ gives each JSObject subclass its own
    /// JSType (ArrayType, JSFunctionType, JSPromiseType, JSDateType, ProxyObjectType,
    /// Uint8ArrayType, ...; runtime/JSType.h:80-160). This port collapses all of them
    /// to `Object` for now. That is faithful for the object-vs-primitive distinction
    /// `is_object()` needs (and for a type-check-before-downcast gate, which only needs
    /// the object range); per-subclass JSType refinement is deferred and localized to
    /// this single helper so it can be sharpened in one place.
    pub(crate) fn js_type(self) -> JsType {
        match self {
            CoreObjectKind::Ordinary => JsType::FinalObject,
            CoreObjectKind::Array
            | CoreObjectKind::Function
            | CoreObjectKind::NativeFunction
            | CoreObjectKind::BoundFunction
            | CoreObjectKind::ClosureCell
            | CoreObjectKind::Map
            | CoreObjectKind::Set
            | CoreObjectKind::WeakMap
            | CoreObjectKind::WeakSet
            | CoreObjectKind::RegExp
            | CoreObjectKind::Promise
            | CoreObjectKind::Date
            | CoreObjectKind::ArrayBuffer
            | CoreObjectKind::Uint8Array
            | CoreObjectKind::DataView
            | CoreObjectKind::Proxy
            // GetterSetter is C++ GetterSetterType (non-object) but is never a
            // JS-visible value (accessor-slot internal only); folding it into the
            // Object umbrella keeps the match total without a JsType variant it never
            // needs on a JS path. See the CoreObjectKind::GetterSetter comment.
            | CoreObjectKind::GetterSetter => JsType::Object,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreNativeFunction {
    ObjectConstructor,
    ObjectPrototypeHasOwnProperty,
    ObjectPrototypeToString,
    ObjectPrototypeValueOf,
    ObjectDefineGetter,
    ObjectDefineSetter,
    FunctionCall,
    // C++ JSC FunctionPrototype.cpp: Function.prototype.apply
    // (functionPrototypeApplyCodeGenerator) and Function.prototype.bind
    // (functionProtoFuncBind). `apply` reuses the same call path as `call`
    // (execute_function_value_with_completion), sourcing arguments from the
    // array-like argument instead of varargs. `bind` allocates a
    // CoreObjectKind::BoundFunction (Rust mirror of JSBoundFunction).
    FunctionApply,
    FunctionBind,
    // C++ JSC FunctionConstructor (runtime/FunctionConstructor.cpp): the global
    // `Function`. Rust does not implement dynamic source compilation
    // (`new Function(string)`), so this native function only exists to give the
    // shared function prototype a `constructor` and to expose `Function` as a
    // global for `typeof`/`instanceof`; calling it throws (see
    // native_function_constructor).
    FunctionConstructor,
    ArrayConstructor,
    ArrayIsArray,
    ArrayFrom,
    ArrayOf,
    MathAbs,
    MathFloor,
    MathLog,
    MathMax,
    MathMin,
    MathPow,
    MathRandom,
    MathSqrt,
    MathTrunc,
    MathCeil,
    MathRound,
    MathSign,
    MathExp,
    MathCbrt,
    MathLog2,
    MathLog10,
    MathSin,
    MathCos,
    MathTan,
    MathAsin,
    MathAcos,
    MathAtan,
    MathAtan2,
    MathSinh,
    MathCosh,
    MathTanh,
    MathAsinh,
    MathAcosh,
    MathAtanh,
    MathExpm1,
    MathLog1p,
    MathHypot,
    ParseInt,
    ParseFloat,
    // C++ JSC GlobalObjectMethodTable / globalFuncIsFinite & globalFuncIsNaN
    // (runtime/JSGlobalObjectFunctions.cpp): the global `isFinite`/`isNaN`
    // functions. Both ToNumber the argument then test finiteness/NaN.
    GlobalIsFinite,
    GlobalIsNaN,
    // C++ JSC globalFuncEscape / globalFuncUnescape / globalFuncDecodeURI /
    // globalFuncDecodeURIComponent / globalFuncEncodeURI /
    // globalFuncEncodeURIComponent (runtime/JSGlobalObjectFunctions.cpp:566-705).
    // Installed on the global object with DontEnum (JSGlobalObject.cpp:699-704).
    GlobalEscape,
    GlobalUnescape,
    GlobalDecodeURI,
    GlobalDecodeURIComponent,
    GlobalEncodeURI,
    GlobalEncodeURIComponent,
    HostPerformanceNow,
    HostPrint,
    HostAlert,
    HostConsoleLog,
    HostConsoleInfo,
    HostConsoleWarn,
    HostConsoleError,
    HostReadFile,
    HostCurrentResolve,
    HostCurrentReject,
    JsonParse,
    JsonStringify,
    ReflectApply,
    ReflectDeleteProperty,
    ReflectGet,
    ReflectGetOwnPropertyDescriptor,
    ReflectGetPrototypeOf,
    ReflectHas,
    ReflectOwnKeys,
    ReflectSet,
    ReflectSetPrototypeOf,
    ProxyConstructor,
    ProxyRevocable,
    ProxyRevoke,
    StringConstructor,
    StringFromCharCode,
    NumberConstructor,
    NumberPrototypeToString,
    NumberPrototypeValueOf,
    BooleanConstructor,
    ErrorConstructor,
    TypeErrorConstructor,
    ReferenceErrorConstructor,
    ErrorPrototypeToString,
    MapConstructor,
    MapGet,
    MapSet,
    MapHas,
    MapDelete,
    MapClear,
    MapSize,
    SetConstructor,
    SetAdd,
    SetHas,
    SetDelete,
    SetClear,
    SetSize,
    WeakMapConstructor,
    WeakMapGet,
    WeakMapSet,
    WeakMapHas,
    WeakMapDelete,
    WeakSetConstructor,
    WeakSetAdd,
    WeakSetHas,
    WeakSetDelete,
    RegExpConstructor,
    RegExpTest,
    RegExpExec,
    RegExpPrototypeToString,
    // RegExp.prototype accessor getters. C++ JSC installs each as a distinct
    // native getter in RegExpPrototype::finishCreation (runtime/RegExpPrototype.cpp:81-90):
    // regExpProtoGetterSource (:446), regExpProtoGetterFlags (:429), and the
    // per-flag boolean getters regExpProtoGetterGlobal/HasIndices/IgnoreCase/
    // Multiline/DotAll/Sticky/Unicode/UnicodeSets (:301-427).
    RegExpProtoGetterSource,
    RegExpProtoGetterFlags,
    RegExpProtoGetterGlobal,
    RegExpProtoGetterHasIndices,
    RegExpProtoGetterIgnoreCase,
    RegExpProtoGetterMultiline,
    RegExpProtoGetterDotAll,
    RegExpProtoGetterSticky,
    RegExpProtoGetterUnicode,
    RegExpProtoGetterUnicodeSets,
    PromiseConstructor,
    PromiseResolve,
    PromiseReject,
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    PromiseResolvingFunction,
    DateConstructor,
    DateNow,
    DateParse,
    DateUtc,
    DateGetTime,
    DateValueOf,
    DateToISOString,
    DatePrototypeToString,
    BigIntConstructor,
    BigIntPrototypeToString,
    BigIntPrototypeValueOf,
    ArrayBufferConstructor,
    ArrayBufferByteLength,
    ArrayBufferSlice,
    Uint8ArrayConstructor,
    // Number-content typed-array constructors. Each is a distinct C++
    // JSGenericTypedArrayView<Adaptor> constructor; they share one Rust
    // constructor body parameterized by element kind (native_typed_array_
    // constructor). BigInt64/BigUint64/Float16 are not wired (no Octane consumer
    // and they need ToBigInt / f16 narrowing not present on the value path).
    Int8ArrayConstructor,
    Uint8ClampedArrayConstructor,
    Int16ArrayConstructor,
    Uint16ArrayConstructor,
    Int32ArrayConstructor,
    Uint32ArrayConstructor,
    Float32ArrayConstructor,
    Float64ArrayConstructor,
    Uint8ArrayLength,
    Uint8ArrayByteLength,
    Uint8ArrayByteOffset,
    Uint8ArrayBuffer,
    Uint8ArrayFill,
    Uint8ArraySet,
    Uint8ArraySubarray,
    DataViewConstructor,
    DataViewBuffer,
    DataViewByteLength,
    DataViewByteOffset,
    DataViewGetUint8,
    DataViewSetUint8,
    DataViewGetInt8,
    DataViewSetInt8,
    SymbolConstructor,
    SymbolFor,
    SymbolKeyFor,
    SymbolDescription,
    SymbolPrototypeToString,
    SymbolPrototypeValueOf,
    ArrayPush,
    ArrayPop,
    ArrayShift,
    ArrayUnshift,
    ArrayJoin,
    ArrayPrototypeToString,
    ArraySlice,
    ArrayConcat,
    ArrayFill,
    ArrayReverse,
    ArraySort,
    ArraySplice,
    ArrayIndexOf,
    ArrayIncludes,
    ArrayForEach,
    ArrayMap,
    ArrayFilter,
    ArraySome,
    ArrayEvery,
    ArrayFind,
    ArrayFindIndex,
    ArrayReduce,
    ArrayReduceRight,
    StringCharAt,
    StringCharCodeAt,
    StringIndexOf,
    StringLastIndexOf,
    StringSlice,
    StringSubstring,
    StringSubstr,
    StringSplit,
    StringReplace,
    StringMatch,
    StringToLowerCase,
    StringToUpperCase,
    StringToLocaleLowerCase,
    StringToLocaleUpperCase,
    Assign,
    Create,
    DefineProperty,
    Entries,
    GetOwnPropertyDescriptor,
    GetPrototypeOf,
    HasOwn,
    Keys,
    SetPrototypeOf,
    Values,
    // C++ JSC globalFuncEval (runtime/JSGlobalObjectFunctions.cpp:450): the
    // global `eval`. INDIRECT/global eval only. The native arm cannot compile
    // here (it would re-enter the compile pipeline while DispatchState borrows
    // are live); instead it returns `DispatchOutcome::EvalRequest`, deferring
    // compile+run to the Vm, which owns the compile pipeline (Option A).
    GlobalEval,
}

impl CoreNativeFunction {
    // C++ JSC: every Array.prototype instance method begins with
    // `callFrame->thisValue().toThis(globalObject, strict).toObject(globalObject)`
    // -- e.g. arrayProtoFuncSlice (runtime/ArrayPrototype.cpp:735),
    // arrayProtoFuncJoin (:444), arrayProtoFuncReverse (:597),
    // arrayProtoFuncIndexOf (:1326), arrayProtoFuncPush (:566). For an
    // undefined/null `this`, toObject -> toObjectSlowCase
    // (runtime/JSCJSValue.cpp:169-171) throws a CATCHABLE TypeError via
    // throwException(createNotAnObjectError(...)), NOT a VM abort. This
    // predicate identifies the Array.prototype *instance* methods that funnel
    // through ToObject(this); the static methods (ArrayConstructor/ArrayIsArray
    // /ArrayFrom/ArrayOf) do not take `this` and are excluded.
    pub(crate) const fn is_array_to_object_this(self) -> bool {
        matches!(
            self,
            Self::ArrayPush
                | Self::ArrayPop
                | Self::ArrayShift
                | Self::ArrayUnshift
                | Self::ArrayJoin
                | Self::ArrayPrototypeToString
                | Self::ArraySlice
                | Self::ArrayConcat
                | Self::ArrayFill
                | Self::ArrayReverse
                | Self::ArraySort
                | Self::ArraySplice
                | Self::ArrayIndexOf
                | Self::ArrayIncludes
                | Self::ArrayForEach
                | Self::ArrayMap
                | Self::ArrayFilter
                | Self::ArraySome
                | Self::ArrayEvery
                | Self::ArrayFind
                | Self::ArrayFindIndex
                | Self::ArrayReduce
                | Self::ArrayReduceRight
        )
    }

    pub(crate) const fn intrinsic(self) -> Option<NativeIntrinsic> {
        match self {
            Self::StringCharCodeAt => Some(NativeIntrinsic::StringCharCodeAt),
            Self::StringIndexOf => Some(NativeIntrinsic::StringIndexOf),
            Self::StringLastIndexOf => Some(NativeIntrinsic::StringLastIndexOf),
            Self::StringSubstring => Some(NativeIntrinsic::StringSubstring),
            _ => None,
        }
    }

    pub(crate) fn construct_ability(self) -> ConstructAbility {
        match self {
            Self::ObjectConstructor
            | Self::ArrayConstructor
            | Self::ProxyConstructor
            | Self::NumberConstructor
            | Self::BooleanConstructor
            | Self::StringConstructor
            | Self::ErrorConstructor
            | Self::TypeErrorConstructor
            | Self::ReferenceErrorConstructor
            | Self::MapConstructor
            | Self::SetConstructor
            | Self::WeakMapConstructor
            | Self::WeakSetConstructor
            | Self::RegExpConstructor
            | Self::PromiseConstructor
            | Self::DateConstructor
            | Self::ArrayBufferConstructor
            | Self::Uint8ArrayConstructor
            | Self::Int8ArrayConstructor
            | Self::Uint8ClampedArrayConstructor
            | Self::Int16ArrayConstructor
            | Self::Uint16ArrayConstructor
            | Self::Int32ArrayConstructor
            | Self::Uint32ArrayConstructor
            | Self::Float32ArrayConstructor
            | Self::Float64ArrayConstructor
            | Self::DataViewConstructor => ConstructAbility::CanConstruct,
            _ => ConstructAbility::CannotConstruct,
        }
    }

    pub(crate) fn not_a_constructor_message(self) -> &'static str {
        match self {
            Self::BigIntConstructor => "BigInt is not a constructor",
            Self::SymbolConstructor => "Symbol is not a constructor",
            Self::MathMax => "Math.max is not a constructor",
            _ => "Function is not a constructor",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CoreInstanceField {
    pub(crate) key: CorePropertyKey,
    pub(crate) initializer: Option<RuntimeValue>,
}

/// POD storage form of a `CoreInstanceField` for the store-owned `instance_field_lists`
/// slab (gc-r4 R4 POD-ification, JSFunction-captures unit / SD-2).
///
/// C++ JSC keys a class field by its `UniquedStringImpl*`/Symbol identity. `CoreInstanceField`
/// keeps a `CorePropertyKey`, whose `String` variant is `Drop`-bearing — storing it on the
/// cell path (even via the slab) would keep the cell from being POD in the faithful sense.
/// This record stores the key as a `Copy` `AtomId` uid instead (interned via
/// `intern_property_uid`, recovered via `property_keys_by_uid`), so the whole record is POD
/// (`Copy`): `AtomId` is `Copy` and `Option<RuntimeValue>` is `Copy`. The initializer is a
/// `RuntimeValue` GC edge a later collector trace MUST visit (gc-r4 GAP A).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CoreInstanceFieldRecord {
    pub(crate) key_uid: AtomId,
    pub(crate) initializer: Option<RuntimeValue>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CorePropertyKey {
    Identifier(u32),
    String(String),
    Symbol(u64),
}

impl CorePropertyKey {
    pub(crate) fn is_string(&self, text: &str) -> bool {
        matches!(self, Self::String(value) if value == text)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GeneratedPropertyLoadCoreKey<'a> {
    Identifier(u32),
    String(&'a str),
}

impl<'a> GeneratedPropertyLoadCoreKey<'a> {
    pub(crate) fn supports_named_property_offset(self) -> bool {
        match self {
            Self::Identifier(_) => true,
            Self::String(text) => parse_array_index_name(text).is_none(),
        }
    }

    pub(crate) fn to_core_property_key(self) -> CorePropertyKey {
        match self {
            Self::Identifier(identifier) => CorePropertyKey::Identifier(identifier),
            Self::String(text) => CorePropertyKey::String(text.to_owned()),
        }
    }
}

pub(crate) fn generated_property_load_cell_has_own_property(
    objects: &CoreObjectStore,
    cell: &CoreObjectCell,
    key: GeneratedPropertyLoadCoreKey<'_>,
) -> bool {
    // gc-r4 B-iv: own-named-property presence is a function of the cell's Structure
    // (the offset authority), not a per-cell HashMap. `key` is a named load key (the
    // caller already gated `supports_named_property_offset`), so presence == the shape
    // assigning it an offset (C++ `structure->get(...)` returning a valid offset).
    objects
        .structure_property(cell.structure_id, &key.to_core_property_key())
        .is_some()
}

pub(crate) fn generated_property_load_cell_data_property_at_offset(
    objects: &CoreObjectStore,
    cell: &CoreObjectCell,
    key: GeneratedPropertyLoadCoreKey<'_>,
    expected_offset: PropertyOffset,
) -> Option<RuntimeValue> {
    // C++ JSC JSObject::getDirect(offset)/locationForOffset (JSObject.h:711,748):
    // once the structure guard holds (verified by the caller against
    // entry.structure / base_structure), the value is read directly at the
    // structure-assigned offset with NO key comparison or HashMap scan. This is
    // exactly the offset-indexed load batch 3 will emit as
    // `mov reg <- [storage_base + offset*8]` from the butterfly.
    //
    // gc-r4 Butterfly-values: the offset slot lives in the store-owned butterfly slab
    // reached by `cell.butterfly`.
    //
    // gc-r4 B-iii: accessors now ALSO occupy a real Structure offset (their butterfly
    // slot holds a `from_cell(GetterSetter)`), so the structure guard ALONE no longer
    // proves the slot is a DATA value. Gate the data-load fast path on the structure's
    // attributes NOT carrying `PropertyAttribute::Accessor` — exactly the precondition
    // C++ checks before emitting an `AccessCase::Load` (an accessor gets an
    // `AccessCase::Getter` stub instead, never this data load). This reads the SHAPE
    // (the offset/attribute authority), not the per-cell value HashMap, so it does not
    // change the VALUE authority this batch.
    if let Some((_, attributes)) =
        objects.structure_property(cell.structure_id, &key.to_core_property_key())
    {
        if attributes & PROPERTY_ATTRIBUTE_ACCESSOR != 0 {
            return None;
        }
    }
    objects.butterfly_prop_get(cell.butterfly, expected_offset)
}

pub(crate) fn generated_property_load_offset_miss_reason(
    objects: &CoreObjectStore,
    cell: &CoreObjectCell,
    key: GeneratedPropertyLoadCoreKey<'_>,
    expected_offset: PropertyOffset,
    actual_offset: Option<PropertyOffset>,
) -> GeneratedPropertyLoadProbeMissReason {
    use GeneratedPropertyLoadProbeMissReason as Miss;

    // Diagnostic-only classification when the offset-indexed read returned None. gc-r4
    // B-iv: presence + data-vs-accessor come from the cell's Structure::PropertyTable
    // (the offset/attribute authority), not the deleted per-cell HashMap; `actual_offset`
    // is the key's real offset in that table, supplied by the caller so a cached offset
    // that disagrees with the structure is reported as KeyOffsetMismatch.
    let Some((_, attributes)) =
        objects.structure_property(cell.structure_id, &key.to_core_property_key())
    else {
        return Miss::MissingProperty;
    };
    if attributes & PROPERTY_ATTRIBUTE_ACCESSOR != 0 {
        return Miss::NonDataProperty;
    }
    match actual_offset {
        Some(actual) if actual != expected_offset => Miss::KeyOffsetMismatch,
        _ => Miss::MissingOrInvalidOffset,
    }
}

pub(crate) fn core_property_key_supports_named_property_offset(key: &CorePropertyKey) -> bool {
    // gc-r4 B-iii/B-iv: Identifier, non-index String, and Symbol keys get a real named
    // Structure offset (so they have a butterfly slot home once the per-cell `properties`
    // HashMap is gone). C++ keys the Structure PropertyTable/transition table by a
    // property's uniqued uid, and `intern_property_uid` uniques any such key.
    //
    // ARRAY-INDEX strings are EXCLUDED: C++ stores integer-index-named properties in the
    // object's INDEXED butterfly storage (contiguous/ArrayStorage), NOT the named
    // PropertyTable — they have no named offset (so the named-property IC never arms for
    // them). The write paths route an array-index key to the butterfly `elements` side for
    // EVERY object kind (`route_array_index_to_elements`), and the read/enumerate/delete
    // paths serve it from there, so its value still has a POD home after the flip.
    matches!(
        key,
        CorePropertyKey::Identifier(_) | CorePropertyKey::String(_) | CorePropertyKey::Symbol(_)
    ) && key_array_index(key).is_none()
}

/// C++ JSC runtime/PropertyAttribute.h: `PropertyAttribute::Accessor == 1 << 4` (also
/// runtime/PropertySlot.h:50). Set on a property's `unsigned attributes` when it holds
/// a getter/setter rather than a data value, so a data add and an accessor add of the
/// SAME key produce DISTINCT attribute bitfields -> DISTINCT transition edges ->
/// DISTINCT successor structures (without it they would wrongly share one structure).
pub(crate) const PROPERTY_ATTRIBUTE_ACCESSOR: u32 = 1 << 4;

/// Encode `CorePropertyAttributes` (+ accessor-ness) as the `unsigned attributes`
/// bitfield the ported Structure transition table + PropertyTable key on (C++
/// runtime/PropertyAttribute.h: ReadOnly == 1<<1, DontEnum == 1<<2, DontDelete == 1<<3,
/// Accessor == 1<<4). The writable/enumerable/configurable trio plus the Accessor bit
/// the interpreter models is encoded; the mapping is injective over those combinations
/// so distinct attribute sets produce distinct transition edges (the authoritative
/// attribute VALUES stay in `properties`). gc-r4 B-i threads `is_accessor` from the
/// `structure_add_property` call sites so an accessor add keys a DIFFERENT edge than a
/// data add of the same key. C++ never sets the ReadOnly (writable) bit on an accessor —
/// writability is a DATA-property attribute only — so it is suppressed when `is_accessor`,
/// leaving an accessor's default attributes as just the Accessor bit.
pub(crate) fn core_attributes_to_u32(attributes: CorePropertyAttributes, is_accessor: bool) -> u32 {
    let mut bits = 0u32;
    if !is_accessor && !attributes.writable {
        bits |= 1 << 1; // ReadOnly
    }
    if !attributes.enumerable {
        bits |= 1 << 2; // DontEnum
    }
    if !attributes.configurable {
        bits |= 1 << 3; // DontDelete
    }
    if is_accessor {
        bits |= PROPERTY_ATTRIBUTE_ACCESSOR;
    }
    bits
}

/// Decode the Structure's `unsigned attributes` bitfield back into the interpreter's
/// `CorePropertyAttributes` (the inverse of `core_attributes_to_u32`). The faithful
/// reader side of the gc-r4 B-iv flip: `get_own_property`/`own_property_from_shape`
/// reconstruct a `CoreProperty` from the SHAPE (Structure offset+attributes) + the
/// butterfly value, so the per-cell `properties` HashMap is no longer the attribute
/// authority. C++ stores the bits directly on the property slot
/// (runtime/PropertyAttribute.h: ReadOnly == 1<<1, DontEnum == 1<<2, DontDelete == 1<<3,
/// Accessor == 1<<4) and reads writable/enumerable/configurable off them; an accessor
/// never carries ReadOnly (writability is a data-only attribute), so `writable` is the
/// data-property predicate `!accessor && !ReadOnly`.
pub(crate) fn core_attributes_from_u32(bits: u32) -> CorePropertyAttributes {
    let is_accessor = bits & PROPERTY_ATTRIBUTE_ACCESSOR != 0;
    let read_only = bits & (1 << 1) != 0;
    let dont_enum = bits & (1 << 2) != 0;
    let dont_delete = bits & (1 << 3) != 0;
    CorePropertyAttributes {
        writable: !is_accessor && !read_only,
        enumerable: !dont_enum,
        configurable: !dont_delete,
    }
}

// C++ JSC PropertyOffset.h mirror. firstOutOfLineOffset == 64 is the boundary
// between inline storage (object header slots) and the Butterfly out-of-line
// region. INLINE_CAPACITY == 6 is the JSFinalObject default inline capacity
// (`JSObject::defaultInlineCapacity`, runtime/JSObject.h:1229 == (64 - 16)/8 == 6):
// the structure's PropertyTable assigns offsets 0..5 inline then jumps the 7th
// property to firstOutOfLineOffset == 64 (offsetForPropertyNumber, PropertyOffset.h:
// 136). offset_storage_index packs BOTH bands into one forward Vec this batch; the
// real inline-slot / Butterfly storage split is deferred to gc-r4 Batch 5.
pub(crate) const FIRST_OUT_OF_LINE_OFFSET: i32 = 64;
pub(crate) const INLINE_CAPACITY: i32 = 6;

const _: () = assert!(
    FIRST_OUT_OF_LINE_OFFSET == crate::object::STRUCTURE_FIRST_OUT_OF_LINE_OFFSET,
    "interpreter firstOutOfLineOffset must match the ported PropertyOffset.h constant"
);
const _: () = assert!(
    INLINE_CAPACITY < FIRST_OUT_OF_LINE_OFFSET,
    "JSObject.h:1230 static_assert(defaultInlineCapacity < firstOutOfLineOffset)"
);

/// C++ JSC PropertyOffset.h:87 isInlineOffset: offset < firstOutOfLineOffset.
/// With INLINE_CAPACITY == 6 the structure assigns offsets 0..5 in the inline band;
/// offset_storage_index maps them to forward indices 0..5 of out_of_line_storage.
/// Part of the PropertyOffset.h mirror; offset_storage_index keys on INLINE_CAPACITY
/// directly, so this predicate is kept for parity/readers but has no live caller yet.
#[allow(dead_code)]
pub(crate) fn is_inline_offset(offset: PropertyOffset) -> bool {
    offset.raw() >= 0 && offset.raw() < FIRST_OUT_OF_LINE_OFFSET
}

/// C++ JSC PropertyOffset.h isOutOfLineOffset.
///
/// Part of the PropertyOffset.h mirror; reactivated when INLINE_CAPACITY > 0 splits
/// the offset space into inline vs out-of-line bands. Unused in the INLINE_CAPACITY==0
/// first cut, which indexes out_of_line_storage by the raw offset directly.
#[allow(dead_code)]
pub(crate) fn is_out_of_line_offset(offset: PropertyOffset) -> bool {
    offset.raw() >= FIRST_OUT_OF_LINE_OFFSET
}

/// Index of an offset within `out_of_line_storage`.
///
/// C++ JSC splits the offset into an inline band `[0, inlineCapacity)` indexing the
/// object's inline slots (offsetInInlineStorage, PropertyOffset.h:99) and an
/// out-of-line band `[firstOutOfLineOffset, ...)` indexing the Butterfly at NEGATIVE
/// indices (offsetInOutOfLineStorage = -(offset - firstOutOfLineOffset) - 1,
/// PropertyOffset.h:106). DIVERGENCE (deferred to gc-r4 Batch 5): the Rust mirror
/// packs BOTH bands into one FORWARD-indexed Vec, so this returns a non-negative
/// contiguous slot index:
///   - inline offset n in [0, INLINE_CAPACITY)  -> index n
///   - out-of-line offset 64 + k                -> index INLINE_CAPACITY + k
/// so offsets 0,1,2,3,4,5,64,65,... map to indices 0,1,2,3,4,5,6,7,..., exactly the
/// allocation order. This must mirror Structure::PropertyTable's offsetForPropertyNumber
/// (the offset source) or a property would read a wrong slot.
pub(crate) fn offset_storage_index(offset: PropertyOffset) -> usize {
    let raw = offset.raw();
    debug_assert!(raw >= 0, "negative property offset has no slot");
    if raw < INLINE_CAPACITY {
        // Inline band: the slot index is the inline offset itself.
        raw as usize
    } else {
        debug_assert!(
            raw >= FIRST_OUT_OF_LINE_OFFSET,
            "offsets never fall in the (INLINE_CAPACITY, firstOutOfLineOffset) gap"
        );
        // Out-of-line band packed immediately after the inline slots.
        (INLINE_CAPACITY + (raw - FIRST_OUT_OF_LINE_OFFSET)) as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CoreProperty {
    pub(crate) kind: CorePropertyKind,
    pub(crate) attributes: CorePropertyAttributes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CorePropertyKind {
    Data(RuntimeValue),
    Accessor {
        getter: Option<RuntimeValue>,
        setter: Option<RuntimeValue>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CorePropertyAttributes {
    pub(crate) writable: bool,
    pub(crate) enumerable: bool,
    pub(crate) configurable: bool,
}

impl CorePropertyAttributes {
    pub(crate) const DATA_DEFAULT: Self = Self {
        writable: true,
        enumerable: true,
        configurable: true,
    };

    pub(crate) const ACCESSOR_DEFAULT: Self = Self {
        writable: false,
        enumerable: true,
        configurable: true,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CorePropertyGet {
    Missing,
    Data(RuntimeValue),
    Getter(RuntimeValue),
    AccessorWithoutGetter,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CorePropertyLookupSite {
    pub bytecode_index: Option<BytecodeIndex>,
    pub opcode: Option<CoreOpcode>,
    pub cache_key: Option<CacheKey>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CorePropertyStoreSite {
    pub bytecode_index: Option<BytecodeIndex>,
    pub opcode: Option<CoreOpcode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CorePropertyLookupClassification {
    OwnData,
    PrototypeData,
    OwnAccessorGetter,
    PrototypeAccessorGetter,
    AccessorWithoutGetter,
    Missing,
    IndexedOrTypedArray,
    OpaqueOrUncacheable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CorePropertyLookupChainEntry {
    pub object: RuntimeValue,
    pub structure: StructureId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CorePropertyLookupRecord {
    pub bytecode_index: Option<BytecodeIndex>,
    pub opcode: Option<CoreOpcode>,
    pub lookup_mode: PropertyLookupMode,
    pub base: RuntimeValue,
    pub base_object: Option<RuntimeValue>,
    pub base_structure: Option<StructureId>,
    pub base_normalization: PropertyLoadBaseNormalization,
    pub key: CorePropertyKey,
    pub cache_key: Option<CacheKey>,
    pub holder: Option<RuntimeValue>,
    pub offset: Option<PropertyOffset>,
    pub prototype_depth: usize,
    pub classification: CorePropertyLookupClassification,
    pub may_call_js: bool,
    pub cacheability: PropertyCacheability,
    pub returned_value: Option<RuntimeValue>,
    pub getter: Option<RuntimeValue>,
    pub access_case_kind: Option<AccessCaseKind>,
    pub chain: Vec<CorePropertyLookupChainEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CorePropertyStoreSnapshot {
    pub base_object: Option<RuntimeValue>,
    pub base_structure: Option<StructureId>,
    pub has_own_property: bool,
    pub has_own_data_property: bool,
    pub is_indexed_or_typed_array_store: bool,
    pub is_dense_array_indexed_store: bool,
    pub has_own_indexed_element: bool,
    pub offset: Option<PropertyOffset>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CorePropertyStoreRecord {
    pub bytecode_index: Option<BytecodeIndex>,
    pub opcode: Option<CoreOpcode>,
    pub base_object: Option<RuntimeValue>,
    pub base_structure_before: Option<StructureId>,
    pub base_structure_after: Option<StructureId>,
    pub key: CorePropertyKey,
    pub offset_after: Option<PropertyOffset>,
    pub stored_value: RuntimeValue,
    pub outcome: PropertyStoreObservationOutcome,
    pub may_call_js: bool,
    pub cacheability: PropertyCacheability,
    pub write_barrier_count: u32,
    pub last_write_barrier: Option<BarrierRequirementOutcome>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CoreArrayProfileObservationRecord {
    pub bytecode_index: BytecodeIndex,
    pub opcode: CoreOpcode,
    pub base_object: RuntimeValue,
    pub base_structure: StructureId,
    pub index: u32,
    pub profile: ArrayProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CoreCallObservationRecord {
    pub owner: CodeBlockId,
    pub frame: CallFrameId,
    pub bytecode_index: BytecodeIndex,
    pub opcode: CoreOpcode,
    pub destination: VirtualRegister,
    pub callee_register: VirtualRegister,
    pub callee_value: RuntimeValue,
    pub this_source: CallObservationThisSource,
    pub this_value: RuntimeValue,
    pub provided_argument_count: u32,
    pub target_kind: CallObservationTargetKind,
    pub outcome: CallObservationOutcome,
    pub may_call_js: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreCallObservationCapture<'a> {
    pub instruction: DispatchInstruction<'a>,
    pub destination: VirtualRegister,
    pub callee_register: VirtualRegister,
    pub callee_value: RuntimeValue,
    pub this_source: CallObservationThisSource,
    pub this_value: RuntimeValue,
    pub provided_argument_count: u32,
    pub target_kind: CallObservationTargetKind,
    pub may_call_js: bool,
}

impl CorePropertyLookupRecord {
    pub(crate) fn from_object_lookup(
        site: CorePropertyLookupSite,
        base: RuntimeValue,
        key: &CorePropertyKey,
        holder: Option<RuntimeValue>,
        prototype_depth: usize,
        classification: CorePropertyLookupClassification,
    ) -> Self {
        let (may_call_js, mut cacheability, mut access_case_kind) = match classification {
            CorePropertyLookupClassification::OwnData
            | CorePropertyLookupClassification::PrototypeData => (
                false,
                PropertyCacheability::Allowed,
                Some(AccessCaseKind::Load),
            ),
            CorePropertyLookupClassification::OwnAccessorGetter
            | CorePropertyLookupClassification::PrototypeAccessorGetter => (
                true,
                PropertyCacheability::Disallowed,
                Some(AccessCaseKind::Getter),
            ),
            CorePropertyLookupClassification::AccessorWithoutGetter => (
                false,
                PropertyCacheability::Disallowed,
                Some(AccessCaseKind::Getter),
            ),
            CorePropertyLookupClassification::Missing => (
                false,
                PropertyCacheability::Allowed,
                Some(AccessCaseKind::Miss),
            ),
            CorePropertyLookupClassification::IndexedOrTypedArray => (
                false,
                PropertyCacheability::Disallowed,
                Some(AccessCaseKind::IndexedLoad),
            ),
            CorePropertyLookupClassification::OpaqueOrUncacheable => {
                (true, PropertyCacheability::Disallowed, None)
            }
        };
        if classification == CorePropertyLookupClassification::Missing
            && key_array_index(key).is_some()
        {
            cacheability = PropertyCacheability::Disallowed;
            access_case_kind = None;
        }
        Self {
            bytecode_index: site.bytecode_index,
            opcode: site.opcode,
            lookup_mode: PropertyLookupMode::Get,
            base,
            base_object: Some(base),
            base_structure: None,
            base_normalization: PropertyLoadBaseNormalization::None,
            key: key.clone(),
            cache_key: site.cache_key,
            holder,
            offset: None,
            prototype_depth,
            classification,
            may_call_js,
            cacheability,
            returned_value: None,
            getter: None,
            access_case_kind,
            chain: Vec::new(),
        }
    }

    pub(crate) fn from_has_property_lookup(
        site: CorePropertyLookupSite,
        base: RuntimeValue,
        key: &CorePropertyKey,
        holder: Option<RuntimeValue>,
        prototype_depth: usize,
        classification: CorePropertyLookupClassification,
        result: bool,
    ) -> Self {
        let mut record =
            Self::from_object_lookup(site, base, key, holder, prototype_depth, classification);
        record.lookup_mode = PropertyLookupMode::HasProperty;
        record.returned_value = Some(RuntimeValue::from_bool(result));
        match classification {
            CorePropertyLookupClassification::OwnData
            | CorePropertyLookupClassification::PrototypeData
            | CorePropertyLookupClassification::OwnAccessorGetter
            | CorePropertyLookupClassification::PrototypeAccessorGetter
            | CorePropertyLookupClassification::AccessorWithoutGetter => {
                record.may_call_js = false;
                record.cacheability = PropertyCacheability::Allowed;
                record.access_case_kind = None;
            }
            CorePropertyLookupClassification::Missing => {
                record.may_call_js = false;
                record.access_case_kind = Some(AccessCaseKind::Miss);
            }
            CorePropertyLookupClassification::IndexedOrTypedArray => {
                record.may_call_js = false;
                record.cacheability = PropertyCacheability::Disallowed;
                record.access_case_kind = None;
            }
            CorePropertyLookupClassification::OpaqueOrUncacheable => {
                record.access_case_kind = None;
            }
        }
        record
    }

    pub(crate) fn opaque(
        site: CorePropertyLookupSite,
        base: RuntimeValue,
        base_object: Option<RuntimeValue>,
        key: &CorePropertyKey,
        may_call_js: bool,
        cacheability: PropertyCacheability,
    ) -> Self {
        Self {
            bytecode_index: site.bytecode_index,
            opcode: site.opcode,
            lookup_mode: PropertyLookupMode::Get,
            base,
            base_object,
            base_structure: None,
            base_normalization: PropertyLoadBaseNormalization::None,
            key: key.clone(),
            cache_key: site.cache_key,
            holder: None,
            offset: None,
            prototype_depth: 0,
            classification: CorePropertyLookupClassification::OpaqueOrUncacheable,
            may_call_js,
            cacheability,
            returned_value: None,
            getter: None,
            access_case_kind: None,
            chain: Vec::new(),
        }
    }

    pub(crate) fn opaque_has_property(
        site: CorePropertyLookupSite,
        base: RuntimeValue,
        base_object: Option<RuntimeValue>,
        key: &CorePropertyKey,
        may_call_js: bool,
        cacheability: PropertyCacheability,
        result: Option<bool>,
    ) -> Self {
        let mut record = Self::opaque(site, base, base_object, key, may_call_js, cacheability);
        record.lookup_mode = PropertyLookupMode::HasProperty;
        record.returned_value = result.map(RuntimeValue::from_bool);
        record
    }

    pub(crate) fn from_string_prototype_own_data(
        site: CorePropertyLookupSite,
        base: RuntimeValue,
        string_prototype: RuntimeValue,
        string_prototype_structure: StructureId,
        key: &CorePropertyKey,
        offset: Option<PropertyOffset>,
        returned_value: RuntimeValue,
    ) -> Self {
        Self {
            bytecode_index: site.bytecode_index,
            opcode: site.opcode,
            lookup_mode: PropertyLookupMode::Get,
            base,
            base_object: Some(string_prototype),
            base_structure: Some(string_prototype_structure),
            base_normalization: PropertyLoadBaseNormalization::StringPrototype,
            key: key.clone(),
            cache_key: site.cache_key,
            holder: Some(string_prototype),
            offset,
            prototype_depth: 0,
            classification: CorePropertyLookupClassification::OwnData,
            may_call_js: false,
            cacheability: PropertyCacheability::Allowed,
            returned_value: Some(returned_value),
            getter: None,
            access_case_kind: Some(AccessCaseKind::Load),
            chain: vec![CorePropertyLookupChainEntry {
                object: string_prototype,
                structure: string_prototype_structure,
            }],
        }
    }

    pub(crate) fn normalized_string_prototype_lookup(base: RuntimeValue, mut record: Self) -> Self {
        record.base = base;
        record.base_normalization = PropertyLoadBaseNormalization::StringPrototype;
        record
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CorePropertyPut {
    Stored,
    Setter(RuntimeValue),
    IgnoredGetterOnly,
    IgnoredReadOnly,
    /// `array.length = v` where `ToNumber(v) != ToUint32(v)` — C++ JSC
    /// `JSArray::put` throws a catchable `RangeError("Invalid array length")`
    /// (runtime/JSArray.cpp:321). The interpreter maps this to that throw.
    InvalidArrayLength,
}

/// Disposition of an `array.length = v` assignment, mirroring the C++ JSC
/// `JSArray::put` -> `setLength` path (runtime/JSArray.cpp:307-325, 1237).
enum ArrayLengthPut {
    /// `v` is a valid Uint32 length; the element vector was resized.
    Resized,
    /// `ToNumber(v) != ToUint32(v)` — RangeError("Invalid array length").
    Invalid,
    /// `v` needs the full ToNumber/ToPrimitive machinery (string/object/symbol/
    /// bigint) that lives in the interpreter, not the object store; fall through
    /// to the generic property put.
    NeedsGenericPut,
}

/// Result of a put on a primitive base, mirroring C++ JSC
/// `JSValue::putToPrimitive` (runtime/JSCJSValue.cpp:217). The primitive's
/// synthesized prototype chain is walked: a prototype accessor with a setter is
/// invoked with the primitive as receiver (`Setter`); otherwise the assignment
/// is a no-op (`NoOp`) — in sloppy mode `JSObject::definePropertyOnReceiver`
/// (JSObject.cpp:973) returns false silently because the receiver is not an
/// object, and a getter-only accessor or read-only data property on the chain
/// likewise yields a sloppy no-op. Strict-mode TypeError is deferred (see the
/// call site).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PutToPrimitiveOutcome {
    Setter(RuntimeValue),
    NoOp,
}

/// gc-r4 B1a: the store-level Butterfly slab API over `RuntimeValue`.
///
/// Each method keys a `ButterflyAllocation` (object/butterfly_handle.rs; the live
/// rep of C++ `Butterfly`, Butterfly.h:134-150) by its `ButterflyHandle` index
/// into the store-owned `butterflies` slab and delegates to the live rep. The
/// property methods map the structure-assigned `PropertyOffset` to a forward slot
/// via `offset_storage_index` exactly as `write_data_property_offset_slot` does,
/// so the later cutover is a faithful swap of the per-cell mirror onto this slab.
/// gc-r4 Butterfly-values cutover: these are now LIVE — every object cell carries a
/// `ButterflyHandle` (at the JSObject `m_butterfly` slot, offset 8) into this slab,
/// and all out-of-line property VALUE storage + indexed element storage is keyed
/// through these methods. The per-cell `out_of_line_storage`/`elements` Vecs are
/// gone; this slab is their store-owned home. (The named-property VALUE AUTHORITY is
/// still the per-cell `properties` HashMap this batch; the slab `props` side is its
/// offset-indexed mirror, as `out_of_line_storage` was — see the batch PAUSE note.)
impl CoreObjectStore {
    /// Allocate a fresh, empty butterfly; return its handle.
    ///
    /// C++ JSC `Heap::tryAllocateButterfly` / `Butterfly::create`
    /// (Butterfly.h:172-179) out of the GC Auxiliary subspace; the real arena
    /// allocation is deferred to R4. Here: push a default (empty)
    /// `ButterflyAllocation` and return its slab index.
    pub(crate) fn allocate_butterfly(&mut self) -> ButterflyHandle {
        let index = self.butterflies.len();
        self.butterflies.push(ButterflyAllocation::default());
        ButterflyHandle(index)
    }

    /// DEEP-copy an existing butterfly into a fresh slab entry; return the new
    /// handle. INDEPENDENT storage — never a shared handle.
    ///
    /// C++ JSC copies a butterfly's storage when materializing a CopyOnWrite
    /// region or reallocating (Butterfly.h:226-245, `createOrGrow*`). The Rust
    /// analog clones the `ButterflyAllocation` (both sides are `Copy`-element
    /// `Vec`s) into a new index so source and clone never alias.
    ///
    /// gc-r4 Butterfly-values: NOT used by the store-snapshot path —
    /// `CoreObjectStore::clone()` deep-clones the WHOLE `butterflies` slab alongside
    /// `objects`, so each cloned store already owns independent butterflies. This is
    /// the per-handle CoW/duplication primitive for the future case where a SINGLE
    /// cell is duplicated within one store (no such path exists yet), kept faithful to
    /// the B1a API surface.
    #[allow(dead_code)]
    pub(crate) fn clone_butterfly(&mut self, handle: ButterflyHandle) -> ButterflyHandle {
        let copy = self.butterflies[handle.0].clone();
        let index = self.butterflies.len();
        self.butterflies.push(copy);
        ButterflyHandle(index)
    }

    /// Store a RegExp's pattern string in the store-owned `regexp_sources` slab and
    /// return its handle.
    ///
    /// C++ JSC `RegExp::m_patternString` (runtime/RegExp.h:219) is the out-of-line
    /// pattern `String` set once at `RegExp::create`. The Rust analog (pre-R4) is a
    /// store-owned slab index, like `allocate_butterfly`; the raw arena allocation
    /// arrives at R4. Append-only (a RegExp pattern is immutable), so the index is
    /// stable for the slab's lifetime.
    pub(crate) fn allocate_regexp_source(&mut self, source: String) -> AuxiliaryHandle {
        let index = self.regexp_sources.len();
        self.regexp_sources.push(source);
        AuxiliaryHandle(index)
    }

    /// Borrow the RegExp pattern string behind `handle` (C++ `RegExp::pattern()`,
    /// runtime/RegExp.h:67).
    pub(crate) fn regexp_source_str(&self, handle: AuxiliaryHandle) -> &str {
        &self.regexp_sources[handle.0]
    }

    /// Read the property slot for `offset` from butterfly `handle` (C++
    /// `JSObject::getDirect`, JSObject.h:711). `None` for a negative offset.
    pub(crate) fn butterfly_prop_get(
        &self,
        handle: ButterflyHandle,
        offset: PropertyOffset,
    ) -> Option<RuntimeValue> {
        if offset.raw() < 0 {
            return None;
        }
        self.butterflies[handle.0].prop_get(offset_storage_index(offset))
    }

    /// Write `value` into the property slot for `offset` in butterfly `handle`,
    /// growing with `undefined` fill (C++ `JSObject::putDirectOffset`,
    /// JSObject.h:711; mirrors `write_data_property_offset_slot`). No-op for a
    /// negative offset.
    pub(crate) fn butterfly_prop_put(
        &mut self,
        handle: ButterflyHandle,
        offset: PropertyOffset,
        value: RuntimeValue,
    ) {
        if offset.raw() < 0 {
            return;
        }
        self.butterflies[handle.0].prop_put(offset_storage_index(offset), value);
    }

    /// Clear the property slot for `offset` in butterfly `handle` back to
    /// `undefined` (deletion / data->accessor). No-op for a negative offset.
    pub(crate) fn butterfly_prop_clear(&mut self, handle: ButterflyHandle, offset: PropertyOffset) {
        if offset.raw() < 0 {
            return;
        }
        self.butterflies[handle.0].prop_clear(offset_storage_index(offset));
    }

    /// Read the indexed element at `index` from butterfly `handle` (C++
    /// `Butterfly::contiguous()`, Butterfly.h:196). Hole/out-of-range -> `None`.
    pub(crate) fn butterfly_elem_get(
        &self,
        handle: ButterflyHandle,
        index: usize,
    ) -> Option<RuntimeValue> {
        self.butterflies[handle.0].elem_get(index)
    }

    /// Write `value` into the indexed element at `index` in butterfly `handle`,
    /// hole-filling growth (C++ `Butterfly::contiguous()` store, Butterfly.h:196).
    pub(crate) fn butterfly_elem_put(
        &mut self,
        handle: ButterflyHandle,
        index: usize,
        value: RuntimeValue,
    ) {
        self.butterflies[handle.0].elem_put(index, value);
    }

    /// Resize the indexed element side of butterfly `handle` to `len`
    /// (C++ butterfly vectorLength resize, Butterfly.h:187-189).
    pub(crate) fn butterfly_elem_resize(&mut self, handle: ButterflyHandle, len: usize) {
        self.butterflies[handle.0].elem_resize(len);
    }

    /// Number of indexed element slots in butterfly `handle` (the Butterfly
    /// vectorLength analog, Butterfly.h:187).
    pub(crate) fn butterfly_elem_len(&self, handle: ButterflyHandle) -> usize {
        self.butterflies[handle.0].elem_len()
    }

    /// Append `value` to the indexed element side of butterfly `handle`
    /// (C++ contiguous append, Butterfly.h:186-189).
    pub(crate) fn butterfly_elem_push(&mut self, handle: ButterflyHandle, value: RuntimeValue) {
        self.butterflies[handle.0].elem_push(value);
    }

    /// Clear the indexed element at `index` in butterfly `handle` to a hole
    /// (`delete arr[i]`; C++ indexed deleteProperty). No-op out of range.
    pub(crate) fn butterfly_elem_clear(&mut self, handle: ButterflyHandle, index: usize) {
        self.butterflies[handle.0].elem_clear(index);
    }

    /// Pop the last indexed element of butterfly `handle` (`Array.prototype.pop`
    /// fast path); flattens a trailing hole to `None`.
    pub(crate) fn butterfly_elem_pop(&mut self, handle: ButterflyHandle) -> Option<RuntimeValue> {
        self.butterflies[handle.0].elem_pop()
    }

    /// Borrow the indexed element side of butterfly `handle` as a slice (for
    /// enumeration / snapshot reads). C++ `Butterfly::contiguous()` span.
    pub(crate) fn butterfly_elements(&self, handle: ButterflyHandle) -> &[Option<RuntimeValue>] {
        self.butterflies[handle.0].elements_slice()
    }
}

// gc-r4 POD-ification (BoundFunction unit): the bound-args aux-backing API, the
// store-owned home of each bound function's [[BoundArguments]] value array. Mirrors the
// butterfly slab API above (allocate -> handle; index the slab through the handle), but
// the backing is allocated ONLY for bound functions (not every cell) — C++ JSBoundFunction
// is the only kind with `m_boundArgs` (runtime/JSBoundFunction.h:133).
impl CoreObjectStore {
    /// Push `args` into the store-owned bound-args slab and return its POD handle.
    ///
    /// C++ JSC JSBoundFunction::create allocates the out-of-line bound-arguments array
    /// (m_boundArgs, JSBoundFunction.h:133) from the GC Auxiliary subspace; the real
    /// arena allocation is deferred to R4. Here: push the value array and return its slab
    /// index. Mirrors `allocate_butterfly`.
    pub(crate) fn allocate_bound_args(&mut self, args: Vec<RuntimeValue>) -> AuxiliaryHandle {
        let index = self.bound_args_backings.len();
        self.bound_args_backings.push(args);
        AuxiliaryHandle(index)
    }

    /// Borrow the bound-args value array for `handle` (C++ JSBoundFunction
    /// boundArgs()/m_boundArgs read, JSBoundFunction.h:133). Caller is responsible for
    /// only passing a real handle assigned by `allocate_bound_args` (the `INVALID`
    /// sentinel never reaches here — `bound_function_data` checks `kind ==
    /// BoundFunction` first, and every such cell got a real handle at creation), exactly
    /// as the butterfly accessors assume `allocate_cell` assigned a real handle.
    pub(crate) fn bound_args_slice(&self, handle: AuxiliaryHandle) -> &[RuntimeValue] {
        &self.bound_args_backings[handle.0]
    }

    /// Push a closure's captured-variable value array into the store-owned
    /// `captures_backings` slab and return its POD handle.
    ///
    /// C++ JSC: a closure's captured variables live in a JSLexicalEnvironment reached
    /// through the scope chain (JSLexicalEnvironment.h:56-80, JSCallee::m_scope). gc-r4
    /// SD-2 accepts the aux-value-slab POD EXPEDIENT (the faithful scope-chain relocation
    /// is deferred); this mirrors `allocate_bound_args`. Called for EVERY function at
    /// creation (even an empty capture set) so a Function cell's handle is always real.
    pub(crate) fn allocate_captures(&mut self, captures: Vec<RuntimeValue>) -> AuxiliaryHandle {
        let index = self.captures_backings.len();
        self.captures_backings.push(captures);
        AuxiliaryHandle(index)
    }

    /// Borrow the captured-variable value array for `handle` (the closure's captures,
    /// read by `function_capture` / `function_call_target`). Every Function cell got a
    /// real handle at `allocate_function_with_construct_ability`, so the `INVALID`
    /// sentinel never reaches here, exactly as `bound_args_slice` assumes.
    pub(crate) fn captures_slice(&self, handle: AuxiliaryHandle) -> &[RuntimeValue] {
        &self.captures_backings[handle.0]
    }
}

// gc-r4 R4 POD-ification (ArrayBuffer unit): the byte-backing aux API, the store-owned
// home of each ArrayBuffer's raw bytes. Mirrors the bound-args/butterfly slab API
// (allocate -> handle; index the slab through the handle), but the backing is allocated
// ONLY for ArrayBuffer cells (not every cell) — C++ JSC `ArrayBufferContents::m_data`
// (runtime/ArrayBuffer.h:126) is the only such payload, a raw `void*` byte buffer. The
// bytes are NOT GC edges (raw integers), so unlike `bound_args_backings` no collector
// trace needs to visit them.
impl CoreObjectStore {
    /// Allocate a zero-filled byte backing of `byte_length` in the store-owned
    /// `array_buffer_backings` slab and return its POD handle.
    ///
    /// C++ JSC `ArrayBufferContents::tryAllocate` (ArrayBuffer.cpp) zero-initializes
    /// `m_data` of `sizeInBytes` (ArrayBuffer.h:126). The Rust analog (pre-R4) is a
    /// store-owned slab index, like `allocate_bound_args`; the raw arena allocation
    /// arrives at R4.
    pub(crate) fn allocate_array_buffer_backing(&mut self, byte_length: usize) -> AuxiliaryHandle {
        let index = self.array_buffer_backings.len();
        self.array_buffer_backings.push(vec![0u8; byte_length]);
        AuxiliaryHandle(index)
    }

    /// Borrow the byte backing behind `handle` (C++ `ArrayBuffer::data()`,
    /// ArrayBuffer.h:88 reading `m_contents.data()`). Caller passes a real handle
    /// assigned by `allocate_array_buffer_backing` (every reader checks `kind ==
    /// ArrayBuffer` first, so the `INVALID` sentinel never reaches here), exactly as
    /// the butterfly accessors assume `allocate_cell` assigned a real handle.
    pub(crate) fn array_buffer_bytes(&self, handle: AuxiliaryHandle) -> &[u8] {
        &self.array_buffer_backings[handle.0]
    }

    /// Mutably borrow the byte backing behind `handle` (typed-array/DataView in-place
    /// stores; C++ writes through the `m_data` pointer). No write barrier — raw bytes
    /// are not GC edges.
    pub(crate) fn array_buffer_bytes_mut(&mut self, handle: AuxiliaryHandle) -> &mut [u8] {
        &mut self.array_buffer_backings[handle.0]
    }
}

// gc-r4 R4 POD-ification (Map/Set unit): the ordered-storage aux-backing API — the
// store-owned home of each Map/WeakMap's insertion-ordered (key,value) entries and each
// Set/WeakSet's insertion-ordered values. Mirrors the bound-args slab API above
// (allocate -> handle; index the slab through the handle), but a backing is allocated
// ONLY for the four collection kinds. C++ JSC reaches these through `m_storage` (a
// `JSOrderedHashTable::Storage` JSCellButterfly, JSOrderedHashTable.h:164).
//
// POD expedient (NOT the faithful JSOrderedHashTable): these methods preserve EXACTLY
// the prior per-cell Vec semantics — insertion order, and linear keyed lookup done at
// the call site over a snapshot (the interpreter's SameValueZero / strict equality
// needs `&self` on the interpreter, so lookup stays there). Only the storage moved off
// the cell; the faithful ordered-hash port is a deferred batch (see the slab comment).
impl CoreObjectStore {
    /// Allocate a fresh empty map-entry-list slab slot and return its POD handle.
    /// Mirrors `allocate_bound_args`; called eagerly at `allocate_map`/`allocate_weak_map`.
    fn allocate_map_entries(&mut self) -> AuxiliaryHandle {
        let index = self.map_entry_lists.len();
        self.map_entry_lists.push(Vec::new());
        AuxiliaryHandle(index)
    }

    /// Allocate a fresh empty set-value-list slab slot and return its POD handle.
    /// Mirrors `allocate_bound_args`; called eagerly at `allocate_set`/`allocate_weak_set`.
    fn allocate_set_values(&mut self) -> AuxiliaryHandle {
        let index = self.set_value_lists.len();
        self.set_value_lists.push(Vec::new());
        AuxiliaryHandle(index)
    }

    /// Resolve `map`'s ordered-entry slab handle (`None` if not a live map-like cell or
    /// carrying the INVALID sentinel). Returns an owned `Copy` handle so the cell borrow
    /// is released before the caller indexes the slab.
    fn map_entries_handle(&self, map: RuntimeValue) -> Option<AuxiliaryHandle> {
        match self.find(map).map(|cell| cell.map_entries) {
            Some(handle) if handle != AuxiliaryHandle::INVALID => Some(handle),
            _ => None,
        }
    }

    /// Resolve `set`'s ordered-value slab handle (`None` if not a live set-like cell or
    /// carrying the INVALID sentinel).
    fn set_values_handle(&self, set: RuntimeValue) -> Option<AuxiliaryHandle> {
        match self.find(set).map(|cell| cell.set_values) {
            Some(handle) if handle != AuxiliaryHandle::INVALID => Some(handle),
            _ => None,
        }
    }

    /// Clone of `map`'s insertion-ordered entries (for linear keyed lookup at the call
    /// site and for forEach/iteration). Empty if `map` has no ordered backing.
    pub(crate) fn map_entries_snapshot(
        &self,
        map: RuntimeValue,
    ) -> Vec<(RuntimeValue, RuntimeValue)> {
        match self.map_entries_handle(map) {
            Some(handle) => self.map_entry_lists[handle.0].clone(),
            None => Vec::new(),
        }
    }

    /// Value of the entry at insertion index `index`, or `None`.
    pub(crate) fn map_entry_value(&self, map: RuntimeValue, index: usize) -> Option<RuntimeValue> {
        let handle = self.map_entries_handle(map)?;
        self.map_entry_lists[handle.0]
            .get(index)
            .map(|(_, value)| *value)
    }

    /// Number of entries (Map/WeakMap `size`).
    pub(crate) fn map_entries_len(&self, map: RuntimeValue) -> usize {
        match self.map_entries_handle(map) {
            Some(handle) => self.map_entry_lists[handle.0].len(),
            None => 0,
        }
    }

    /// Append `(key, value)` at the end (insertion order; a fresh-key add).
    pub(crate) fn map_entries_push(
        &mut self,
        map: RuntimeValue,
        key: RuntimeValue,
        value: RuntimeValue,
    ) {
        if let Some(handle) = self.map_entries_handle(map) {
            self.map_entry_lists[handle.0].push((key, value));
        }
    }

    /// Overwrite the value of the entry at `index` (an existing-key set; key unchanged).
    pub(crate) fn map_entry_set_value(
        &mut self,
        map: RuntimeValue,
        index: usize,
        value: RuntimeValue,
    ) {
        if let Some(handle) = self.map_entries_handle(map) {
            if let Some(entry) = self.map_entry_lists[handle.0].get_mut(index) {
                entry.1 = value;
            }
        }
    }

    /// Remove the entry at `index` (Map/WeakMap `delete`, preserving insertion order).
    pub(crate) fn map_entries_remove(&mut self, map: RuntimeValue, index: usize) {
        if let Some(handle) = self.map_entries_handle(map) {
            let list = &mut self.map_entry_lists[handle.0];
            if index < list.len() {
                list.remove(index);
            }
        }
    }

    /// Drop all entries (Map `clear`).
    pub(crate) fn map_entries_clear(&mut self, map: RuntimeValue) {
        if let Some(handle) = self.map_entries_handle(map) {
            self.map_entry_lists[handle.0].clear();
        }
    }

    /// Clone of `set`'s insertion-ordered values (for linear lookup / iteration).
    pub(crate) fn set_values_snapshot(&self, set: RuntimeValue) -> Vec<RuntimeValue> {
        match self.set_values_handle(set) {
            Some(handle) => self.set_value_lists[handle.0].clone(),
            None => Vec::new(),
        }
    }

    /// Number of values (Set/WeakSet `size`).
    pub(crate) fn set_values_len(&self, set: RuntimeValue) -> usize {
        match self.set_values_handle(set) {
            Some(handle) => self.set_value_lists[handle.0].len(),
            None => 0,
        }
    }

    /// Append `value` at the end (insertion order; a fresh-value add).
    pub(crate) fn set_values_push(&mut self, set: RuntimeValue, value: RuntimeValue) {
        if let Some(handle) = self.set_values_handle(set) {
            self.set_value_lists[handle.0].push(value);
        }
    }

    /// Remove the value at `index` (Set/WeakSet `delete`, preserving insertion order).
    pub(crate) fn set_values_remove(&mut self, set: RuntimeValue, index: usize) {
        if let Some(handle) = self.set_values_handle(set) {
            let list = &mut self.set_value_lists[handle.0];
            if index < list.len() {
                list.remove(index);
            }
        }
    }

    /// Drop all values (Set `clear`).
    pub(crate) fn set_values_clear(&mut self, set: RuntimeValue) {
        if let Some(handle) = self.set_values_handle(set) {
            self.set_value_lists[handle.0].clear();
        }
    }
}

// gc-r4 GAP A — the live `CoreObjectCell` trace (`visitChildren`).
//
// C++ source of truth: `JSObject::visitChildren(JSCell*, SlotVisitor&)`
// (runtime/JSObject.cpp): visit the object's inline value slots + its Structure/
// prototype, then `visitButterfly` (out-of-line property storage + contiguous
// indexed elements); each subclass (JSBoundFunction, JSPromise, JSMap/JSSet,
// GetterSetter, ...) appends its own out-of-line value edges. The Rust port keeps
// those per-kind value backings in store-owned auxiliary slabs reached by POD
// handles (gc-r4 SD-4), so the faithful trace is hosted on the STORE.

/// Live-path marking sink for a `CoreObjectCell`'s strong GC edges.
///
/// C++ analog: `SlotVisitor`, the consumer of the edges a cell's `visitChildren`
/// appends (`heap/SlotVisitor.h` `append`/`appendToMarkStack`). Each edge here is
/// the cell projection of a live `RuntimeValue` (`value/repr.rs CellValue`) — the
/// gc-r4 SD-1 / GAP-D value type. This trait deliberately does NOT reuse the
/// skeleton `gc::Tracer`, whose `visit_cell(GcRef<JsCell>)` is over the wrong
/// value-type path (`object/identity.rs` + `object/storage.rs JsValue`, retired in
/// the GAP-D reconciliation).
///
/// It lives in the interpreter layer, not `gc`: `gc` is value-type-agnostic by
/// design (`gc/mod.rs:3` — "no local dependencies"; its `Tracer` is over
/// `GcRef<JsCell>` for exactly this reason) and may not name a value type. At R4
/// the collector driver (which owns both the store and the heap) supplies an
/// adapter that implements this trait by decoding `CellValue::pointer_payload_bits`
/// to the arena cell address and forwarding to `gc`'s `Tracer::visit_cell`.
//
// gc-r4 GAP A is authored but UNWIRED: no live collection calls `trace_cell` yet
// (the marking RUN is R4-gated). Only the unit test exercises it today, so the
// non-test build sees these as dead — `#[allow(dead_code)]` until the R4 collector
// driver wires them, mirroring the other unwired R4 foundation (e.g. the
// `butterflies` slab / `clone_butterfly`).
#[allow(dead_code)]
pub(crate) trait CellEdgeVisitor {
    /// Append one strong cell edge. Immediates never reach here — the trace
    /// filters them with `RuntimeValue::as_cell` first (see `trace_value_edge`).
    fn visit_cell_edge(&mut self, cell: CellValue);
}

/// Append one value-slot edge to the visitor, skipping non-cell immediates.
///
/// C++ analog: `SlotVisitor::appendUnbarriered(JSValue)` — a number/bool/
/// undefined/null/empty value is not a heap cell and is not a GC edge, so only
/// `value.asCell()` is appended (gc-r4 GAP A: filter with `RuntimeValue::as_cell`,
/// the SD-1 live value type). Centralizing the filter here keeps every edge site
/// (inline slot, butterfly, aux slab) uniform.
#[allow(dead_code)] // gc-r4 GAP A authored-but-unwired (R4-gated; see CellEdgeVisitor).
fn trace_value_edge(value: RuntimeValue, visitor: &mut dyn CellEdgeVisitor) {
    if let Some(cell) = value.as_cell() {
        visitor.visit_cell_edge(cell);
    }
}

impl CoreObjectStore {
    /// Visit every GC edge of `cell` (gc-r4 GAP A; the faithful analog of
    /// `JSObject::visitChildren`). The edges are `RuntimeValue`s (SD-1); their
    /// cell projections are appended via `visitor`. The trace is total and
    /// read-only: it never dereferences an edge and never mutates the store.
    ///
    /// DESIGN POINT (gc-r4 GAP A): `JSObject::visitChildren` reaches the butterfly
    /// and per-kind out-of-line state through the cell's OWN pointers. Pre-R4 those
    /// live in store-owned slabs reached by POD handles, so the trace needs the
    /// store (`&self`) to resolve `handle -> slab slot -> values`. Hosting it as a
    /// store method `(cell, visitor)` mirrors the C++ static `(cell, visitor)`
    /// shape, with the store standing in as the out-of-line-storage owner the raw
    /// cell pointer becomes at R4. No new shared-ownership model: `&self` and
    /// `&CoreObjectCell` are both shared borrows.
    #[allow(dead_code)] // gc-r4 GAP A authored-but-unwired (R4-gated; see CellEdgeVisitor).
    pub(crate) fn trace_cell(&self, cell: &CoreObjectCell, visitor: &mut dyn CellEdgeVisitor) {
        // ---- inline RuntimeValue header slots (C++ JSObject inline value slots
        // + the prototype edge, which C++ visits via Structure::m_prototype; the
        // port stores `prototype` on the cell). `Option::None` == an absent slot
        // (no edge). Order is immaterial to the mark set.
        let inline_optional = [
            cell.prototype,
            cell.super_base,
            cell.super_constructor,
            cell.native_bound_promise,
            cell.native_bound_proxy,
            cell.primitive_value,
            cell.view_buffer,
            cell.proxy_target,
            cell.proxy_handler,
            cell.bound_target,
            // GetterSetter::m_getter / m_setter (runtime/GetterSetter.h:132-133).
            cell.getter_value,
            cell.setter_value,
        ];
        for value in inline_optional.into_iter().flatten() {
            trace_value_edge(value, visitor);
        }
        // Non-`Option` inline slots: a default cell carries the Empty sentinel
        // here, which `as_cell` rejects, so an unset slot is naturally skipped.
        trace_value_edge(cell.binding_value, visitor);
        trace_value_edge(cell.promise_result, visitor);
        trace_value_edge(cell.bound_this, visitor);

        // ---- butterfly: out-of-line property storage + contiguous indexed
        // elements (C++ JSObject::visitButterfly). A null/INVALID butterfly is
        // skipped, exactly as C++ null-checks `m_butterfly`; `.get` also makes a
        // stale index a no-op so the trace stays total.
        if cell.butterfly != ButterflyHandle::INVALID {
            if let Some(butterfly) = self.butterflies.get(cell.butterfly.0) {
                for &value in &butterfly.props {
                    trace_value_edge(value, visitor);
                }
                // `None` is a hole (no element), not an edge.
                for value in butterfly.elements.iter().copied().flatten() {
                    trace_value_edge(value, visitor);
                }
            }
        }

        // ---- per-kind out-of-line value backings (store-owned aux slabs). Each
        // holds `RuntimeValue` GC edges relocated off the cell (gc-r4 SD-4); the
        // trace resolves the cell's POD handle against its slab. An `INVALID`
        // handle (a cell of another kind) carries no such slab and is skipped.

        // JSBoundFunction::m_boundArgs / [[BoundArguments]] (JSBoundFunction.h:133).
        if cell.bound_args != AuxiliaryHandle::INVALID {
            if let Some(args) = self.bound_args_backings.get(cell.bound_args.0) {
                for &value in args {
                    trace_value_edge(value, visitor);
                }
            }
        }
        // Closure captured-variable values (faithfully a JSLexicalEnvironment's
        // variables; SD-2 aux-slab expedient).
        if cell.captures != AuxiliaryHandle::INVALID {
            if let Some(values) = self.captures_backings.get(cell.captures.0) {
                for &value in values {
                    trace_value_edge(value, visitor);
                }
            }
        }
        // Class instance-field initializers ([[Fields]]); the interned `key_uid`
        // (an `AtomId`) is not a GC edge — only each `initializer` is.
        if cell.instance_fields != AuxiliaryHandle::INVALID {
            if let Some(fields) = self.instance_field_lists.get(cell.instance_fields.0) {
                for value in fields.iter().filter_map(|field| field.initializer) {
                    trace_value_edge(value, visitor);
                }
            }
        }
        // Map/WeakMap insertion-ordered entries: visit BOTH the key and the value.
        if cell.map_entries != AuxiliaryHandle::INVALID {
            if let Some(entries) = self.map_entry_lists.get(cell.map_entries.0) {
                for &(key, value) in entries {
                    trace_value_edge(key, visitor);
                    trace_value_edge(value, visitor);
                }
            }
        }
        // Set/WeakSet insertion-ordered values.
        if cell.set_values != AuxiliaryHandle::INVALID {
            if let Some(values) = self.set_value_lists.get(cell.set_values.0) {
                for &value in values {
                    trace_value_edge(value, visitor);
                }
            }
        }
        // Pending JSPromise reaction records (JSPromise.h:35). Each record's
        // `result_promise`/`on_fulfilled`/`on_rejected` are GC edges (the same
        // three the store's write-barrier path already barriers); `kind` is not.
        if cell.promise_reactions != PromiseReactionsHandle::INVALID {
            if let Some(reactions) = self.promise_reaction_lists.get(cell.promise_reactions.0) {
                for reaction in reactions {
                    trace_value_edge(reaction.result_promise, visitor);
                    trace_value_edge(reaction.on_fulfilled, visitor);
                    trace_value_edge(reaction.on_rejected, visitor);
                }
            }
        }

        // ---- DELIBERATELY NOT VISITED (not GC edges):
        // - `regexp_source` -> `regexp_sources` slab: a pattern `String` (text),
        //   not a cell pointer. C++ `RegExp::m_patternString` is a `StringImpl`
        //   swept by its own subspace, never an outgoing edge from the RegExp.
        // - `array_buffer_data` -> `array_buffer_backings` slab: raw `Vec<u8>`
        //   bytes, not `RuntimeValue` edges (C++ `ArrayBufferContents::m_data` is
        //   a `void*`).
        // Also not RuntimeValue edges, so out of scope here:
        // - `structure_id`: a `StructureIdTable` handle, not a live `RuntimeValue`
        //   cell. C++ visits the Structure cell; the port's Structure lives in the
        //   `structure_table` registry (not yet a heap cell), so it is not a
        //   RuntimeValue edge — a known divergence to revisit when Structures
        //   become real cells.
        // - `date_value` / `view_*` scalars, `function_index`, `native_function`,
        //   `regexp_flags`, `promise_state` and the other POD tags.
    }
}

impl CoreObjectStore {
    pub(crate) fn allocate(&mut self) -> RuntimeValue {
        let prototype = self.ensure_object_prototype();
        self.allocate_with_prototype(Some(prototype))
    }

    pub(crate) fn allocate_with_prototype(
        &mut self,
        prototype: Option<RuntimeValue>,
    ) -> RuntimeValue {
        self.allocate_cell(CoreObjectCell {
            prototype,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_with_prototype_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        prototype: Option<RuntimeValue>,
    ) -> Result<RuntimeValue, ExecutionError> {
        let object = self.allocate_cell(CoreObjectCell::default());
        self.set_prototype_or_null_with_write_barrier(heap, object, prototype)?;
        Ok(object)
    }

    pub(crate) fn allocate_array(&mut self) -> RuntimeValue {
        let prototype = self.ensure_array_prototype();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Array,
            prototype: Some(prototype),
            ..CoreObjectCell::default()
        })
    }

    #[cfg(test)]
    pub(crate) fn allocate_function(
        &mut self,
        function_index: u32,
        captures: Vec<RuntimeValue>,
        prototype_property_key: Option<CorePropertyKey>,
    ) -> RuntimeValue {
        self.allocate_function_with_construct_ability(
            function_index,
            captures,
            prototype_property_key,
            ConstructAbility::CanConstruct,
        )
    }

    pub(crate) fn allocate_function_with_construct_ability(
        &mut self,
        function_index: u32,
        captures: Vec<RuntimeValue>,
        prototype_property_key: Option<CorePropertyKey>,
        construct_ability: ConstructAbility,
    ) -> RuntimeValue {
        let function_prototype = self.ensure_function_prototype();
        // gc-r4 R4 POD-ification (captures unit): relocate the captured-variable values out
        // of the cell into the store-owned `captures_backings` slab; the cell carries only
        // the POD `AuxiliaryHandle`. Done for every function (even an empty set), mirroring
        // `allocate_bound_function`'s `allocate_bound_args`, so the read sites always see a
        // real handle.
        let captures = self.allocate_captures(captures);
        let function = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Function,
            prototype: Some(function_prototype),
            function_index: Some(function_index),
            captures,
            construct_ability,
            ..CoreObjectCell::default()
        });
        if let Some(key) = prototype_property_key {
            // gc-r4 B-iv: a function is born EMPTY then installs its own `.prototype`
            // through the normal define path (the per-cell `properties` initial-property
            // channel is gone), so the initial shape == the runtime shape and same-shape
            // siblings converge under one add-property transition. C++ JSFunction installs
            // `prototype` writable, DontEnum | DontDelete.
            let prototype = self.allocate();
            let _ = self.define_data_property(
                function,
                &key,
                prototype,
                CorePropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: false,
                },
            );
            self.install_prototype_constructor(prototype, function);
        }
        function
    }

    pub(crate) fn allocate_function_with_construct_ability_and_write_barrier(
        &mut self,
        heap: &mut Heap,
        function_index: u32,
        captures: Vec<RuntimeValue>,
        prototype_property_key: Option<CorePropertyKey>,
        construct_ability: ConstructAbility,
    ) -> Result<RuntimeValue, ExecutionError> {
        let function = self.allocate_function_with_construct_ability(
            function_index,
            captures.clone(),
            prototype_property_key.clone(),
            construct_ability,
        );
        for capture in captures {
            self.apply_value_store_write_barrier(heap, function, capture)?;
        }
        if let Some(key) = prototype_property_key {
            if let Some(prototype) = self.constructor_instance_prototype(function, &key) {
                self.apply_value_store_write_barrier(heap, function, prototype)?;
                self.apply_value_store_write_barrier(heap, prototype, function)?;
            }
        }
        Ok(function)
    }

    pub(crate) fn allocate_native_function(
        &mut self,
        native_function: CoreNativeFunction,
    ) -> RuntimeValue {
        let prototype = self.ensure_function_prototype();
        self.allocate_native_function_with_prototype(native_function, Some(prototype))
    }

    pub(crate) fn allocate_native_function_with_prototype(
        &mut self,
        native_function: CoreNativeFunction,
        prototype: Option<RuntimeValue>,
    ) -> RuntimeValue {
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::NativeFunction,
            prototype,
            native_function: Some(native_function),
            construct_ability: native_function.construct_ability(),
            ..CoreObjectCell::default()
        })
    }

    /// C++ JSC JSBoundFunction::create (runtime/JSBoundFunction.cpp): allocate a
    /// bound function whose prototype is Function.prototype, capturing the
    /// target callable, bound `this`, and bound leading arguments.
    pub(crate) fn allocate_bound_function(
        &mut self,
        target: RuntimeValue,
        bound_this: RuntimeValue,
        bound_args: Vec<RuntimeValue>,
    ) -> RuntimeValue {
        let function_prototype = self.ensure_function_prototype();
        // gc-r4 POD-ification: relocate the [[BoundArguments]] value array out of the cell
        // into the store-owned slab; the cell carries only the POD handle (m_boundArgs).
        let bound_args = self.allocate_bound_args(bound_args);
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::BoundFunction,
            prototype: Some(function_prototype),
            bound_target: Some(target),
            bound_this,
            bound_args,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_object_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::ObjectConstructor);
        let prototype = self.ensure_object_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        for (name, native_function) in [
            ("assign", CoreNativeFunction::Assign),
            ("create", CoreNativeFunction::Create),
            ("defineProperty", CoreNativeFunction::DefineProperty),
            ("entries", CoreNativeFunction::Entries),
            (
                "getOwnPropertyDescriptor",
                CoreNativeFunction::GetOwnPropertyDescriptor,
            ),
            ("getPrototypeOf", CoreNativeFunction::GetPrototypeOf),
            ("hasOwn", CoreNativeFunction::HasOwn),
            ("keys", CoreNativeFunction::Keys),
            ("setPrototypeOf", CoreNativeFunction::SetPrototypeOf),
            ("values", CoreNativeFunction::Values),
        ] {
            let function = self.allocate_native_function(native_function);
            let key = CorePropertyKey::String(name.into());
            let _ = self.define_data_property(
                constructor,
                &key,
                function,
                CorePropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
        Ok(constructor)
    }

    pub(crate) fn allocate_array_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::ArrayConstructor);
        let prototype = self.ensure_array_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        for (name, native_function) in [
            ("from", CoreNativeFunction::ArrayFrom),
            ("isArray", CoreNativeFunction::ArrayIsArray),
            ("of", CoreNativeFunction::ArrayOf),
        ] {
            self.install_native_method(constructor, name, native_function);
        }
        Ok(constructor)
    }

    // C++ JSC JSGlobalObject::init wires the Function constructor at
    // runtime/JSGlobalObject.cpp: FunctionConstructor::create uses
    // m_functionPrototype as its prototype, `Function.prototype` is the shared
    // function prototype, and `m_functionPrototype->constructor` is the
    // constructor (DontEnum). The global name `Function` is bound to it.
    // We reuse the existing `ensure_function_prototype()` object rather than
    // creating a new prototype, matching that the same Function.prototype is
    // shared by every function.
    //
    // CALLING `Function(...)` IS supported: the native arm assembles a function
    // program and defers compilation to the Vm via
    // `DispatchOutcome::CompileFunctionRequest` (see `native_function_constructor`).
    // CONSTRUCT (`new Function(...)`) is NOT yet wired: the op_construct native
    // path runs synchronously with no deferred-completion mechanism (the same
    // construct-side deferral the eval infra also lacks -- there is no `new eval`),
    // so this constructor stays `CannotConstruct` (see construct_ability) and
    // `new Function(...)` raises a catchable "not a constructor" TypeError. C++ JSC
    // makes Function constructible; wiring construct requires threading a
    // deferred completion through the native-construct dispatch.
    pub(crate) fn allocate_function_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let prototype = self.ensure_function_prototype();
        let constructor = self.allocate_native_function(CoreNativeFunction::FunctionConstructor);
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_math_object(&mut self) -> RuntimeValue {
        // Math is intentionally an ordinary intrinsic object, not a
        // constructor. Static functions and constants are installed as own data
        // properties so source-level property access uses the same path as
        // Object and Array intrinsics.
        let object = self.allocate();
        for (name, native_function) in [
            ("abs", CoreNativeFunction::MathAbs),
            ("floor", CoreNativeFunction::MathFloor),
            ("log", CoreNativeFunction::MathLog),
            ("max", CoreNativeFunction::MathMax),
            ("min", CoreNativeFunction::MathMin),
            ("pow", CoreNativeFunction::MathPow),
            ("random", CoreNativeFunction::MathRandom),
            ("sqrt", CoreNativeFunction::MathSqrt),
            ("trunc", CoreNativeFunction::MathTrunc),
            ("ceil", CoreNativeFunction::MathCeil),
            ("round", CoreNativeFunction::MathRound),
            ("sign", CoreNativeFunction::MathSign),
            ("exp", CoreNativeFunction::MathExp),
            ("cbrt", CoreNativeFunction::MathCbrt),
            ("log2", CoreNativeFunction::MathLog2),
            ("log10", CoreNativeFunction::MathLog10),
            ("sin", CoreNativeFunction::MathSin),
            ("cos", CoreNativeFunction::MathCos),
            ("tan", CoreNativeFunction::MathTan),
            ("asin", CoreNativeFunction::MathAsin),
            ("acos", CoreNativeFunction::MathAcos),
            ("atan", CoreNativeFunction::MathAtan),
            ("atan2", CoreNativeFunction::MathAtan2),
            ("sinh", CoreNativeFunction::MathSinh),
            ("cosh", CoreNativeFunction::MathCosh),
            ("tanh", CoreNativeFunction::MathTanh),
            ("asinh", CoreNativeFunction::MathAsinh),
            ("acosh", CoreNativeFunction::MathAcosh),
            ("atanh", CoreNativeFunction::MathAtanh),
            ("expm1", CoreNativeFunction::MathExpm1),
            ("log1p", CoreNativeFunction::MathLog1p),
            ("hypot", CoreNativeFunction::MathHypot),
        ] {
            let function = self.allocate_native_function(native_function);
            let key = CorePropertyKey::String(name.into());
            let _ = self.define_data_property(
                object,
                &key,
                function,
                CorePropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
        // C++ JSC MathObject::finishCreation (runtime/MathObject.cpp:83-90)
        // installs eight constants, each DontDelete | DontEnum | ReadOnly, in this
        // order. JSC computes them via libm at startup (e.g. Math::log(10.0)); the
        // port uses Rust's correctly-rounded std::f64::consts equivalents, which
        // represent the same mathematical constants (any difference is sub-ULP and
        // unobservable). FRAC_1_SQRT_2 == sqrt(0.5) and SQRT_2 == sqrt(2.0).
        for (name, value) in [
            ("E", RuntimeValue::from_double(std::f64::consts::E)),
            ("LN2", RuntimeValue::from_double(std::f64::consts::LN_2)),
            ("LN10", RuntimeValue::from_double(std::f64::consts::LN_10)),
            ("LOG2E", RuntimeValue::from_double(std::f64::consts::LOG2_E)),
            (
                "LOG10E",
                RuntimeValue::from_double(std::f64::consts::LOG10_E),
            ),
            ("PI", RuntimeValue::from_double(std::f64::consts::PI)),
            (
                "SQRT1_2",
                RuntimeValue::from_double(std::f64::consts::FRAC_1_SQRT_2),
            ),
            ("SQRT2", RuntimeValue::from_double(std::f64::consts::SQRT_2)),
        ] {
            let key = CorePropertyKey::String(name.into());
            let _ = self.define_data_property(
                object,
                &key,
                value,
                CorePropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                },
            );
        }
        object
    }

    pub(crate) fn allocate_json_object(&mut self) -> RuntimeValue {
        // JSON is an ordinary intrinsic object in this executable slice, not a
        // constructor. The native functions intentionally cover the finite
        // tree-shaped subset that the Rust VM can allocate today.
        let object = self.allocate();
        for (name, native_function) in [
            ("parse", CoreNativeFunction::JsonParse),
            ("stringify", CoreNativeFunction::JsonStringify),
        ] {
            let function = self.allocate_native_function(native_function);
            let key = CorePropertyKey::String(name.into());
            let _ = self.define_data_property(
                object,
                &key,
                function,
                CorePropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
        object
    }

    pub(crate) fn allocate_reflect_object(&mut self) -> RuntimeValue {
        let object = self.allocate();
        for (name, native_function) in [
            ("apply", CoreNativeFunction::ReflectApply),
            ("deleteProperty", CoreNativeFunction::ReflectDeleteProperty),
            ("get", CoreNativeFunction::ReflectGet),
            (
                "getOwnPropertyDescriptor",
                CoreNativeFunction::ReflectGetOwnPropertyDescriptor,
            ),
            ("getPrototypeOf", CoreNativeFunction::ReflectGetPrototypeOf),
            ("has", CoreNativeFunction::ReflectHas),
            ("ownKeys", CoreNativeFunction::ReflectOwnKeys),
            ("set", CoreNativeFunction::ReflectSet),
            ("setPrototypeOf", CoreNativeFunction::ReflectSetPrototypeOf),
        ] {
            self.install_native_method(object, name, native_function);
        }
        object
    }

    pub(crate) fn allocate_proxy_constructor(&mut self) -> RuntimeValue {
        let constructor = self.allocate_native_function(CoreNativeFunction::ProxyConstructor);
        self.install_native_method(constructor, "revocable", CoreNativeFunction::ProxyRevocable);
        constructor
    }

    pub(crate) fn allocate_string_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::StringConstructor);
        let prototype = self.ensure_string_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        self.install_native_method(
            constructor,
            "fromCharCode",
            CoreNativeFunction::StringFromCharCode,
        );
        Ok(constructor)
    }

    pub(crate) fn allocate_number_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::NumberConstructor);
        let prototype = self.ensure_number_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        // C++ JSC NumberConstructor::finishCreation (runtime/NumberConstructor.cpp:80-88)
        // installs the eight numeric constants directly on the Number constructor,
        // each with DontDelete | DontEnum | ReadOnly (writable:false,
        // enumerable:false, configurable:false), in this exact order. Without them
        // Number.MIN_VALUE is undefined, so box2d's
        // `b2Assert(1 - m.t0 > Number.MIN_VALUE)` compares against undefined->NaN
        // and throws (Octane/box2d.js:110,157). Each value matches the C++
        // jsDoubleNumber argument exactly; none is an int32, so from_double's
        // strict-int32 canonicalization never fires (all stay doubles like
        // jsDoubleNumber).
        for (name, value) in [
            ("EPSILON", f64::EPSILON),
            ("MAX_VALUE", f64::MAX),
            // C++ literal 5E-324 rounds to the smallest positive subnormal double
            // (== f64::from_bits(1)); the Rust literal rounds to the same value.
            ("MIN_VALUE", 5e-324),
            ("MAX_SAFE_INTEGER", 9007199254740991.0),
            ("MIN_SAFE_INTEGER", -9007199254740991.0),
            ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
            ("POSITIVE_INFINITY", f64::INFINITY),
            ("NaN", f64::NAN),
        ] {
            let key = CorePropertyKey::String(name.into());
            let _ = self.define_data_property(
                constructor,
                &key,
                RuntimeValue::from_double(value),
                CorePropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                },
            );
        }
        // C++ JSC NumberConstructor::finishCreation (NumberConstructor.cpp:89-90)
        // installs Number.parseInt / Number.parseFloat as DontEnum, reusing the
        // realm's existing parseInt/parseFloat function objects
        // (realm()->parseIntFunction()). The port has no stored handle to those
        // objects here, so it installs fresh ParseInt/ParseFloat natives with
        // identical behavior; the only divergence is object identity
        // (Number.parseInt === parseInt is false), which no Octane bench observes.
        // FOLLOW-UP: reuse the realm's parseInt/parseFloat objects once exposed.
        self.install_native_method(constructor, "parseInt", CoreNativeFunction::ParseInt);
        self.install_native_method(constructor, "parseFloat", CoreNativeFunction::ParseFloat);
        // FOLLOW-UP (out of scope here): Number.isFinite/isNaN/isInteger/
        // isSafeInteger (NumberConstructor.cpp:92 + NumberConstructor.lut.h) need
        // NEW non-coercing natives. They are NOT the global isFinite/isNaN
        // (CoreNativeFunction::GlobalIsFinite/GlobalIsNaN), which ToNumber-coerce
        // their argument; the Number.* forms do not coerce, so reusing the global
        // natives would be a behavior divergence. Box2d needs only the constants.
        Ok(constructor)
    }

    pub(crate) fn allocate_boolean_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::BooleanConstructor);
        let prototype = self.ensure_boolean_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_error_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::ErrorConstructor);
        let prototype = self.ensure_error_prototype(name_value, message_value);
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_type_error_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        error_name_value: RuntimeValue,
        type_error_name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::TypeErrorConstructor);
        let prototype = self.ensure_type_error_prototype(
            error_name_value,
            type_error_name_value,
            message_value,
        );
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_reference_error_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        error_name_value: RuntimeValue,
        reference_error_name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor =
            self.allocate_native_function(CoreNativeFunction::ReferenceErrorConstructor);
        let prototype = self.ensure_reference_error_prototype(
            error_name_value,
            reference_error_name_value,
            message_value,
        );
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_map_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::MapConstructor);
        let prototype = self.ensure_map_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_set_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::SetConstructor);
        let prototype = self.ensure_set_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_weak_map_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::WeakMapConstructor);
        let prototype = self.ensure_weak_map_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_weak_set_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::WeakSetConstructor);
        let prototype = self.ensure_weak_set_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_regexp_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::RegExpConstructor);
        let prototype = self.ensure_regexp_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_promise_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::PromiseConstructor);
        let prototype = self.ensure_promise_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        for (name, native_function) in [
            ("resolve", CoreNativeFunction::PromiseResolve),
            ("reject", CoreNativeFunction::PromiseReject),
        ] {
            self.install_native_method(constructor, name, native_function);
        }
        Ok(constructor)
    }

    pub(crate) fn allocate_date_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::DateConstructor);
        let prototype = self.ensure_date_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        for (name, native_function) in [
            ("now", CoreNativeFunction::DateNow),
            ("parse", CoreNativeFunction::DateParse),
            ("UTC", CoreNativeFunction::DateUtc),
        ] {
            self.install_native_method(constructor, name, native_function);
        }
        Ok(constructor)
    }

    pub(crate) fn allocate_bigint_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::BigIntConstructor);
        let prototype = self.ensure_bigint_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_array_buffer_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::ArrayBufferConstructor);
        let prototype = self.ensure_array_buffer_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    /// Allocate the constructor for a typed-array element kind, mirroring C++ each
    /// JSGenericTypedArrayView<Adaptor> constructor: a native function whose
    /// `prototype` is the kind's view prototype and whose prototype's
    /// `constructor` points back. (BYTES_PER_ELEMENT/length=3 own properties are
    /// not modeled here; the existing Uint8Array constructor does not install them
    /// either, so this stays a faithful mirror of the current Rust skeleton.)
    pub(crate) fn allocate_typed_array_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        kind: TypedArrayElementKind,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor =
            self.allocate_native_function(typed_array_constructor_native_function(kind));
        let prototype = self.ensure_typed_array_prototype(kind);
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_data_view_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::DataViewConstructor);
        let prototype = self.ensure_data_view_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        Ok(constructor)
    }

    pub(crate) fn allocate_symbol_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        iterator_symbol: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let constructor = self.allocate_native_function(CoreNativeFunction::SymbolConstructor);
        let prototype = self.ensure_symbol_prototype();
        self.install_constructor_prototype(constructor, prototype);
        self.install_prototype_constructor_with_write_barrier(heap, prototype, constructor)?;
        for (name, native_function) in [
            ("for", CoreNativeFunction::SymbolFor),
            ("keyFor", CoreNativeFunction::SymbolKeyFor),
        ] {
            self.install_native_method(constructor, name, native_function);
        }
        let _ = self.define_data_property(
            constructor,
            &CorePropertyKey::String("iterator".into()),
            iterator_symbol,
            CorePropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
            },
        );
        Ok(constructor)
    }

    pub(crate) fn install_standard_global_properties(
        &mut self,
        heap: &mut Heap,
        strings: &mut CoreStringStore,
        symbols: &mut CoreSymbolStore,
        global_object: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let standard_attributes = CorePropertyAttributes {
            writable: true,
            enumerable: false,
            configurable: true,
        };
        let object = self.allocate_object_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Object",
            object,
            standard_attributes,
        )?;
        let array = self.allocate_array_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Array",
            array,
            standard_attributes,
        )?;
        // C++ JSC JSGlobalObject::init binds the global `Function` to the
        // Function constructor (runtime/JSGlobalObject.cpp:
        // putDirectWithoutTransition(vm.propertyNames->Function, ...,
        // DontEnum)). standard_attributes matches DontEnum (writable,
        // configurable, not enumerable).
        let function = self.allocate_function_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Function",
            function,
            standard_attributes,
        )?;
        let math = self.allocate_math_object();
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Math",
            math,
            standard_attributes,
        )?;
        let json = self.allocate_json_object();
        self.install_standard_global_data_property(
            heap,
            global_object,
            "JSON",
            json,
            standard_attributes,
        )?;
        let reflect = self.allocate_reflect_object();
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Reflect",
            reflect,
            standard_attributes,
        )?;
        let string = self.allocate_string_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "String",
            string,
            standard_attributes,
        )?;
        let number = self.allocate_number_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Number",
            number,
            standard_attributes,
        )?;
        let boolean = self.allocate_boolean_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Boolean",
            boolean,
            standard_attributes,
        )?;
        let error_name = strings.allocate_untracked("Error");
        let type_error_name = strings.allocate_untracked("TypeError");
        let reference_error_name = strings.allocate_untracked("ReferenceError");
        let empty_message = strings.allocate_untracked("");
        let error =
            self.allocate_error_constructor_with_write_barrier(heap, error_name, empty_message)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Error",
            error,
            standard_attributes,
        )?;
        let type_error = self.allocate_type_error_constructor_with_write_barrier(
            heap,
            error_name,
            type_error_name,
            empty_message,
        )?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "TypeError",
            type_error,
            standard_attributes,
        )?;
        let reference_error = self.allocate_reference_error_constructor_with_write_barrier(
            heap,
            error_name,
            reference_error_name,
            empty_message,
        )?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "ReferenceError",
            reference_error,
            standard_attributes,
        )?;
        let map = self.allocate_map_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Map",
            map,
            standard_attributes,
        )?;
        let set = self.allocate_set_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Set",
            set,
            standard_attributes,
        )?;
        let weak_map = self.allocate_weak_map_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "WeakMap",
            weak_map,
            standard_attributes,
        )?;
        let weak_set = self.allocate_weak_set_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "WeakSet",
            weak_set,
            standard_attributes,
        )?;
        let regexp = self.allocate_regexp_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "RegExp",
            regexp,
            standard_attributes,
        )?;
        let promise = self.allocate_promise_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Promise",
            promise,
            standard_attributes,
        )?;
        let date = self.allocate_date_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Date",
            date,
            standard_attributes,
        )?;
        let bigint = self.allocate_bigint_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "BigInt",
            bigint,
            standard_attributes,
        )?;
        let array_buffer = self.allocate_array_buffer_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "ArrayBuffer",
            array_buffer,
            standard_attributes,
        )?;
        // Install each wired Number-content typed-array constructor as a standard
        // global data property (Int8Array, Uint8Array, Uint8ClampedArray, ...),
        // mirroring C++ JSTypedArrayConstructors global installation.
        for kind in WIRED_TYPED_ARRAY_KINDS {
            let constructor =
                self.allocate_typed_array_constructor_with_write_barrier(heap, kind)?;
            self.install_standard_global_data_property(
                heap,
                global_object,
                typed_array_kind_name(kind),
                constructor,
                standard_attributes,
            )?;
        }
        let data_view = self.allocate_data_view_constructor_with_write_barrier(heap)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "DataView",
            data_view,
            standard_attributes,
        )?;
        let proxy = self.allocate_proxy_constructor();
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Proxy",
            proxy,
            standard_attributes,
        )?;
        let iterator = symbols.well_known_untracked("Symbol.iterator");
        let symbol = self.allocate_symbol_constructor_with_write_barrier(heap, iterator)?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Symbol",
            symbol,
            standard_attributes,
        )?;
        // C++ JSC JSGlobalObject::init binds the global `eval` to globalFuncEval
        // (runtime/JSGlobalObject.cpp / JSGlobalObjectFunctions.cpp:450) as a
        // DontEnum data property. standard_attributes matches DontEnum (writable,
        // configurable, not enumerable). Indirect/global eval only.
        let eval = self.allocate_native_function(CoreNativeFunction::GlobalEval);
        self.install_standard_global_data_property(
            heap,
            global_object,
            "eval",
            eval,
            standard_attributes,
        )?;
        let parse_int = self.allocate_native_function(CoreNativeFunction::ParseInt);
        self.install_standard_global_data_property(
            heap,
            global_object,
            "parseInt",
            parse_int,
            standard_attributes,
        )?;
        let parse_float = self.allocate_native_function(CoreNativeFunction::ParseFloat);
        self.install_standard_global_data_property(
            heap,
            global_object,
            "parseFloat",
            parse_float,
            standard_attributes,
        )?;
        // C++ JSC JSGlobalObject::init binds the global `isFinite`/`isNaN`
        // (runtime/JSGlobalObjectFunctions.cpp). standard_attributes matches
        // their DontEnum installation.
        let is_finite = self.allocate_native_function(CoreNativeFunction::GlobalIsFinite);
        self.install_standard_global_data_property(
            heap,
            global_object,
            "isFinite",
            is_finite,
            standard_attributes,
        )?;
        let is_nan = self.allocate_native_function(CoreNativeFunction::GlobalIsNaN);
        self.install_standard_global_data_property(
            heap,
            global_object,
            "isNaN",
            is_nan,
            standard_attributes,
        )?;
        // C++ JSC JSGlobalObject::init installs the URI/escape global functions
        // with DontEnum (runtime/JSGlobalObject.cpp:699-704). standard_attributes
        // matches DontEnum (writable, configurable, not enumerable).
        for (name, native) in [
            ("escape", CoreNativeFunction::GlobalEscape),
            ("unescape", CoreNativeFunction::GlobalUnescape),
            ("decodeURI", CoreNativeFunction::GlobalDecodeURI),
            (
                "decodeURIComponent",
                CoreNativeFunction::GlobalDecodeURIComponent,
            ),
            ("encodeURI", CoreNativeFunction::GlobalEncodeURI),
            (
                "encodeURIComponent",
                CoreNativeFunction::GlobalEncodeURIComponent,
            ),
        ] {
            let function = self.allocate_native_function(native);
            self.install_standard_global_data_property(
                heap,
                global_object,
                name,
                function,
                standard_attributes,
            )?;
        }
        // C++ JSC JSGlobalObject::init installs `NaN` and `Infinity` as value
        // properties with DontEnum | DontDelete | ReadOnly attributes
        // (not writable, not enumerable, not configurable).
        let value_constant_attributes = CorePropertyAttributes {
            writable: false,
            enumerable: false,
            configurable: false,
        };
        self.install_standard_global_data_property(
            heap,
            global_object,
            "NaN",
            RuntimeValue::from_double(f64::NAN),
            value_constant_attributes,
        )?;
        self.install_standard_global_data_property(
            heap,
            global_object,
            "Infinity",
            RuntimeValue::from_double(f64::INFINITY),
            value_constant_attributes,
        )?;
        Ok(())
    }

    pub(crate) fn install_host_global_properties<I, S>(
        &mut self,
        heap: &mut Heap,
        global_object: RuntimeValue,
        names: I,
    ) -> Result<(), ExecutionError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let host_attributes = CorePropertyAttributes {
            writable: true,
            enumerable: false,
            configurable: true,
        };
        for name in names {
            let name = name.as_ref();
            let value = match name {
                "performance" => self.allocate_host_performance_object(),
                "print" => self.allocate_native_function(CoreNativeFunction::HostPrint),
                "alert" => self.allocate_native_function(CoreNativeFunction::HostAlert),
                "console" => self.allocate_host_console_object(),
                "readFile" => self.allocate_native_function(CoreNativeFunction::HostReadFile),
                // C++ JSC jsc shell binds `read` as a host-function alias of
                // readFile (jsc.cpp:683 -> functionReadFile). Reuse HostReadFile.
                "read" => self.allocate_native_function(CoreNativeFunction::HostReadFile),
                "top" => self.allocate_host_top_object(),
                _ => return Err(ExecutionError::UnknownHostGlobal),
            };
            self.install_standard_global_data_property(
                heap,
                global_object,
                name,
                value,
                host_attributes,
            )?;
        }
        Ok(())
    }

    pub(crate) fn allocate_host_performance_object(&mut self) -> RuntimeValue {
        let object = self.allocate();
        self.install_native_method(object, "now", CoreNativeFunction::HostPerformanceNow);
        object
    }

    pub(crate) fn allocate_host_console_object(&mut self) -> RuntimeValue {
        let object = self.allocate();
        for (name, native_function) in [
            ("log", CoreNativeFunction::HostConsoleLog),
            ("info", CoreNativeFunction::HostConsoleInfo),
            ("warn", CoreNativeFunction::HostConsoleWarn),
            ("error", CoreNativeFunction::HostConsoleError),
        ] {
            self.install_native_method(object, name, native_function);
        }
        object
    }

    pub(crate) fn allocate_host_top_object(&mut self) -> RuntimeValue {
        let object = self.allocate();
        self.install_native_method(
            object,
            "currentResolve",
            CoreNativeFunction::HostCurrentResolve,
        );
        self.install_native_method(
            object,
            "currentReject",
            CoreNativeFunction::HostCurrentReject,
        );
        object
    }

    pub(crate) fn install_standard_global_data_property(
        &mut self,
        _heap: &mut Heap,
        global_object: RuntimeValue,
        name: &str,
        value: RuntimeValue,
        attributes: CorePropertyAttributes,
    ) -> Result<(), ExecutionError> {
        let key = CorePropertyKey::String(name.into());
        let _ = self.define_data_property(global_object, &key, value, attributes)?;
        Ok(())
    }

    pub(crate) fn allocate_map(&mut self) -> RuntimeValue {
        let prototype = self.ensure_map_prototype();
        // gc-r4 Map/Set unit: eagerly allocate this Map's empty ordered-entry backing
        // (like every JSObject gets a butterfly at `allocate_cell`) so its handle is
        // valid for the cell's whole life and no read path sees the INVALID sentinel.
        let entries = self.allocate_map_entries();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Map,
            prototype: Some(prototype),
            map_entries: entries,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_regexp(&mut self, source: String, flags: RegexFlags) -> RuntimeValue {
        let prototype = self.ensure_regexp_prototype();
        // Relocate the pattern string into the store-owned slab first (C++
        // RegExp::m_patternString); the cell carries only the POD handle. The flags
        // text is NOT stored — it is recomputed from `flags` on demand (JSC has no
        // stored flags string; see the `regexp_flags` field comment).
        let source_handle = self.allocate_regexp_source(source);
        let object = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::RegExp,
            prototype: Some(prototype),
            regexp_source: source_handle,
            regexp_flags: flags,
            ..CoreObjectCell::default()
        });
        let _ = self.put_data_own(
            object,
            &CorePropertyKey::String("lastIndex".into()),
            RuntimeValue::from_i32(0),
        );
        object
    }

    pub(crate) fn allocate_promise(&mut self) -> RuntimeValue {
        let prototype = self.ensure_promise_prototype();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Promise,
            prototype: Some(prototype),
            promise_state: PromiseState::Pending,
            promise_result: RuntimeValue::undefined(),
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_settled_promise_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        state: PromiseState,
        result: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let prototype = self.ensure_promise_prototype();
        let promise = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Promise,
            prototype: Some(prototype),
            promise_state: state,
            promise_result: RuntimeValue::undefined(),
            ..CoreObjectCell::default()
        });
        self.apply_value_store_write_barrier(heap, promise, result)?;
        let Some(promise_cell) = self.find_mut(promise) else {
            return Err(ExecutionError::ExpectedObject);
        };
        promise_cell.promise_result = result;
        Ok(promise)
    }

    pub(crate) fn allocate_proxy_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        target: RuntimeValue,
        handler: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let prototype = self.find(target).and_then(|target| target.prototype);
        let proxy = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Proxy,
            ..CoreObjectCell::default()
        });
        if let Some(prototype) = prototype {
            self.set_prototype_or_null_with_write_barrier(heap, proxy, Some(prototype))?;
        }
        self.apply_value_store_write_barrier(heap, proxy, target)?;
        self.apply_value_store_write_barrier(heap, proxy, handler)?;
        let Some(proxy_cell) = self.find_mut(proxy) else {
            return Err(ExecutionError::ExpectedObject);
        };
        proxy_cell.proxy_target = Some(target);
        proxy_cell.proxy_handler = Some(handler);
        Ok(proxy)
    }

    pub(crate) fn allocate_proxy_revoke_function_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        proxy: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let prototype = self.ensure_function_prototype();
        let revoke = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::NativeFunction,
            prototype: Some(prototype),
            native_function: Some(CoreNativeFunction::ProxyRevoke),
            ..CoreObjectCell::default()
        });
        self.apply_value_store_write_barrier(heap, revoke, proxy)?;
        let Some(revoke_cell) = self.find_mut(revoke) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        revoke_cell.native_bound_proxy = Some(proxy);
        Ok(revoke)
    }

    pub(crate) fn allocate_date(&mut self, time_value: f64) -> RuntimeValue {
        let prototype = self.ensure_date_prototype();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Date,
            prototype: Some(prototype),
            date_value: time_clip(time_value),
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_array_buffer(&mut self, byte_length: usize) -> RuntimeValue {
        let prototype = self.ensure_array_buffer_prototype();
        // gc-r4 ArrayBuffer unit: the backing bytes live in the store-owned slab; the
        // cell carries only the POD handle (C++ ArrayBufferContents::m_data relocation).
        let backing = self.allocate_array_buffer_backing(byte_length);
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::ArrayBuffer,
            prototype: Some(prototype),
            array_buffer_data: backing,
            ..CoreObjectCell::default()
        })
    }

    // Test-only Uint8 convenience over allocate_typed_array_with_write_barrier;
    // the production path uses the kind-parameterized allocator directly.
    #[cfg(test)]
    pub(crate) fn allocate_uint8_array_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        buffer: RuntimeValue,
        byte_offset: usize,
        length: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        self.allocate_typed_array_with_write_barrier(
            heap,
            TypedArrayElementKind::Uint8,
            buffer,
            byte_offset,
            length,
        )
    }

    /// Allocate a typed-array view of `element_kind` over `buffer`, mirroring C++
    /// `JSGenericTypedArrayView::create` for the buffer-backed form. `length` is
    /// the element count; `view_byte_length` is `length * element_size`. The view
    /// shares the buffer (no copy), and the element prototype is selected by kind.
    pub(crate) fn allocate_typed_array_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        element_kind: TypedArrayElementKind,
        buffer: RuntimeValue,
        byte_offset: usize,
        length: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let element_size = usize::from(typed_array_element_size(element_kind));
        let prototype = self.ensure_typed_array_prototype(element_kind);
        let view = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Uint8Array,
            prototype: Some(prototype),
            view_byte_offset: byte_offset,
            view_byte_length: length.saturating_mul(element_size),
            view_length: length,
            view_element_kind: element_kind,
            ..CoreObjectCell::default()
        });
        self.apply_value_store_write_barrier(heap, view, buffer)?;
        let Some(view_cell) = self.find_mut(view) else {
            return Err(ExecutionError::ExpectedObject);
        };
        view_cell.view_buffer = Some(buffer);
        Ok(view)
    }

    pub(crate) fn allocate_data_view_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        buffer: RuntimeValue,
        byte_offset: usize,
        byte_length: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let prototype = self.ensure_data_view_prototype();
        let view = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::DataView,
            prototype: Some(prototype),
            view_byte_offset: byte_offset,
            view_byte_length: byte_length,
            ..CoreObjectCell::default()
        });
        self.apply_value_store_write_barrier(heap, view, buffer)?;
        let Some(view_cell) = self.find_mut(view) else {
            return Err(ExecutionError::ExpectedObject);
        };
        view_cell.view_buffer = Some(buffer);
        Ok(view)
    }

    pub(crate) fn allocate_promise_resolving_function_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        promise: RuntimeValue,
        kind: CorePromiseResolvingKind,
    ) -> Result<RuntimeValue, ExecutionError> {
        let prototype = self.ensure_function_prototype();
        let function = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::NativeFunction,
            prototype: Some(prototype),
            native_function: Some(CoreNativeFunction::PromiseResolvingFunction),
            promise_resolving_kind: Some(kind),
            ..CoreObjectCell::default()
        });
        self.apply_value_store_write_barrier(heap, function, promise)?;
        let Some(function_cell) = self.find_mut(function) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        function_cell.native_bound_promise = Some(promise);
        Ok(function)
    }

    pub(crate) fn allocate_set(&mut self) -> RuntimeValue {
        let prototype = self.ensure_set_prototype();
        // gc-r4 Map/Set unit: eagerly allocate this Set's empty ordered-value backing
        // (see `allocate_map`).
        let values = self.allocate_set_values();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Set,
            prototype: Some(prototype),
            set_values: values,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_weak_map(&mut self) -> RuntimeValue {
        let prototype = self.ensure_weak_map_prototype();
        // gc-r4 Map/Set unit: a WeakMap stores (key,value) entries like a Map, so it
        // eagerly gets a map-entry backing (see `allocate_map`).
        let entries = self.allocate_map_entries();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::WeakMap,
            prototype: Some(prototype),
            map_entries: entries,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_weak_set(&mut self) -> RuntimeValue {
        let prototype = self.ensure_weak_set_prototype();
        // gc-r4 Map/Set unit: a WeakSet stores values like a Set, so it eagerly gets a
        // set-value backing (see `allocate_map`).
        let values = self.allocate_set_values();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::WeakSet,
            prototype: Some(prototype),
            set_values: values,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn ensure_object_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.object_prototype {
            return prototype;
        }
        let prototype = self.allocate_cell(CoreObjectCell::default());
        self.object_prototype = Some(prototype);
        for (name, native_function) in [
            (
                "hasOwnProperty",
                CoreNativeFunction::ObjectPrototypeHasOwnProperty,
            ),
            ("toString", CoreNativeFunction::ObjectPrototypeToString),
            ("valueOf", CoreNativeFunction::ObjectPrototypeValueOf),
            // Legacy accessor helpers (C++ JSC ObjectPrototype.cpp
            // objectProtoFuncDefineGetter / objectProtoFuncDefineSetter).
            ("__defineGetter__", CoreNativeFunction::ObjectDefineGetter),
            ("__defineSetter__", CoreNativeFunction::ObjectDefineSetter),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_function_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.function_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.function_prototype = Some(prototype);
        // C++ JSC FunctionPrototype::addFunctionProperties installs call/apply/
        // bind on Function.prototype as DontEnum. We mirror that here. apply/bind
        // and call share the function prototype as their own prototype.
        for (name, native_function) in [
            ("call", CoreNativeFunction::FunctionCall),
            ("apply", CoreNativeFunction::FunctionApply),
            ("bind", CoreNativeFunction::FunctionBind),
        ] {
            let function =
                self.allocate_native_function_with_prototype(native_function, Some(prototype));
            let key = CorePropertyKey::String(name.into());
            let _ = self.define_data_property(
                prototype,
                &key,
                function,
                CorePropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
        prototype
    }

    pub(crate) fn ensure_array_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.array_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.array_prototype = Some(prototype);
        for (name, native_function) in [
            ("push", CoreNativeFunction::ArrayPush),
            ("pop", CoreNativeFunction::ArrayPop),
            ("shift", CoreNativeFunction::ArrayShift),
            ("unshift", CoreNativeFunction::ArrayUnshift),
            ("join", CoreNativeFunction::ArrayJoin),
            ("toString", CoreNativeFunction::ArrayPrototypeToString),
            ("slice", CoreNativeFunction::ArraySlice),
            ("concat", CoreNativeFunction::ArrayConcat),
            ("fill", CoreNativeFunction::ArrayFill),
            ("reverse", CoreNativeFunction::ArrayReverse),
            ("sort", CoreNativeFunction::ArraySort),
            ("splice", CoreNativeFunction::ArraySplice),
            ("indexOf", CoreNativeFunction::ArrayIndexOf),
            ("includes", CoreNativeFunction::ArrayIncludes),
            ("forEach", CoreNativeFunction::ArrayForEach),
            ("map", CoreNativeFunction::ArrayMap),
            ("filter", CoreNativeFunction::ArrayFilter),
            ("some", CoreNativeFunction::ArraySome),
            ("every", CoreNativeFunction::ArrayEvery),
            ("find", CoreNativeFunction::ArrayFind),
            ("findIndex", CoreNativeFunction::ArrayFindIndex),
            ("reduce", CoreNativeFunction::ArrayReduce),
            ("reduceRight", CoreNativeFunction::ArrayReduceRight),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_string_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.string_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.string_prototype = Some(prototype);
        for (name, native_function) in [
            ("charAt", CoreNativeFunction::StringCharAt),
            ("charCodeAt", CoreNativeFunction::StringCharCodeAt),
            ("indexOf", CoreNativeFunction::StringIndexOf),
            ("lastIndexOf", CoreNativeFunction::StringLastIndexOf),
            ("slice", CoreNativeFunction::StringSlice),
            ("substring", CoreNativeFunction::StringSubstring),
            ("substr", CoreNativeFunction::StringSubstr),
            ("split", CoreNativeFunction::StringSplit),
            ("replace", CoreNativeFunction::StringReplace),
            ("match", CoreNativeFunction::StringMatch),
            ("toLowerCase", CoreNativeFunction::StringToLowerCase),
            ("toUpperCase", CoreNativeFunction::StringToUpperCase),
            (
                "toLocaleLowerCase",
                CoreNativeFunction::StringToLocaleLowerCase,
            ),
            (
                "toLocaleUpperCase",
                CoreNativeFunction::StringToLocaleUpperCase,
            ),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_number_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.number_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.number_prototype = Some(prototype);
        // C++ JSC: NumberPrototype::finishCreation installs toString and valueOf
        // on Number.prototype. toString is numberProtoFuncToString and valueOf
        // is numberProtoFuncValueOf.
        self.install_native_method(
            prototype,
            "toString",
            CoreNativeFunction::NumberPrototypeToString,
        );
        self.install_native_method(
            prototype,
            "valueOf",
            CoreNativeFunction::NumberPrototypeValueOf,
        );
        prototype
    }

    pub(crate) fn ensure_boolean_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.boolean_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.boolean_prototype = Some(prototype);
        prototype
    }

    pub(crate) fn ensure_error_prototype(
        &mut self,
        name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) -> RuntimeValue {
        if let Some(prototype) = self.error_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.error_prototype = Some(prototype);
        self.install_error_prototype_fields(prototype, name_value, message_value);
        self.install_native_method(
            prototype,
            "toString",
            CoreNativeFunction::ErrorPrototypeToString,
        );
        prototype
    }

    pub(crate) fn ensure_type_error_prototype(
        &mut self,
        error_name_value: RuntimeValue,
        type_error_name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) -> RuntimeValue {
        if let Some(prototype) = self.type_error_prototype {
            return prototype;
        }
        let error_prototype = self.ensure_error_prototype(error_name_value, message_value);
        let prototype = self.allocate_with_prototype(Some(error_prototype));
        self.type_error_prototype = Some(prototype);
        self.install_error_prototype_fields(prototype, type_error_name_value, message_value);
        prototype
    }

    pub(crate) fn ensure_reference_error_prototype(
        &mut self,
        error_name_value: RuntimeValue,
        reference_error_name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) -> RuntimeValue {
        if let Some(prototype) = self.reference_error_prototype {
            return prototype;
        }
        let error_prototype = self.ensure_error_prototype(error_name_value, message_value);
        let prototype = self.allocate_with_prototype(Some(error_prototype));
        self.reference_error_prototype = Some(prototype);
        self.install_error_prototype_fields(prototype, reference_error_name_value, message_value);
        prototype
    }

    // C++ JSC: RangeError.prototype, a native error subclass whose [[Prototype]] is
    // Error.prototype (ErrorConstructor / NativeErrorPrototype). Built lazily and
    // identically to ReferenceError.prototype above; used by the catchable
    // `RangeError("Invalid array length")` that `JSArray::put` throws.
    pub(crate) fn ensure_range_error_prototype(
        &mut self,
        error_name_value: RuntimeValue,
        range_error_name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) -> RuntimeValue {
        if let Some(prototype) = self.range_error_prototype {
            return prototype;
        }
        let error_prototype = self.ensure_error_prototype(error_name_value, message_value);
        let prototype = self.allocate_with_prototype(Some(error_prototype));
        self.range_error_prototype = Some(prototype);
        self.install_error_prototype_fields(prototype, range_error_name_value, message_value);
        prototype
    }

    pub(crate) fn ensure_map_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.map_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.map_prototype = Some(prototype);
        for (name, native_function) in [
            ("get", CoreNativeFunction::MapGet),
            ("set", CoreNativeFunction::MapSet),
            ("has", CoreNativeFunction::MapHas),
            ("delete", CoreNativeFunction::MapDelete),
            ("clear", CoreNativeFunction::MapClear),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        self.install_native_getter(prototype, "size", CoreNativeFunction::MapSize);
        prototype
    }

    pub(crate) fn ensure_set_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.set_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.set_prototype = Some(prototype);
        for (name, native_function) in [
            ("add", CoreNativeFunction::SetAdd),
            ("has", CoreNativeFunction::SetHas),
            ("delete", CoreNativeFunction::SetDelete),
            ("clear", CoreNativeFunction::SetClear),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        self.install_native_getter(prototype, "size", CoreNativeFunction::SetSize);
        prototype
    }

    pub(crate) fn ensure_weak_map_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.weak_map_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.weak_map_prototype = Some(prototype);
        for (name, native_function) in [
            ("get", CoreNativeFunction::WeakMapGet),
            ("set", CoreNativeFunction::WeakMapSet),
            ("has", CoreNativeFunction::WeakMapHas),
            ("delete", CoreNativeFunction::WeakMapDelete),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_weak_set_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.weak_set_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.weak_set_prototype = Some(prototype);
        for (name, native_function) in [
            ("add", CoreNativeFunction::WeakSetAdd),
            ("has", CoreNativeFunction::WeakSetHas),
            ("delete", CoreNativeFunction::WeakSetDelete),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_regexp_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.regexp_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.regexp_prototype = Some(prototype);
        for (name, native_function) in [
            ("test", CoreNativeFunction::RegExpTest),
            ("exec", CoreNativeFunction::RegExpExec),
            ("toString", CoreNativeFunction::RegExpPrototypeToString),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        // RegExp.prototype accessor getters, mirroring the order and
        // DontEnum|Accessor attributes of RegExpPrototype::finishCreation
        // (runtime/RegExpPrototype.cpp:81-90). install_native_getter installs a
        // DontEnum accessor with no setter, matching the native getter setup.
        // C++ also installs `unicodeSets` (:88); RegExpFlags models that flag, so
        // we install it too rather than deferring.
        for (name, native_function) in [
            ("global", CoreNativeFunction::RegExpProtoGetterGlobal),
            ("dotAll", CoreNativeFunction::RegExpProtoGetterDotAll),
            (
                "hasIndices",
                CoreNativeFunction::RegExpProtoGetterHasIndices,
            ),
            (
                "ignoreCase",
                CoreNativeFunction::RegExpProtoGetterIgnoreCase,
            ),
            ("multiline", CoreNativeFunction::RegExpProtoGetterMultiline),
            ("sticky", CoreNativeFunction::RegExpProtoGetterSticky),
            ("unicode", CoreNativeFunction::RegExpProtoGetterUnicode),
            (
                "unicodeSets",
                CoreNativeFunction::RegExpProtoGetterUnicodeSets,
            ),
            ("source", CoreNativeFunction::RegExpProtoGetterSource),
            ("flags", CoreNativeFunction::RegExpProtoGetterFlags),
        ] {
            self.install_native_getter(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_promise_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.promise_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.promise_prototype = Some(prototype);
        for (name, native_function) in [
            ("then", CoreNativeFunction::PromiseThen),
            ("catch", CoreNativeFunction::PromiseCatch),
            ("finally", CoreNativeFunction::PromiseFinally),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_date_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.date_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.date_prototype = Some(prototype);
        for (name, native_function) in [
            ("getTime", CoreNativeFunction::DateGetTime),
            ("valueOf", CoreNativeFunction::DateValueOf),
            ("toISOString", CoreNativeFunction::DateToISOString),
            ("toString", CoreNativeFunction::DatePrototypeToString),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_bigint_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.bigint_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.bigint_prototype = Some(prototype);
        for (name, native_function) in [
            ("toString", CoreNativeFunction::BigIntPrototypeToString),
            ("valueOf", CoreNativeFunction::BigIntPrototypeValueOf),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_array_buffer_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.array_buffer_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.array_buffer_prototype = Some(prototype);
        self.install_native_getter(
            prototype,
            "byteLength",
            CoreNativeFunction::ArrayBufferByteLength,
        );
        self.install_native_method(prototype, "slice", CoreNativeFunction::ArrayBufferSlice);
        prototype
    }

    /// Lazily allocate the prototype object for a typed-array element kind,
    /// mirroring C++ where every JSGenericTypedArrayView<Adaptor> has a distinct
    /// prototype off Object.prototype. The length/byteLength/byteOffset/buffer
    /// getters and fill/set/subarray methods are shared across kinds because the
    /// native implementations now read the element kind off the receiver cell
    /// (the C++ prototype functions are likewise generic over TypedArrayType).
    pub(crate) fn ensure_typed_array_prototype(
        &mut self,
        kind: TypedArrayElementKind,
    ) -> RuntimeValue {
        let index = typed_array_kind_index(kind);
        if let Some(prototype) = self.typed_array_prototypes[index] {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.typed_array_prototypes[index] = Some(prototype);
        for (name, native_function) in [
            ("length", CoreNativeFunction::Uint8ArrayLength),
            ("byteLength", CoreNativeFunction::Uint8ArrayByteLength),
            ("byteOffset", CoreNativeFunction::Uint8ArrayByteOffset),
            ("buffer", CoreNativeFunction::Uint8ArrayBuffer),
        ] {
            self.install_native_getter(prototype, name, native_function);
        }
        for (name, native_function) in [
            ("fill", CoreNativeFunction::Uint8ArrayFill),
            ("set", CoreNativeFunction::Uint8ArraySet),
            ("subarray", CoreNativeFunction::Uint8ArraySubarray),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_data_view_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.data_view_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.data_view_prototype = Some(prototype);
        for (name, native_function) in [
            ("buffer", CoreNativeFunction::DataViewBuffer),
            ("byteLength", CoreNativeFunction::DataViewByteLength),
            ("byteOffset", CoreNativeFunction::DataViewByteOffset),
        ] {
            self.install_native_getter(prototype, name, native_function);
        }
        for (name, native_function) in [
            ("getUint8", CoreNativeFunction::DataViewGetUint8),
            ("setUint8", CoreNativeFunction::DataViewSetUint8),
            ("getInt8", CoreNativeFunction::DataViewGetInt8),
            ("setInt8", CoreNativeFunction::DataViewSetInt8),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn ensure_symbol_prototype(&mut self) -> RuntimeValue {
        if let Some(prototype) = self.symbol_prototype {
            return prototype;
        }
        let object_prototype = self.ensure_object_prototype();
        let prototype = self.allocate_with_prototype(Some(object_prototype));
        self.symbol_prototype = Some(prototype);
        self.install_native_getter(
            prototype,
            "description",
            CoreNativeFunction::SymbolDescription,
        );
        for (name, native_function) in [
            ("toString", CoreNativeFunction::SymbolPrototypeToString),
            ("valueOf", CoreNativeFunction::SymbolPrototypeValueOf),
        ] {
            self.install_native_method(prototype, name, native_function);
        }
        prototype
    }

    pub(crate) fn install_error_prototype_fields(
        &mut self,
        prototype: RuntimeValue,
        name_value: RuntimeValue,
        message_value: RuntimeValue,
    ) {
        let attributes = CorePropertyAttributes {
            writable: true,
            enumerable: false,
            configurable: true,
        };
        let _ = self.define_data_property(
            prototype,
            &CorePropertyKey::String("name".into()),
            name_value,
            attributes,
        );
        let _ = self.define_data_property(
            prototype,
            &CorePropertyKey::String("message".into()),
            message_value,
            attributes,
        );
    }

    pub(crate) fn install_native_method(
        &mut self,
        object: RuntimeValue,
        name: &str,
        native_function: CoreNativeFunction,
    ) {
        let function = self.allocate_native_function(native_function);
        let key = CorePropertyKey::String(name.into());
        let _ = self.define_data_property(
            object,
            &key,
            function,
            CorePropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
            },
        );
    }

    pub(crate) fn install_native_getter(
        &mut self,
        object: RuntimeValue,
        name: &str,
        native_function: CoreNativeFunction,
    ) {
        let getter = self.allocate_native_function(native_function);
        let key = CorePropertyKey::String(name.into());
        let _ = self.define_accessor_property(
            object,
            &key,
            Some(getter),
            None,
            CorePropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }

    pub(crate) fn install_constructor_prototype(
        &mut self,
        constructor: RuntimeValue,
        prototype: RuntimeValue,
    ) {
        let key = CorePropertyKey::String("prototype".into());
        let _ = self.define_data_property(
            constructor,
            &key,
            prototype,
            CorePropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
            },
        );
    }

    pub(crate) fn install_prototype_constructor(
        &mut self,
        prototype: RuntimeValue,
        constructor: RuntimeValue,
    ) {
        let key = CorePropertyKey::String("constructor".into());
        let _ = self.define_data_property(
            prototype,
            &key,
            constructor,
            CorePropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
            },
        );
    }

    pub(crate) fn install_prototype_constructor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        prototype: RuntimeValue,
        constructor: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.apply_value_store_write_barrier(heap, prototype, constructor)?;
        self.install_prototype_constructor(prototype, constructor);
        Ok(())
    }

    pub(crate) fn allocate_closure_cell(&mut self, value: RuntimeValue) -> RuntimeValue {
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::ClosureCell,
            binding_value: value,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_closure_cell_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let cell = self.allocate_closure_cell(RuntimeValue::undefined());
        self.put_closure_cell_with_write_barrier(heap, cell, value)?;
        Ok(cell)
    }

    pub(crate) fn allocate_cell(&mut self, mut cell: CoreObjectCell) -> RuntimeValue {
        // Set the faithful JSCell::m_type (runtime/JSCell.h:298) header from the cell's
        // final kind at the single allocation chokepoint, so every published object
        // cell carries a coherent tag without per-call-site edits (covers all
        // `..CoreObjectCell::default()` literals).
        cell.js_type = cell.kind.js_type();
        // gc-r4 Butterfly-values: assign a fresh store-owned butterfly at the single
        // allocation chokepoint (C++ Heap::tryAllocateButterfly out of the Auxiliary
        // subspace). The cell was built with the INVALID sentinel; this gives it its
        // own (empty) slab entry BEFORE the structure is seeded and the out-of-line
        // VALUE mirror + indexed elements are filled below.
        cell.butterfly = self.allocate_butterfly();
        if cell.structure_id == StructureId::INVALID {
            // C++ JSC: a fresh object adopts the shared empty Structure for its
            // class+prototype instead of a private one, so same-shape siblings can
            // converge under one property-transition graph. seed_structure_id
            // reconstructs that shared root (see its comment); the prior behavior
            // here minted a private id per object, defeating cross-instance ICs.
            //
            // gc-r4 B-iv: cells are no longer born carrying initial own properties (the
            // per-cell `properties` channel is gone). The one prior user,
            // allocate_function_with_construct_ability, now allocates EMPTY here and
            // installs `.prototype` afterward through `define_data_property`, so the
            // initial shape is always the empty (class, prototype) seed root and the
            // initial-shape replay (seed_initial_shape_structure_id) + the out-of-line
            // initial fill (fill_initial_property_storage) are no longer needed.
            cell.structure_id = self.seed_structure_id(cell.kind, cell.prototype);
        }
        let mut object = Box::pin(cell);
        let ptr = NonNull::from(object.as_mut().get_mut());
        let payload = ptr.as_ptr() as usize;
        let index = self.objects.len();
        debug_assert!(
            !self.object_indices_by_payload.contains_key(&payload),
            "new interpreter object payload reused while still live"
        );
        self.objects.push(object);
        self.object_indices_by_payload.insert(payload, index);
        // SAFETY: The host owns the boxed cell for the lifetime of the dispatch
        // run and never moves the allocation after the value is published.
        RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
    }

    /// Allocate a GetterSetter cell (C++ runtime/GetterSetter.h:42 `GetterSetter::
    /// create`): a fixed POD cell holding the property's getter and setter functions.
    /// A null getter/setter maps to `None` (GetterSetter.h:132-133, the missing half is
    /// the undefined sentinel). The returned value IS `from_cell(getter_setter)` — the
    /// `RuntimeValue` an accessor's butterfly slot stores, exactly as C++ stores a
    /// `GetterSetter*` (gc-r4 B-ii). Routed through the single `allocate_cell` chokepoint
    /// so the cell gets a coherent header + butterfly like every other cell.
    pub(crate) fn allocate_getter_setter(
        &mut self,
        getter: Option<RuntimeValue>,
        setter: Option<RuntimeValue>,
    ) -> RuntimeValue {
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::GetterSetter,
            getter_value: getter,
            setter_value: setter,
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn rebuild_object_indices(&mut self) {
        self.object_indices_by_payload.clear();
        for (index, object) in self.objects.iter().enumerate() {
            let payload = core::ptr::from_ref(object.as_ref().get_ref()) as usize;
            self.object_indices_by_payload.insert(payload, index);
        }
    }

    /// Mint a fresh, EMPTY standalone structure (a new root in the structure_table).
    ///
    /// gc-r4 Batch 2: the former flat id allocator is gone; a structure id IS a
    /// `StructureIdTable` handle. This is the analog of C++ `Structure::create` for a
    /// shape with NO properties to carry. Used where there is no prior shape to
    /// preserve (and by tests fabricating a planned transition target). For a
    /// non-PropertyAddition change that MUST preserve surviving offsets, use
    /// `fresh_dictionary_structure` instead.
    pub(crate) fn allocate_structure_id(&mut self) -> StructureId {
        self.structure_table.create_root(
            PrototypePointer::null(),
            NON_ARRAY,
            0,
            0,
            INLINE_CAPACITY as u8,
        )
    }

    /// The `PrototypePointer` for a stored prototype (the prototype object's pinned
    /// pointer bits, or null), the faithful key C++ Structure stores in `m_prototype`.
    pub(crate) fn prototype_pointer(&self, prototype: Option<RuntimeValue>) -> PrototypePointer {
        match prototype.and_then(|value| value.as_cell().map(|cell| cell.pointer_payload_bits())) {
            Some(payload) => PrototypePointer::from_object(payload),
            None => PrototypePointer::null(),
        }
    }

    /// Intern a `CorePropertyKey` to a stable uniqued `AtomId` (the uid the ported
    /// PropertyTable / transition table key on), the adapter from the interpreter's
    /// key identity to JSC's `UniquedStringImpl*` identity. Slot 0 is reserved
    /// (`AtomId::UNASSIGNED` / a null transition-table pointer), so uids start at 1.
    pub(crate) fn intern_property_uid(&mut self, key: &CorePropertyKey) -> AtomId {
        if let Some(uid) = self.property_uids.get(key) {
            return *uid;
        }
        self.next_property_uid += 1;
        let uid = AtomId::from_table_slot(self.next_property_uid);
        self.property_uids.insert(key.clone(), uid);
        self.property_keys_by_uid.insert(uid, key.clone());
        uid
    }

    /// The uid a `CorePropertyKey` was interned to, if it has ever named an offset.
    pub(crate) fn lookup_property_uid(&self, key: &CorePropertyKey) -> Option<AtomId> {
        self.property_uids.get(key).copied()
    }

    /// True iff `sid` is a live registered structure handle.
    fn is_live_structure(&self, sid: StructureId) -> bool {
        sid != StructureId::INVALID && sid.raw() < self.structure_table.peek_next_handle().raw()
    }

    /// The (offset, attributes) structure `sid` assigns to `key` — read straight from
    /// the structure's Structure::PropertyTable (owned, or materialized-on-miss by
    /// replaying the transition chain, Structure.cpp:456). `PropertyTable::get` returns
    /// the `(offset, attributes)` tuple (object/property_table.rs:378, PropertyTable.h:
    /// 344); the attributes carry the PropertyAttribute bits the transition was keyed on,
    /// INCLUDING `PropertyAttribute::Accessor` (1<<4) for an accessor property — so a
    /// reader can tell an accessor offset from a data offset WITHOUT consulting the
    /// per-cell `properties` map. gc-r4 B-i EXPOSES the structure's attributes for the
    /// dual-write mirror + the eventual flip; the live read sites still resolve VALUES
    /// through `properties` this batch. Returns `None` for a key with no named offset in
    /// this shape (array-index strings, never added, or deleted/displaced).
    pub(crate) fn structure_property(
        &self,
        sid: StructureId,
        key: &CorePropertyKey,
    ) -> Option<(PropertyOffset, u32)> {
        if !self.is_live_structure(sid) {
            return None;
        }
        let uid = self.lookup_property_uid(key)?;
        let (raw, attributes) = match self.structure_table.structure(sid).property_table_or_null() {
            Some(table) => table.get(uid),
            // materialize-on-miss: the table was moved to a child via a transition;
            // rebuild it by replaying the chain (cache-back deferred, gc-r4 B2).
            None => self
                .structure_table
                .materialize_property_table(sid)
                .get(uid),
        };
        if raw < 0 {
            None
        } else {
            Some((PropertyOffset::new(raw), attributes))
        }
    }

    /// The property offset assigned to `key` by structure `sid`. The SINGLE offset
    /// authority (replacing the deleted per-cell `property_offsets`); a thin projection
    /// of `structure_property` that drops the attributes.
    pub(crate) fn structure_offset(
        &self,
        sid: StructureId,
        key: &CorePropertyKey,
    ) -> Option<PropertyOffset> {
        self.structure_property(sid, key).map(|(offset, _)| offset)
    }

    /// Reconstruct the own named property `key` of `cell` from its SHAPE (the Structure
    /// offset + attributes) and the butterfly slot value — the gc-r4 B-iv post-flip
    /// replacement for the per-cell `properties` HashMap, which is now DELETED.
    ///
    /// Faithful to `JSObject::getOwnNonIndexPropertySlot` (runtime/JSObject.h:1394-1428):
    ///   `offset = structure->get(vm, key, attributes)` (here `structure_property`);
    ///   if no valid offset -> the key is absent (`None`);
    ///   `JSValue value = getDirect(offset)` (here `butterfly_prop_get`);
    ///   if `attributes & PropertyAttribute::Accessor` the slot holds a `GetterSetter*`
    ///     (`fillGetterPropertySlot`) -> reconstruct an Accessor from the GetterSetter
    ///     cell's getter/setter (gc-r4 B-ii: the slot is `from_cell(GetterSetter)`);
    ///   else `slot.setValue(attributes, value)` -> a Data property.
    /// Returns `None` for a key with no named offset in this shape (absent, deleted, or
    /// an array-index key served from the indexed butterfly region instead).
    pub(crate) fn own_property_from_shape(
        &self,
        cell: &CoreObjectCell,
        key: &CorePropertyKey,
    ) -> Option<CoreProperty> {
        let (offset, attrs) = self.structure_property(cell.structure_id, key)?;
        let attributes = core_attributes_from_u32(attrs);
        if attrs & PROPERTY_ATTRIBUTE_ACCESSOR != 0 {
            // The butterfly slot holds `from_cell(GetterSetter)`; read the getter/setter
            // off that cell (C++ GetterSetter::getter()/setter(), GetterSetter.h:132-133).
            let getter_setter = self.butterfly_prop_get(cell.butterfly, offset)?;
            let gs = self.find(getter_setter)?;
            Some(CoreProperty {
                kind: CorePropertyKind::Accessor {
                    getter: gs.getter_value,
                    setter: gs.setter_value,
                },
                attributes,
            })
        } else {
            // A data slot in the structure ALWAYS has a butterfly home (every add does a
            // lockstep `putDirectOffset`); a present-in-shape key whose slot read misses
            // is the `undefined` data value (C++ getDirect returns JSValue() == undefined
            // for a never-written valid offset), never "absent".
            let value = self
                .butterfly_prop_get(cell.butterfly, offset)
                .unwrap_or_else(RuntimeValue::undefined);
            Some(CoreProperty {
                kind: CorePropertyKind::Data(value),
                attributes,
            })
        }
    }

    /// The own named properties of structure `sid`, in PropertyTable ENTRY (insertion)
    /// order, as `(key, offset, attributes)`. The gc-r4 B-iv replacement for the per-cell
    /// `property_order` Vec: C++ keeps enumeration order in the Structure's PropertyTable
    /// entry vector (`Structure::forEachProperty` / `getPropertyNamesFromStructure`,
    /// Structure.cpp:1326), never per-object. Visits live entries via
    /// `PropertyTable::forEachProperty` (PropertyTable.h:609) and maps each entry's uid
    /// back to its `CorePropertyKey` through `property_keys_by_uid`. Indexed (array)
    /// elements are NOT here — they live in the butterfly indexed region and are
    /// enumerated separately by the array/typed-array paths.
    pub(crate) fn structure_property_keys(
        &self,
        sid: StructureId,
    ) -> Vec<(CorePropertyKey, PropertyOffset, u32)> {
        if !self.is_live_structure(sid) {
            return Vec::new();
        }
        // `PropertyTableEntry::offset()` is the raw `i32` PropertyOffset; wrap into the
        // interpreter `PropertyOffset` newtype when projecting out.
        let mut raw: Vec<(AtomId, i32, u32)> = Vec::new();
        let collect = |table: &StructurePropertyTable, out: &mut Vec<(AtomId, i32, u32)>| {
            table.for_each_property(|entry| {
                if let Some(uid) = entry.key() {
                    out.push((uid, entry.offset(), entry.attributes()));
                }
            });
        };
        match self.structure_table.structure(sid).property_table_or_null() {
            Some(table) => collect(table, &mut raw),
            None => collect(
                &self.structure_table.materialize_property_table(sid),
                &mut raw,
            ),
        }
        raw.into_iter()
            .filter_map(|(uid, offset, attrs)| {
                self.property_keys_by_uid
                    .get(&uid)
                    .map(|key| (key.clone(), PropertyOffset::new(offset), attrs))
            })
            .collect()
    }

    /// Faithful `Structure::attributeChangeTransition` on a per-object dictionary
    /// (runtime/Structure.cpp:806): an OFFSET-STABLE kind/attribute change of an EXISTING
    /// property. Used for in-place data<->accessor conversion, accessor getter/setter
    /// update, and data attribute changes. Mints a fresh per-object dictionary that
    /// PRESERVES every offset (incl. `key`'s — `removed: None`), then rewrites `key`'s
    /// attributes in that dictionary's PropertyTable keeping its offset. The caller then
    /// OVERWRITES the butterfly slot at the returned offset with the new value (the data
    /// value, or `from_cell(GetterSetter)` for an accessor). Returns
    /// `(new_dictionary, preserved_offset)`.
    ///
    /// Pre-B-iv this path called `fresh_dictionary_structure(old, Some(key))`, which
    /// REMOVED the key from the shape — harmless while the HashMap was authoritative, but
    /// after the flip it would make the property VANISH. Keeping the offset is the fix.
    fn convert_property_in_place(
        &mut self,
        old_structure: StructureId,
        key: &CorePropertyKey,
        attributes: CorePropertyAttributes,
        is_accessor: bool,
    ) -> (StructureId, PropertyOffset) {
        let new_structure = self.fresh_dictionary_structure(old_structure, None);
        let offset = self
            .structure_offset(new_structure, key)
            .unwrap_or(PropertyOffset::INVALID);
        let attrs_u32 = core_attributes_to_u32(attributes, is_accessor);
        self.change_attributes_in_dictionary(new_structure, key, attrs_u32);
        (new_structure, offset)
    }

    /// Set the `unsigned attributes` of `key` in dictionary structure `sid`'s owned
    /// PropertyTable in place, keeping its offset (the `Structure::attributeChange` core
    /// over `PropertyTable::updateAttributeIfExists`, Structure.cpp:1317 /
    /// PropertyTable.h:444). No-op if `key` was never interned or `sid` has no owned table.
    fn change_attributes_in_dictionary(
        &mut self,
        sid: StructureId,
        key: &CorePropertyKey,
        attrs: u32,
    ) {
        if let Some(uid) = self.lookup_property_uid(key) {
            self.structure_table.update_attributes(sid, uid, attrs);
        }
    }

    /// The offset the NEXT property added to structure `sid` would take — the analog
    /// of `Structure::transitionOffset()` peeked ahead, used by the generated-store IC
    /// to validate a planned transition offset. Mirrors PropertyTable::nextOffset
    /// (PropertyOffset.h:136 / PropertyTable.h:471): recycle a freed offset, else the
    /// fresh offset for property number `size()`.
    pub(crate) fn next_property_offset_for_structure(&self, sid: StructureId) -> PropertyOffset {
        if !self.is_live_structure(sid) {
            return PropertyOffset::new(offset_for_property_number(0, INLINE_CAPACITY));
        }
        let cap = self.structure_table.structure(sid).inline_capacity() as i32;
        let mut table = self.structure_table.materialize_property_table(sid);
        PropertyOffset::new(table.next_offset(cap))
    }

    /// Convenience: `next_property_offset_for_structure` for an object value (the
    /// generated-store IC validates a planned offset via the by-structure form; this
    /// object-keyed wrapper is used by the store fidelity tests).
    #[cfg(test)]
    pub(crate) fn next_property_offset(&self, object: RuntimeValue) -> PropertyOffset {
        match self.find(object) {
            Some(cell) => {
                let sid = cell.structure_id;
                self.next_property_offset_for_structure(sid)
            }
            None => PropertyOffset::INVALID,
        }
    }

    /// Faithful `Structure::addPropertyTransition` (Structure.cpp:561) wrapper: returns
    /// the shared successor structure AND the property's offset for a property
    /// ADDITION (a key that does not yet have a named offset in `old`). Two same-shape
    /// objects adding the same `(key, attributes)` from the same `old` converge on ONE
    /// successor (and ONE offset) via the transition table — the monomorphic-IC
    /// guarantee. `is_accessor` (gc-r4 B-i) ORs in the `PropertyAttribute::Accessor`
    /// bit so a data add and an accessor add of the same key key DISTINCT edges.
    /// Symbol keys now key the table too (gc-r4 B-iii, `intern_property_uid` uniques
    /// them); only ARRAY-INDEX strings fall back to the conservative fresh-dictionary
    /// with an invalid offset (their value lives in the butterfly indexed region).
    pub(crate) fn structure_add_property(
        &mut self,
        old: StructureId,
        key: &CorePropertyKey,
        attributes: CorePropertyAttributes,
        is_accessor: bool,
    ) -> (StructureId, PropertyOffset) {
        if !core_property_key_supports_named_property_offset(key) {
            return (
                self.fresh_dictionary_structure(old, None),
                PropertyOffset::INVALID,
            );
        }
        let uid = self.intern_property_uid(key);
        let attributes_u32 = core_attributes_to_u32(attributes, is_accessor);
        let (handle, raw_offset) =
            self.structure_table
                .add_property_transition(old, uid, attributes_u32);
        (handle, PropertyOffset::new(raw_offset))
    }

    /// Mint a fresh per-object (dictionary) structure that carries `old`'s surviving
    /// offsets, with `removed` (if it has a named offset) taken out and its slot freed
    /// for recycle. The conservative fresh-id path for non-PropertyAddition shape
    /// changes (delete / data<->accessor / attribute change); see
    /// `StructureIdTable::create_dictionary_from`.
    pub(crate) fn fresh_dictionary_structure(
        &mut self,
        old: StructureId,
        removed: Option<&CorePropertyKey>,
    ) -> StructureId {
        if !self.is_live_structure(old) {
            return self.allocate_structure_id();
        }
        let removed_uid = removed.and_then(|key| self.lookup_property_uid(key));
        self.structure_table
            .create_dictionary_from(old, removed_uid)
    }

    /// Stable seed-key identity of a stored prototype.
    ///
    /// C++ JSC: the structure's stored prototype is part of structure identity, so
    /// objects with distinct prototypes must seed from distinct root structures.
    /// We map the prototype to its pinned pointer payload bits (durable since cells
    /// are Pin<Box<_>> and never move; this is exactly the key find()/find_mut()
    /// use). Absent and explicit-null prototypes get their own buckets. A prototype
    /// value with no extractable cell payload (should not happen for real
    /// prototypes) folds into Null, conservatively preventing collapse with
    /// cell-prototype siblings.
    ///
    /// FIX 2: this used cell.cell_id, which is unset (CellId::default()) until the
    /// prototype is heap-published, so distinct unpublished prototypes collapsed
    /// into one bucket. The payload bits are unique and stable from allocation.
    pub(crate) fn prototype_identity(
        &self,
        prototype: Option<RuntimeValue>,
    ) -> CorePrototypeIdentity {
        match prototype {
            None => CorePrototypeIdentity::None,
            Some(value) => match value.as_cell().map(|cell| cell.pointer_payload_bits()) {
                Some(payload) => CorePrototypeIdentity::Cell(payload),
                None => CorePrototypeIdentity::Null,
            },
        }
    }

    /// Shared empty-shape ROOT structure for a (kind, prototype) pair.
    ///
    /// C++ JSC: JSGlobalObject hands every fresh object of a given class+prototype the
    /// same empty Structure, from which property additions transition. The Rust
    /// interpreter reconstructs that shared root via structure_seed_roots (the
    /// create_root analog) so sibling objects begin from ONE structure id and their
    /// first add-property transition converges (cross-instance IC hits depend on this).
    pub(crate) fn seed_structure_id(
        &mut self,
        kind: CoreObjectKind,
        prototype: Option<RuntimeValue>,
    ) -> StructureId {
        let identity = self.prototype_identity(prototype);
        if let Some(existing) = self.structure_seed_roots.get(&(kind, identity)).copied() {
            return existing;
        }
        let prototype_pointer = self.prototype_pointer(prototype);
        let id = self.structure_table.create_root(
            prototype_pointer,
            NON_ARRAY,
            0,
            0,
            INLINE_CAPACITY as u8,
        );
        self.structure_seed_roots.insert((kind, identity), id);
        id
    }

    pub(crate) fn snapshot_structure_transition_watchpoints(
        &self,
        requests: &[StructureTransitionWatchpointRequest],
    ) -> Vec<StructureTransitionWatchpointSnapshot> {
        requests
            .iter()
            .map(|request| self.structure_transition_watchpoint_snapshot(*request))
            .collect()
    }

    pub(crate) fn start_structure_transition_watchpoints(
        &mut self,
        requests: &[StructureTransitionWatchpointRequest],
    ) -> Vec<StructureTransitionWatchpointSnapshot> {
        requests
            .iter()
            .map(|request| self.start_structure_transition_watchpoint(*request))
            .collect()
    }

    pub(crate) fn structure_transition_watchpoint_snapshot(
        &self,
        request: StructureTransitionWatchpointRequest,
    ) -> StructureTransitionWatchpointSnapshot {
        let (state, generation, kind) = self
            .structure_transition_watchpoints
            .get(&request.set)
            .map(|record| {
                (
                    record.set.state(),
                    WatchpointGeneration(record.set.generation()),
                    record.set.kind(),
                )
            })
            .unwrap_or((WatchpointState::Clear, WatchpointGeneration(0), None));
        StructureTransitionWatchpointSnapshot {
            set: request.set,
            structure: request.structure,
            state,
            generation,
            kind,
        }
    }

    pub(crate) fn start_structure_transition_watchpoint(
        &mut self,
        request: StructureTransitionWatchpointRequest,
    ) -> StructureTransitionWatchpointSnapshot {
        if let Some(previous_structure) = self
            .structure_transition_watchpoints
            .get(&request.set)
            .map(|record| record.structure)
            .filter(|structure| *structure != request.structure)
        {
            self.remove_structure_transition_watchpoint_reverse_lookup(
                previous_structure,
                request.set,
            );
        }

        let record = self
            .structure_transition_watchpoints
            .entry(request.set)
            .or_insert_with(|| CoreStructureTransitionWatchpointRecord {
                structure: request.structure,
                set: WatchpointSet::default(),
            });
        record.structure = request.structure;
        if record.set.state() != WatchpointState::Invalidated {
            record
                .set
                .start_watching(WatchpointKind::StructureTransition);
            self.add_structure_transition_watchpoint_reverse_lookup(request.structure, request.set);
        }
        self.structure_transition_watchpoint_snapshot(request)
    }

    pub(crate) fn add_structure_transition_watchpoint_reverse_lookup(
        &mut self,
        structure: StructureId,
        set: WatchpointSetId,
    ) {
        let sets = self
            .structure_transition_watchpoints_by_structure
            .entry(structure)
            .or_default();
        if !sets.contains(&set) {
            sets.push(set);
        }
    }

    pub(crate) fn remove_structure_transition_watchpoint_reverse_lookup(
        &mut self,
        structure: StructureId,
        set: WatchpointSetId,
    ) {
        let should_remove = if let Some(sets) = self
            .structure_transition_watchpoints_by_structure
            .get_mut(&structure)
        {
            sets.retain(|candidate| *candidate != set);
            sets.is_empty()
        } else {
            false
        };
        if should_remove {
            self.structure_transition_watchpoints_by_structure
                .remove(&structure);
        }
    }

    pub(crate) fn finish_structure_transition(&mut self, old_structure: StructureId) {
        self.structure_chain_invalidation_events
            .push(StructureChainInvalidationEvent { old_structure });
        self.fire_structure_transition_watchpoints(old_structure);
    }

    pub(crate) fn fire_structure_transition_watchpoints(&mut self, old_structure: StructureId) {
        let Some(set_ids) = self
            .structure_transition_watchpoints_by_structure
            .remove(&old_structure)
        else {
            return;
        };

        let mut fired = Vec::new();
        for set_id in set_ids {
            if fired.contains(&set_id) {
                continue;
            }
            let Some(record) = self.structure_transition_watchpoints.get_mut(&set_id) else {
                continue;
            };
            if record.structure != old_structure
                || record.set.state() != WatchpointState::Watching
                || record.set.kind() != Some(WatchpointKind::StructureTransition)
            {
                continue;
            }
            record.set.invalidate("structure transition");
            fired.push(set_id);
            self.fired_watchpoint_events.push(WatchpointFireEvent {
                set: set_id,
                target: WatchpointTarget::StructureTransition {
                    structure: old_structure,
                },
                generation: WatchpointGeneration(record.set.generation()),
            });
        }
    }

    pub(crate) fn drain_watchpoint_fire_events(&mut self) -> Vec<WatchpointFireEvent> {
        std::mem::take(&mut self.fired_watchpoint_events)
    }

    pub(crate) fn has_pending_structure_chain_invalidation_events(&self) -> bool {
        !self.structure_chain_invalidation_events.is_empty()
    }

    pub(crate) fn drain_structure_chain_invalidation_events(
        &mut self,
    ) -> Vec<StructureChainInvalidationEvent> {
        std::mem::take(&mut self.structure_chain_invalidation_events)
    }

    // P3 bridge entry point that gives interpreter-owned object payloads a
    // checked heap identity before roots or barriers publish those cells.
    pub(crate) fn bind_object_to_heap(
        &mut self,
        heap: &mut Heap,
        value: RuntimeValue,
    ) -> Result<CellId, ExecutionError> {
        let payload = value
            .as_cell()
            .map(|cell| cell.pointer_payload_bits())
            .ok_or(ExecutionError::ExpectedObject)?;
        let Some(cell) = self.find_mut(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if let Some(existing) = heap.cell_for_payload(payload) {
            heap.publish_cell(existing)?;
            cell.cell_id = existing;
            return Ok(existing);
        }
        if cell.cell_id != CellId::default() {
            heap.bind_cell_payload(cell.cell_id, payload)?;
            heap.publish_cell(cell.cell_id)?;
            return Ok(cell.cell_id);
        }

        let cell_id = allocate_object_interpreter_cell_id(heap)?;
        heap.bind_cell_payload(cell_id, payload)?;
        heap.publish_cell(cell_id)?;
        cell.cell_id = cell_id;
        Ok(cell_id)
    }

    pub(crate) fn assign_object_heap_cell(
        &mut self,
        heap: &mut Heap,
        value: RuntimeValue,
        cell_id: CellId,
    ) -> Result<(), ExecutionError> {
        let Some(cell) = self.find_mut(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if cell.cell_id != CellId::default() && cell.cell_id != cell_id {
            return Err(ExecutionError::UnknownObject);
        }
        heap.publish_cell(cell_id)?;
        cell.cell_id = cell_id;
        Ok(())
    }

    pub(crate) fn resolve_value_store_target(
        &mut self,
        heap: &mut Heap,
        value: RuntimeValue,
    ) -> Result<Option<CellId>, ExecutionError> {
        let Some(payload) = value_cell_payload(value) else {
            return Ok(None);
        };
        if let Some(cell_id) = heap.cell_for_payload(payload) {
            heap.publish_cell(cell_id)?;
            return Ok(Some(cell_id));
        }
        if self.find(value).is_some() {
            return self.bind_object_to_heap(heap, value).map(Some);
        }
        Ok(None)
    }

    pub(crate) fn apply_value_store_write_barrier(
        &mut self,
        heap: &mut Heap,
        owner: RuntimeValue,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let owner = self.bind_object_to_heap(heap, owner)?;
        let target = self.resolve_value_store_target(heap, value)?;
        // C++ HeapInlines.h:106 reads the OWNER's real cellState (`from->cellState()`),
        // never a fabricated constant. The heap does not yet track per-cell CellState, so
        // every never-collected owner/target is DefinitelyWhite (eden/fresh,
        // heap/CellState.h:37-38). A white owner is outside barrierThreshold == 0
        // (Heap.cpp:3320 while not fenced), so the barrier classifies as NotRequired and the
        // slow path stores nothing while no collector runs. This formerly hardcoded
        // owner=PossiblyBlack / target=PossiblyGrey, which forced Required(MarkingBarrier)
        // plus a remembered-set entry on every store — the measured per-store barrier tax.
        let owner_state = CellState::DefinitelyWhite;
        let target_state = target.map(|_| CellState::DefinitelyWhite);
        heap.apply_write_barrier(WriteBarrierApplicationRequest {
            owner,
            target,
            context: BarrierWriteContext::store(BarrierFieldKind::Value, owner_state, target_state),
            authority: BarrierMutationAuthority::MutatorFieldWrite,
            owner_is_published: true,
        })?;
        Ok(())
    }

    pub(crate) fn get_property(
        &self,
        object: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Result<CorePropertyGet, ExecutionError> {
        self.get_property_from_prototype_chain(object, key)
    }

    pub(crate) fn get_property_with_lookup_record(
        &self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        site: CorePropertyLookupSite,
    ) -> Result<(CorePropertyGet, CorePropertyLookupRecord), ExecutionError> {
        self.get_property_from_prototype_chain_with_lookup_record(object, key, site)
    }

    pub(crate) fn has_property(
        &self,
        mut object: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Result<bool, ExecutionError> {
        loop {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            if self.own_property_from_shape(cell, key).is_some() {
                return Ok(true);
            }
            if cell.kind == CoreObjectKind::Array && key.is_string("length") {
                return Ok(true);
            }
            // gc-r4 B-iv: array-index-named data properties live in indexed butterfly
            // storage for EVERY object kind (not just arrays).
            if cell.kind != CoreObjectKind::Uint8Array {
                if let Some(index) = key_array_index(key) {
                    if self.butterfly_elem_get(cell.butterfly, index).is_some() {
                        return Ok(true);
                    }
                }
            }
            if cell.kind == CoreObjectKind::Uint8Array {
                if let Some(index) = key_array_index(key) {
                    if index < cell.view_length {
                        return Ok(true);
                    }
                }
            }
            let Some(prototype) = cell.prototype else {
                return Ok(false);
            };
            object = prototype;
        }
    }

    pub(crate) fn has_property_with_lookup_record(
        &self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        site: CorePropertyLookupSite,
    ) -> Result<(bool, CorePropertyLookupRecord), ExecutionError> {
        let Some(base_cell) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let base_structure = Some(base_cell.structure_id);

        let mut current = object;
        let mut prototype_depth = 0;
        let mut chain = Vec::new();
        loop {
            let Some(cell) = self.find(current) else {
                return Err(ExecutionError::ExpectedObject);
            };
            chain.push(CorePropertyLookupChainEntry {
                object: current,
                structure: cell.structure_id,
            });
            if let Some(property) = self.own_property_from_shape(cell, key) {
                let classification = match property.kind {
                    CorePropertyKind::Data(_) if prototype_depth == 0 => {
                        CorePropertyLookupClassification::OwnData
                    }
                    CorePropertyKind::Data(_) => CorePropertyLookupClassification::PrototypeData,
                    CorePropertyKind::Accessor {
                        getter: Some(_), ..
                    } if prototype_depth == 0 => {
                        CorePropertyLookupClassification::OwnAccessorGetter
                    }
                    CorePropertyKind::Accessor {
                        getter: Some(_), ..
                    } => CorePropertyLookupClassification::PrototypeAccessorGetter,
                    CorePropertyKind::Accessor { getter: None, .. } => {
                        CorePropertyLookupClassification::AccessorWithoutGetter
                    }
                };
                // Capture the holding cell's structure, then read the offset from its
                // Structure::PropertyTable (the offset authority) once the cell borrow
                // ends, so the store's structure_table can be consulted.
                let found_structure = cell.structure_id;
                let mut record = CorePropertyLookupRecord::from_has_property_lookup(
                    site,
                    object,
                    key,
                    Some(current),
                    prototype_depth,
                    classification,
                    true,
                );
                record.base_structure = base_structure;
                record.chain = chain.clone();
                record.offset = self.structure_offset(found_structure, key);
                return Ok((true, record));
            }
            if cell.kind == CoreObjectKind::Array {
                let found = if key.is_string("length") {
                    true
                } else {
                    key_array_index(key).is_some_and(|index| {
                        self.butterfly_elem_get(cell.butterfly, index).is_some()
                    })
                };
                if found {
                    let mut record = CorePropertyLookupRecord::from_has_property_lookup(
                        site,
                        object,
                        key,
                        Some(current),
                        prototype_depth,
                        CorePropertyLookupClassification::IndexedOrTypedArray,
                        true,
                    );
                    record.base_structure = base_structure;
                    record.chain = chain.clone();
                    if key_array_index(key).is_some() {
                        record.access_case_kind = Some(AccessCaseKind::IndexedArrayStorageInHit);
                    }
                    return Ok((true, record));
                }
            }
            if cell.kind == CoreObjectKind::Uint8Array {
                if key_array_index(key).is_some_and(|index| index < cell.view_length) {
                    let mut record = CorePropertyLookupRecord::from_has_property_lookup(
                        site,
                        object,
                        key,
                        Some(current),
                        prototype_depth,
                        CorePropertyLookupClassification::IndexedOrTypedArray,
                        true,
                    );
                    record.base_structure = base_structure;
                    record.chain = chain.clone();
                    record.access_case_kind = Some(AccessCaseKind::IndexedTypedArrayUint8In);
                    return Ok((true, record));
                }
            }
            // gc-r4 B-iv: a NON-array object's array-index data property also lives in
            // indexed butterfly storage (arrays handled above, typed arrays excluded).
            if cell.kind != CoreObjectKind::Array && cell.kind != CoreObjectKind::Uint8Array {
                if key_array_index(key)
                    .is_some_and(|index| self.butterfly_elem_get(cell.butterfly, index).is_some())
                {
                    let mut record = CorePropertyLookupRecord::from_has_property_lookup(
                        site,
                        object,
                        key,
                        Some(current),
                        prototype_depth,
                        CorePropertyLookupClassification::IndexedOrTypedArray,
                        true,
                    );
                    record.base_structure = base_structure;
                    record.chain = chain.clone();
                    record.access_case_kind = Some(AccessCaseKind::IndexedArrayStorageInHit);
                    return Ok((true, record));
                }
            }
            let Some(prototype) = cell.prototype else {
                let mut record = CorePropertyLookupRecord::from_has_property_lookup(
                    site,
                    object,
                    key,
                    None,
                    prototype_depth,
                    CorePropertyLookupClassification::Missing,
                    false,
                );
                record.base_structure = base_structure;
                record.chain = chain.clone();
                if site.opcode == Some(CoreOpcode::InByVal)
                    && key_array_index(key).is_some()
                    && base_cell.kind == CoreObjectKind::Ordinary
                {
                    record.access_case_kind = Some(AccessCaseKind::IndexedNoIndexingInMiss);
                }
                return Ok((false, record));
            };
            current = prototype;
            prototype_depth = prototype_depth.saturating_add(1);
        }
    }

    pub(crate) fn property_store_snapshot(
        &self,
        object: RuntimeValue,
        key: &CorePropertyKey,
    ) -> CorePropertyStoreSnapshot {
        let Some(cell) = self.find(object) else {
            return CorePropertyStoreSnapshot {
                base_object: None,
                base_structure: None,
                has_own_property: false,
                has_own_data_property: false,
                is_indexed_or_typed_array_store: false,
                is_dense_array_indexed_store: false,
                has_own_indexed_element: false,
                offset: None,
            };
        };
        let own_property = self.own_property_from_shape(cell, key);
        let has_own_property = own_property.is_some();
        let has_own_data_property =
            own_property.is_some_and(|property| matches!(property.kind, CorePropertyKind::Data(_)));
        let indexed_key = key_array_index(key);
        let is_dense_array_indexed_store =
            matches!(cell.kind, CoreObjectKind::Array) && indexed_key.is_some();
        let has_own_indexed_element = indexed_key.is_some_and(|index| {
            matches!(cell.kind, CoreObjectKind::Array)
                && self.butterfly_elem_get(cell.butterfly, index).is_some()
        });
        let is_indexed_or_typed_array_store =
            is_dense_array_indexed_store || matches!(cell.kind, CoreObjectKind::Uint8Array);
        // Capture the structure, then read the offset from its PropertyTable once the
        // cell borrow ends so the store's structure_table can be consulted.
        let structure = cell.structure_id;
        let offset = self.structure_offset(structure, key);
        CorePropertyStoreSnapshot {
            base_object: Some(object),
            base_structure: Some(structure),
            has_own_property,
            has_own_data_property,
            is_indexed_or_typed_array_store,
            is_dense_array_indexed_store,
            has_own_indexed_element,
            offset,
        }
    }

    /// C++ JSC `JSArray::put` -> `setLength` (runtime/JSArray.cpp:317-325, 1237).
    /// `array.length = v` computes `newLength = ToUint32(v)`, throws a catchable
    /// `RangeError("Invalid array length")` when `ToNumber(v) != newLength`, and
    /// otherwise resizes the element vector — truncating elements at or above
    /// `newLength`, or hole-extending with empty slots. Since the Rust array model
    /// stores `length == elements.len()`, that resize IS the setLength.
    fn set_array_length(&mut self, object: RuntimeValue, value: RuntimeValue) -> ArrayLengthPut {
        // ToNumber for the value kinds the engine's `to_number_value` supports
        // (number/boolean/null/undefined). String/object/symbol/bigint need the
        // full ToNumber/ToPrimitive path that lives in the interpreter; defer them.
        let number = match value.as_number() {
            Some(NumberValue::Int32(value)) => f64::from(value),
            Some(NumberValue::DoubleBits(bits)) => bits.to_f64(),
            None => match value.kind() {
                ValueKind::Boolean => {
                    if value.as_bool() == Some(true) {
                        1.0
                    } else {
                        0.0
                    }
                }
                ValueKind::Null => 0.0,
                ValueKind::Undefined => f64::NAN,
                _ => return ArrayLengthPut::NeedsGenericPut,
            },
        };
        // `ToUint32(number) == number` exactly when `number` is a non-negative
        // integer in [0, 2^32 - 1]; every other value (NaN, negative, fractional,
        // >= 2^32) is the `createRangeError("Invalid array length")` case.
        const MAX_ARRAY_LENGTH: f64 = 4_294_967_295.0;
        if !(number.is_finite()
            && number >= 0.0
            && number <= MAX_ARRAY_LENGTH
            && number.fract() == 0.0)
        {
            return ArrayLengthPut::Invalid;
        }
        let new_length = number as usize;
        // Truncate (drop tail) or hole-extend (push empty slots), matching
        // `JSArray::setLength` clearing/`ensureLength` behavior; the indexed storage
        // is the store-owned butterfly slab, reached by the cell's handle.
        if let Some(handle) = self.find(object).map(|cell| cell.butterfly) {
            self.butterfly_elem_resize(handle, new_length);
        }
        ArrayLengthPut::Resized
    }

    pub(crate) fn put(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<CorePropertyPut, ExecutionError> {
        // C++ JSC `JSArray::put` (runtime/JSArray.cpp:307): `array.length = v` is
        // the dedicated setLength path, NOT an ordinary named-property store. The
        // Rust array model keeps `length == elements.len()` (see `get_own_property`
        // / `array_length`), so without this the assignment fell through to
        // `define_data_property` and stored a *shadowed* "length" data property
        // that `get_own_property` then ignored — making `arr.length = N` (and the
        // common `arr.length = 0` clear / `arr[arr.length] = x` regrow idiom) a
        // silent no-op. This runs before the generic own/prototype property
        // machinery so a length write is always the setLength semantics.
        if key.is_string("length")
            && self
                .find(object)
                .is_some_and(|cell| cell.kind == CoreObjectKind::Array)
        {
            match self.set_array_length(object, value) {
                ArrayLengthPut::Resized => return Ok(CorePropertyPut::Stored),
                ArrayLengthPut::Invalid => return Ok(CorePropertyPut::InvalidArrayLength),
                ArrayLengthPut::NeedsGenericPut => {}
            }
        }
        let Some(receiver) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if let Some(property) = self.own_property_from_shape(receiver, key) {
            return match property.kind {
                CorePropertyKind::Accessor {
                    setter: Some(setter),
                    ..
                } => Ok(CorePropertyPut::Setter(setter)),
                CorePropertyKind::Accessor { setter: None, .. } => {
                    Ok(CorePropertyPut::IgnoredGetterOnly)
                }
                CorePropertyKind::Data(_) if !property.attributes.writable => {
                    Ok(CorePropertyPut::IgnoredReadOnly)
                }
                CorePropertyKind::Data(_) => {
                    self.set_data_own_with_write_barrier(heap, object, key, value)?;
                    Ok(CorePropertyPut::Stored)
                }
            };
        }

        let receiver_kind = receiver.kind;
        let receiver_prototype = receiver.prototype;
        let has_own_array_element = if receiver_kind == CoreObjectKind::Array {
            key_array_index(key)
                .is_some_and(|index| self.butterfly_elem_get(receiver.butterfly, index).is_some())
        } else {
            false
        };
        let array_index = if receiver_kind == CoreObjectKind::Array {
            key_array_index(key)
        } else {
            None
        };
        if receiver_kind == CoreObjectKind::Uint8Array {
            if let Some(index) = key_array_index(key) {
                self.write_typed_element(object, index, typed_array_store_input_number(value)?)?;
                return Ok(CorePropertyPut::Stored);
            }
        }
        if let (Some(index), true) = (array_index, has_own_array_element) {
            self.put_array_element_with_write_barrier(heap, object, index, value)?;
            return Ok(CorePropertyPut::Stored);
        }

        let mut current = receiver_prototype;
        while let Some(prototype) = current {
            let Some(cell) = self.find(prototype) else {
                return Err(ExecutionError::ExpectedObject);
            };
            if let Some(property) = self.own_property_from_shape(cell, key) {
                match property.kind {
                    CorePropertyKind::Accessor {
                        setter: Some(setter),
                        ..
                    } => return Ok(CorePropertyPut::Setter(setter)),
                    CorePropertyKind::Accessor { setter: None, .. } => {
                        return Ok(CorePropertyPut::IgnoredGetterOnly);
                    }
                    CorePropertyKind::Data(_) if !property.attributes.writable => {
                        return Ok(CorePropertyPut::IgnoredReadOnly);
                    }
                    CorePropertyKind::Data(_) => break,
                }
            }
            current = cell.prototype;
        }

        if let Some(index) = array_index {
            self.put_array_element_with_write_barrier(heap, object, index, value)?;
            return Ok(CorePropertyPut::Stored);
        }

        self.define_data_property_with_write_barrier(
            heap,
            object,
            key,
            value,
            CorePropertyAttributes::DATA_DEFAULT,
        )?;
        Ok(CorePropertyPut::Stored)
    }

    /// C++ JSC `JSValue::putToPrimitive` (runtime/JSCJSValue.cpp:217), the put
    /// half of the autobox path that mirrors the get-side
    /// `synthesizePrototype` lookup already used for `(42).toString()` and
    /// `'x'.length`. `synthesized_prototype` is the primitive's
    /// Number/String/Boolean/Symbol/BigInt `.prototype`; we walk its prototype
    /// chain looking for a setter, exactly as `JSObject::putInlineSlow`
    /// (JSObject.cpp:831) walks from `obj = this` upward. An accessor with a
    /// setter is reported so the caller can invoke it with the primitive as
    /// receiver (`slot.thisValue()`); a getter-only accessor or a read-only
    /// data property, or reaching the end of the chain, is a no-op — in sloppy
    /// mode `definePropertyOnReceiver` (JSObject.cpp:973) silently returns false
    /// for a non-object receiver. We never store onto the prototype itself,
    /// because the receiver is the primitive, not the prototype object.
    pub(crate) fn find_setter_for_put_to_primitive(
        &self,
        synthesized_prototype: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Result<PutToPrimitiveOutcome, ExecutionError> {
        let mut current = Some(synthesized_prototype);
        while let Some(object) = current {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            if let Some(property) = self.own_property_from_shape(cell, key) {
                return Ok(match property.kind {
                    CorePropertyKind::Accessor {
                        setter: Some(setter),
                        ..
                    } => PutToPrimitiveOutcome::Setter(setter),
                    // Getter-only accessor or read-only data property: sloppy
                    // no-op (strict TypeError deferred). Writable data property:
                    // also a no-op here, since the receiver is the primitive and
                    // `definePropertyOnReceiver` cannot create a data property on
                    // a non-object receiver.
                    CorePropertyKind::Accessor { setter: None, .. } | CorePropertyKind::Data(_) => {
                        PutToPrimitiveOutcome::NoOp
                    }
                });
            }
            current = cell.prototype;
        }
        Ok(PutToPrimitiveOutcome::NoOp)
    }

    pub(crate) fn get_own_property(
        &self,
        object_value: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Result<Option<CoreProperty>, ExecutionError> {
        let Some(object) = self.find(object_value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind == CoreObjectKind::Array && key.is_string("length") {
            return Ok(Some(CoreProperty {
                kind: CorePropertyKind::Data(RuntimeValue::from_i32(
                    self.butterfly_elem_len(object.butterfly)
                        .try_into()
                        .unwrap_or(i32::MAX),
                )),
                attributes: CorePropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: false,
                },
            }));
        }
        if let Some(property) = self.own_property_from_shape(object, key) {
            return Ok(Some(property));
        }
        // gc-r4 B-iv: an array-index-named data property is served from the INDEXED
        // butterfly region for EVERY object kind (any JS object may carry indexed data
        // properties). Typed arrays use their own typed-element path below.
        if object.kind != CoreObjectKind::Uint8Array {
            if let Some(index) = key_array_index(key) {
                if let Some(value) = self.butterfly_elem_get(object.butterfly, index) {
                    return Ok(Some(CoreProperty {
                        kind: CorePropertyKind::Data(value),
                        attributes: CorePropertyAttributes::DATA_DEFAULT,
                    }));
                }
            }
        }
        if object.kind == CoreObjectKind::Uint8Array {
            if let Some(index) = key_array_index(key) {
                if let Some(value) = self.read_typed_element(object_value, index)? {
                    return Ok(Some(CoreProperty {
                        kind: CorePropertyKind::Data(value),
                        attributes: CorePropertyAttributes {
                            writable: true,
                            enumerable: true,
                            configurable: false,
                        },
                    }));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn has_own_property(
        &self,
        object: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Result<bool, ExecutionError> {
        self.get_own_property(object, key)
            .map(|property| property.is_some())
    }

    pub(crate) fn own_enumerable_string_property_names(
        &self,
        object: RuntimeValue,
    ) -> Result<Vec<String>, ExecutionError> {
        Ok(self
            .own_string_property_names_with_enumerability(object)?
            .into_iter()
            .filter_map(|(name, enumerable)| enumerable.then_some(name))
            .collect())
    }

    pub(crate) fn enumerable_string_property_names_for_in(
        &self,
        mut object: RuntimeValue,
    ) -> Result<Vec<String>, ExecutionError> {
        let mut names = Vec::new();
        let mut visited = HashSet::new();
        loop {
            for (name, enumerable) in self.own_string_property_names_with_enumerability(object)? {
                if visited.insert(name.clone()) && enumerable {
                    names.push(name);
                }
            }
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let Some(prototype) = cell.prototype else {
                return Ok(names);
            };
            object = prototype;
        }
    }

    pub(crate) fn own_string_property_names_with_enumerability(
        &self,
        object: RuntimeValue,
    ) -> Result<Vec<(String, bool)>, ExecutionError> {
        let Some(object) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let mut index_names = BTreeSet::new();
        // gc-r4 B-iv: indexed-butterfly elements enumerate (numeric order, first) for EVERY
        // object kind, not just arrays (any object may carry indexed data properties).
        if object.kind != CoreObjectKind::Uint8Array {
            for (index, value) in self.butterfly_elements(object.butterfly).iter().enumerate() {
                if value.is_some() {
                    index_names.insert(index);
                }
            }
        } else {
            for index in 0..object.view_length {
                index_names.insert(index);
            }
        }

        let mut string_names = Vec::new();
        let mut hidden_index_names = BTreeSet::new();
        // gc-r4 B-iv: enumeration order + attributes come from the Structure's
        // PropertyTable entry order (Structure::getPropertyNamesFromStructure,
        // Structure.cpp:1326), not the deleted per-cell `property_order`.
        for (key, _offset, attrs) in self.structure_property_keys(object.structure_id) {
            let Some(name) = key_string_name(&key) else {
                continue;
            };
            let enumerable = core_attributes_from_u32(attrs).enumerable;
            if let Some(index) = parse_array_index_name(name) {
                if enumerable {
                    index_names.insert(index);
                    hidden_index_names.remove(&index);
                } else {
                    index_names.remove(&index);
                    hidden_index_names.insert(index);
                }
            } else {
                string_names.push((name.to_owned(), enumerable));
            }
        }

        let mut names = index_names
            .into_iter()
            .map(|index| (index.to_string(), true))
            .collect::<Vec<_>>();
        names.extend(
            hidden_index_names
                .into_iter()
                .map(|index| (index.to_string(), false)),
        );
        names.extend(string_names);
        Ok(names)
    }

    pub(crate) fn own_property_keys(
        &self,
        object: RuntimeValue,
    ) -> Result<Vec<CorePropertyKey>, ExecutionError> {
        let Some(object) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let mut keys = Vec::new();
        let mut seen = HashSet::new();
        // gc-r4 B-iv: indexed-butterfly elements (numeric order, first) enumerate for EVERY
        // object kind, not just arrays. Arrays then append the exotic `length`.
        if object.kind != CoreObjectKind::Uint8Array {
            for (index, value) in self.butterfly_elements(object.butterfly).iter().enumerate() {
                if value.is_some() {
                    let key = CorePropertyKey::String(index.to_string());
                    if seen.insert(key.clone()) {
                        keys.push(key);
                    }
                }
            }
        }
        if object.kind == CoreObjectKind::Array {
            let length = CorePropertyKey::String("length".into());
            seen.insert(length.clone());
            keys.push(length);
        }
        if object.kind == CoreObjectKind::Uint8Array {
            for index in 0..object.view_length {
                let key = CorePropertyKey::String(index.to_string());
                seen.insert(key.clone());
                keys.push(key);
            }
        }
        // gc-r4 B-iv: named own-key order comes from the Structure's PropertyTable entry
        // order (the deleted per-cell `property_order` was a redundant mirror of it).
        for (key, _offset, _attrs) in self.structure_property_keys(object.structure_id) {
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
        Ok(keys)
    }

    /// Faithful indexed-storage routing (gc-r4 B-iv): an array-index-named property lives
    /// in the object's INDEXED butterfly storage (C++ contiguous/ArrayStorage), NOT the
    /// named PropertyTable — it has no named offset, so the named-property IC never arms
    /// for it. If `key` is an array index, write `value` into the butterfly `elements`
    /// side and return `Some(())`; otherwise `None` (the caller takes the named path).
    /// Applies to EVERY object kind (any JS object may carry indexed data properties);
    /// typed arrays use their own typed-element store and are routed earlier by callers,
    /// so they are excluded here.
    ///
    /// DIVERGENCE: a data index property always takes DATA_DEFAULT element semantics here;
    /// custom attributes / accessors on integer keys (JSC's ArrayStorage descriptors) are
    /// not modeled — vanishingly rare and absent from Octane.
    fn route_array_index_to_elements(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Option<()> {
        let (kind, handle) = self.find(object).map(|cell| (cell.kind, cell.butterfly))?;
        if kind == CoreObjectKind::Uint8Array {
            return None;
        }
        let index = key_array_index(key)?;
        self.butterfly_elem_put(handle, index, value);
        Some(())
    }

    pub(crate) fn set_data_own(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        if self
            .route_array_index_to_elements(object, key, value)
            .is_some()
        {
            return Ok(());
        }
        // C++ JSC: a pure property addition routes through Structure::addPropertyTransition
        // so the offset comes from the per-shape Structure::PropertyTable and same-shape
        // siblings share one successor structure + offset. An accessor->data kind change is
        // an offset-stable attributeChangeTransition (Structure.cpp:806). A same-shape value
        // replace keeps the structure and rewrites the existing offset slot. gc-r4 B-iv:
        // the offset+attributes (the value authority alongside the butterfly) come from the
        // Structure; the per-cell `properties` HashMap is gone.
        let (old_structure, current) = {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            (cell.structure_id, self.own_property_from_shape(cell, key))
        };
        let (new_structure, offset, shape_changed) = match current {
            None => {
                let (ns, off) = self.structure_add_property(
                    old_structure,
                    key,
                    CorePropertyAttributes::DATA_DEFAULT,
                    false,
                );
                (ns, off, true)
            }
            Some(current) if matches!(current.kind, CorePropertyKind::Accessor { .. }) => {
                // accessor -> data: keep the prior attributes (the pre-flip code rewrote
                // only the kind to Data), offset preserved.
                let (ns, off) =
                    self.convert_property_in_place(old_structure, key, current.attributes, false);
                (ns, off, true)
            }
            Some(_) => (
                old_structure,
                self.structure_offset(old_structure, key)
                    .unwrap_or(PropertyOffset::INVALID),
                false,
            ),
        };
        // putDirectOffset analog: write the value at the structure-assigned offset into
        // the store-owned butterfly slab (copy the handle out under the cell borrow,
        // then write the slab via the &mut self butterfly API).
        let handle = self.find_mut(object).map(|object_cell| {
            if shape_changed {
                object_cell.structure_id = new_structure;
            }
            object_cell.butterfly
        });
        if let Some(handle) = handle {
            self.butterfly_prop_put(handle, offset, value);
        }
        if shape_changed {
            self.finish_structure_transition(old_structure);
        }
        Ok(())
    }

    pub(crate) fn set_data_own_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.apply_value_store_write_barrier(heap, object, value)?;
        self.set_data_own(object, key, value)
    }

    pub(crate) fn put_data_own_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.apply_value_store_write_barrier(heap, object, value)?;
        self.put_data_own(object, key, value)
    }

    pub(crate) fn put_data_own(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        if self
            .route_array_index_to_elements(object, key, value)
            .is_some()
        {
            return Ok(());
        }
        // C++ JSC: a property ADDITION routes through Structure::addPropertyTransition so
        // the offset comes from the per-shape PropertyTable and siblings converge. An
        // accessor->data conversion or an attribute change on an existing data property
        // keeps the property's offset and is an offset-stable attributeChangeTransition
        // (Structure.cpp:806), not a shareable add. A same-shape value replace keeps both.
        // gc-r4 B-iv: the shape (offset+attributes) is the value authority alongside the
        // butterfly; the per-cell `properties` HashMap is gone.
        let (old_structure, current) = {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            (cell.structure_id, self.own_property_from_shape(cell, key))
        };
        let (new_structure, offset, shape_changed) = match current {
            None => {
                let (ns, off) = self.structure_add_property(
                    old_structure,
                    key,
                    CorePropertyAttributes::DATA_DEFAULT,
                    false,
                );
                (ns, off, true)
            }
            Some(current)
                if matches!(current.kind, CorePropertyKind::Data(_))
                    && current.attributes == CorePropertyAttributes::DATA_DEFAULT =>
            {
                // Same-shape value replace: keep structure + offset.
                (
                    old_structure,
                    self.structure_offset(old_structure, key)
                        .unwrap_or(PropertyOffset::INVALID),
                    false,
                )
            }
            Some(_) => {
                // accessor->data, or data attribute change to DATA_DEFAULT: offset-stable
                // attributeChange on a per-object dictionary.
                let (ns, off) = self.convert_property_in_place(
                    old_structure,
                    key,
                    CorePropertyAttributes::DATA_DEFAULT,
                    false,
                );
                (ns, off, true)
            }
        };
        // putDirectOffset analog (into the store-owned butterfly slab).
        let handle = self.find_mut(object).map(|object_cell| {
            if shape_changed {
                object_cell.structure_id = new_structure;
            }
            object_cell.butterfly
        });
        if let Some(handle) = handle {
            self.butterfly_prop_put(handle, offset, value);
        }
        if shape_changed {
            self.finish_structure_transition(old_structure);
        }
        Ok(())
    }

    pub(crate) fn define_data_property(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
        attributes: CorePropertyAttributes,
    ) -> Result<bool, ExecutionError> {
        // gc-r4 B-iv: an array-index-named data property lives in indexed butterfly
        // storage (DATA_DEFAULT semantics), never the named table — route it there. Custom
        // attributes on integer keys are not modeled (see route_array_index_to_elements).
        if self
            .route_array_index_to_elements(object, key, value)
            .is_some()
        {
            return Ok(true);
        }
        // C++ JSC: defining a brand-new property is a property-addition transition
        // keyed by (uid, attributes) (StructureTransitionTable), so siblings defined
        // with the same key+attributes share a structure id. Redefining an existing
        // property (kind or attribute change) is an offset-stable attributeChangeTransition
        // (Structure.cpp:806). gc-r4 B-iv: the existing property + its attributes come from
        // the Structure (the offset/attribute authority), not the deleted HashMap.
        let (old_structure, current) = {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            (cell.structure_id, self.own_property_from_shape(cell, key))
        };
        if let Some(current) = current {
            if !current.attributes.configurable {
                if attributes.configurable || attributes.enumerable != current.attributes.enumerable
                {
                    return Ok(false);
                }
                match current.kind {
                    CorePropertyKind::Accessor { .. } => return Ok(false),
                    CorePropertyKind::Data(current_value) => {
                        if !current.attributes.writable
                            && (attributes.writable || current_value != value)
                        {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        let (new_structure, offset, shape_changed) = match current {
            None => {
                // Brand-new property: a fresh offset via the (uid, attributes)-keyed
                // (shareable) add-property transition.
                let (ns, off) = self.structure_add_property(old_structure, key, attributes, false);
                (ns, off, true)
            }
            Some(current)
                if matches!(current.kind, CorePropertyKind::Data(_))
                    && current.attributes == attributes =>
            {
                // Same data kind + attributes: value-only replace, keep structure + offset.
                (
                    old_structure,
                    self.structure_offset(old_structure, key)
                        .unwrap_or(PropertyOffset::INVALID),
                    false,
                )
            }
            Some(_) => {
                // accessor->data, or data attribute change: offset-stable attributeChange.
                let (ns, off) =
                    self.convert_property_in_place(old_structure, key, attributes, false);
                (ns, off, true)
            }
        };
        // putDirectOffset analog (into the store-owned butterfly slab).
        let handle = self.find_mut(object).map(|object_cell| {
            if shape_changed {
                object_cell.structure_id = new_structure;
            }
            object_cell.butterfly
        });
        if let Some(handle) = handle {
            self.butterfly_prop_put(handle, offset, value);
        }
        if shape_changed {
            self.finish_structure_transition(old_structure);
        }
        Ok(true)
    }

    pub(crate) fn define_data_property_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
        attributes: CorePropertyAttributes,
    ) -> Result<bool, ExecutionError> {
        self.apply_value_store_write_barrier(heap, object, value)?;
        self.define_data_property(object, key, value, attributes)
    }

    pub(crate) fn delete_property(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Result<bool, ExecutionError> {
        // C++ JSC: deleting a named property is a removePropertyTransition / dictionary
        // transition (Structure.cpp) — the property's offset is freed into the
        // PropertyTable's deleted-offset recycle stack and the surviving offsets are
        // preserved. The dictionary transition KINDS are not yet ported, so this takes
        // the conservative fresh per-object dictionary that carries the surviving
        // offsets with the deleted key taken out (offset freed for recycle).
        // gc-r4 B-iv: presence + configurability come from the Structure (offset/attribute
        // authority); the per-cell `properties` HashMap is gone. Indexed (array) element
        // keys are not named-table entries — they are served from the indexed butterfly
        // region and cleared separately below.
        let (old_structure, current, kind, butterfly, view_length) = {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            (
                cell.structure_id,
                self.own_property_from_shape(cell, key),
                cell.kind,
                cell.butterfly,
                cell.view_length,
            )
        };
        if current.is_some_and(|property| !property.attributes.configurable) {
            return Ok(false);
        }
        if kind == CoreObjectKind::Uint8Array {
            if let Some(index) = key_array_index(key) {
                if index < view_length {
                    return Ok(false);
                }
            }
        }
        // `delete obj[i]`: punch a hole in the indexed butterfly storage. gc-r4 B-iv:
        // applies to EVERY object kind (typed arrays handled/rejected above).
        let array_clear = if kind != CoreObjectKind::Uint8Array {
            key_array_index(key).map(|index| (butterfly, index))
        } else {
            None
        };
        let removed = current.is_some();
        if let Some((handle, index)) = array_clear {
            self.butterfly_elem_clear(handle, index);
        }
        if removed {
            // Free the deleted property's storage slot: `fresh_dictionary_structure` takes
            // the key out of the new dictionary's PropertyTable and pushes its offset onto
            // the table's own `m_deletedOffsets` recycle stack (the faithful owner of
            // recycling — the vestigial per-cell `deleted_offsets` is gone). The slab slot
            // is cleared after the cell borrow releases.
            let removed_offset = self.structure_offset(old_structure, key);
            let new_structure = self.fresh_dictionary_structure(old_structure, Some(key));
            let handle = self.find_mut(object).map(|object_cell| {
                object_cell.structure_id = new_structure;
                object_cell.butterfly
            });
            if let (Some(handle), Some(offset)) = (handle, removed_offset) {
                self.butterfly_prop_clear(handle, offset);
            }
            self.finish_structure_transition(old_structure);
        }
        Ok(true)
    }

    /// Install an accessor into the Structure + butterfly — the gc-r4 B-iv single value
    /// authority (the per-cell `properties` HashMap is gone):
    ///   - a FRESH-key accessor (`is_addition`) takes a faithful `addPropertyTransition`
    ///     carrying the `PropertyAttribute::Accessor` bit (so a data add and an accessor
    ///     add of the same key key DISTINCT transition edges -> distinct structures, and
    ///     same-shape siblings converge);
    ///   - an EXISTING-key change (in-place data<->accessor CONVERSION or an accessor
    ///     getter/setter UPDATE) is an offset-STABLE `attributeChangeTransition`
    ///     (Structure.cpp:806) via `convert_property_in_place`, which KEEPS the property's
    ///     offset and just stamps the Accessor attributes — pre-B-iv this freed/removed the
    ///     offset (`fresh_dictionary_structure(old, Some(key))`), which after the flip would
    ///     make the property VANISH.
    /// In BOTH cases a fresh GetterSetter cell (B-ii) holds the merged getter/setter and
    /// `from_cell(getter_setter)` is written into the structure-assigned butterfly slot,
    /// exactly as C++ stores a `GetterSetter*` at the property's offset. Called ONLY when
    /// the caller's `shape_changed` holds, so an idempotent redefine does no churn.
    fn install_accessor_dual_write(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        old_structure: StructureId,
        attributes: CorePropertyAttributes,
        getter: Option<RuntimeValue>,
        setter: Option<RuntimeValue>,
        is_addition: bool,
    ) {
        let (new_structure, offset) = if is_addition {
            self.structure_add_property(old_structure, key, attributes, true)
        } else {
            // Offset-stable attributeChange: data<->accessor conversion or getter/setter
            // update on an existing key. The GetterSetter slot below overwrites the prior
            // value (data value or old GetterSetter) at the preserved offset.
            self.convert_property_in_place(old_structure, key, attributes, true)
        };
        let getter_setter = self.allocate_getter_setter(getter, setter);
        let handle = self.find_mut(object).map(|object_cell| {
            object_cell.structure_id = new_structure;
            object_cell.butterfly
        });
        if let Some(handle) = handle {
            // No-op for a negative offset (should not happen — every key now gets a real
            // named offset).
            self.butterfly_prop_put(handle, offset, getter_setter);
        }
        self.finish_structure_transition(old_structure);
    }

    pub(crate) fn define_getter_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        getter: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.expect_function(getter)?;
        self.define_accessor_with_write_barrier(heap, object, key, Some(getter), None)
    }

    pub(crate) fn define_setter_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        setter: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.expect_function(setter)?;
        self.define_accessor_with_write_barrier(heap, object, key, None, Some(setter))
    }

    pub(crate) fn define_accessor(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        getter: Option<RuntimeValue>,
        setter: Option<RuntimeValue>,
    ) -> Result<(), ExecutionError> {
        if let Some(getter) = getter {
            self.expect_function(getter)?;
        }
        if let Some(setter) = setter {
            self.expect_function(setter)?;
        }
        // gc-r4 B-iv: the existing property + its getter/setter come from the Structure +
        // butterfly (the value authority), reconstructed via own_property_from_shape; the
        // per-cell `properties` HashMap is gone. define_getter/define_setter MERGE into an
        // existing accessor's other half.
        let (old_structure, current) = {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            (cell.structure_id, self.own_property_from_shape(cell, key))
        };
        let is_addition = current.is_none();
        let mut property = current.unwrap_or(CoreProperty {
            kind: CorePropertyKind::Accessor {
                getter: None,
                setter: None,
            },
            attributes: CorePropertyAttributes::ACCESSOR_DEFAULT,
        });
        match &mut property.kind {
            CorePropertyKind::Accessor {
                getter: existing_getter,
                setter: existing_setter,
            } => {
                if let Some(getter) = getter {
                    *existing_getter = Some(getter);
                }
                if let Some(setter) = setter {
                    *existing_setter = Some(setter);
                }
            }
            CorePropertyKind::Data(_) => {
                property = CoreProperty {
                    kind: CorePropertyKind::Accessor { getter, setter },
                    attributes: CorePropertyAttributes::ACCESSOR_DEFAULT,
                };
            }
        }
        // The MERGED getter/setter the GetterSetter cell + butterfly slot must hold.
        let (final_getter, final_setter) = match property.kind {
            CorePropertyKind::Accessor { getter, setter } => (getter, setter),
            CorePropertyKind::Data(_) => (None, None),
        };
        let shape_changed = match current {
            Some(current) => current != property,
            None => true,
        };
        if shape_changed {
            self.install_accessor_dual_write(
                object,
                key,
                old_structure,
                CorePropertyAttributes::ACCESSOR_DEFAULT,
                final_getter,
                final_setter,
                is_addition,
            );
        }
        Ok(())
    }

    pub(crate) fn define_accessor_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        getter: Option<RuntimeValue>,
        setter: Option<RuntimeValue>,
    ) -> Result<(), ExecutionError> {
        if let Some(getter) = getter {
            self.apply_value_store_write_barrier(heap, object, getter)?;
        }
        if let Some(setter) = setter {
            self.apply_value_store_write_barrier(heap, object, setter)?;
        }
        self.define_accessor(object, key, getter, setter)
    }

    pub(crate) fn define_accessor_property(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        getter: Option<RuntimeValue>,
        setter: Option<RuntimeValue>,
        attributes: CorePropertyAttributes,
    ) -> Result<bool, ExecutionError> {
        if let Some(getter) = getter {
            self.expect_function(getter)?;
        }
        if let Some(setter) = setter {
            self.expect_function(setter)?;
        }
        // gc-r4 B-iv: the existing property comes from the Structure + butterfly (the
        // value authority), not the deleted per-cell HashMap.
        let (old_structure, current) = {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            (cell.structure_id, self.own_property_from_shape(cell, key))
        };
        let is_addition = current.is_none();
        if let Some(current) = current {
            if !current.attributes.configurable {
                if attributes.configurable || attributes.enumerable != current.attributes.enumerable
                {
                    return Ok(false);
                }
                match current.kind {
                    CorePropertyKind::Data(_) => return Ok(false),
                    CorePropertyKind::Accessor {
                        getter: current_getter,
                        setter: current_setter,
                    } => {
                        if getter != current_getter || setter != current_setter {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        let property = CoreProperty {
            kind: CorePropertyKind::Accessor { getter, setter },
            attributes,
        };
        let shape_changed = match current {
            Some(current) => current != property,
            None => true,
        };
        if shape_changed {
            // define_accessor_property REPLACES the property, so the getter/setter passed
            // in ARE the final pair the GetterSetter mirror must hold; `attributes` carry
            // the explicit enumerable/configurable bits (the Accessor bit is ORed in by
            // structure_add_property).
            self.install_accessor_dual_write(
                object,
                key,
                old_structure,
                attributes,
                getter,
                setter,
                is_addition,
            );
        }
        Ok(true)
    }

    pub(crate) fn define_accessor_property_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        getter: Option<RuntimeValue>,
        setter: Option<RuntimeValue>,
        attributes: CorePropertyAttributes,
    ) -> Result<bool, ExecutionError> {
        if let Some(getter) = getter {
            self.apply_value_store_write_barrier(heap, object, getter)?;
        }
        if let Some(setter) = setter {
            self.apply_value_store_write_barrier(heap, object, setter)?;
        }
        self.define_accessor_property(object, key, getter, setter, attributes)
    }

    pub(crate) fn set_prototype_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        prototype: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.set_prototype_or_null_with_write_barrier(heap, object, Some(prototype))
    }

    pub(crate) fn get_prototype(
        &self,
        object: RuntimeValue,
    ) -> Result<Option<RuntimeValue>, ExecutionError> {
        let Some(object) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        Ok(object.prototype)
    }

    pub(crate) fn set_prototype_or_null(
        &mut self,
        object: RuntimeValue,
        prototype: Option<RuntimeValue>,
    ) -> Result<(), ExecutionError> {
        if self.find(object).is_none() {
            return Err(ExecutionError::ExpectedObject);
        }
        if let Some(prototype) = prototype {
            if self.find(prototype).is_none() {
                return Err(ExecutionError::ExpectedObject);
            }
            if prototype == object || self.prototype_chain_contains(prototype, object)? {
                return Err(ExecutionError::InvalidCallCompletion);
            }
        }
        // FIX 1: op_construct builds its this-receiver as an empty cell (seeded for
        // the default Object prototype at allocate_cell) and then immediately routes
        // here to install the constructor's `.prototype`. C++ JSC instead births a
        // construct instance ALREADY carrying the (class, Foo.prototype) Structure
        // (JSFinalObject::createStructure / the inheritor-structure cache), so two
        // `new Foo()` siblings start from one Structure and converge under
        // addPropertyTransition. Minting a fresh id here discarded that shared root,
        // so siblings never converged and cross-instance ICs missed.
        //
        // When the object is still EMPTY (no own properties recorded yet, i.e. the
        // just-allocated this-receiver), we therefore RE-SEED from the shared root
        // for (kind, NEW prototype) instead of minting fresh. For a NON-empty object
        // (a genuine later Object.setPrototypeOf / __proto__ assignment) we keep the
        // fresh-id fallback: that is a real prototype-change structure transition,
        // out of scope for the add-property transition table.
        let (kind, is_empty_object, current_structure) = match self.find(object) {
            Some(cell) => {
                let sid = cell.structure_id;
                // gc-r4 B-iv: "empty" == no own NAMED properties (the just-allocated
                // construct receiver), read from the Structure, not the deleted HashMap /
                // property_order. Array indexed elements are unaffected (a fresh receiver
                // has none); a real later setPrototypeOf takes the fresh-id fallback.
                (cell.kind, self.structure_property_keys(sid).is_empty(), sid)
            }
            None => return Err(ExecutionError::ExpectedObject),
        };
        let new_structure = if is_empty_object {
            self.seed_structure_id(kind, prototype)
        } else {
            // A prototype change on a non-empty object is a real structure transition
            // (ChangePrototype, out of scope for the add-property transition table). Use
            // a fresh per-object dictionary that PRESERVES the existing property offsets
            // (the named-data slots are unaffected by the prototype change) — minting a
            // fresh EMPTY structure would lose them. The structure's stored prototype
            // pointer stays the prior one (the cell's `prototype` field is authoritative
            // this batch; the faithful ChangePrototype transition is deferred).
            self.fresh_dictionary_structure(current_structure, None)
        };
        let old_structure = {
            let Some(object) = self.find_mut(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            if object.prototype != prototype {
                let old_structure = object.structure_id;
                object.prototype = prototype;
                object.structure_id = new_structure;
                Some(old_structure)
            } else {
                None
            }
        };
        if let Some(old_structure) = old_structure {
            self.finish_structure_transition(old_structure);
        }
        Ok(())
    }

    pub(crate) fn set_prototype_or_null_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        prototype: Option<RuntimeValue>,
    ) -> Result<(), ExecutionError> {
        if let Some(prototype) = prototype {
            self.apply_value_store_write_barrier(heap, object, prototype)?;
        }
        self.set_prototype_or_null(object, prototype)
    }

    pub(crate) fn prototype_chain_contains(
        &self,
        mut object: RuntimeValue,
        target: RuntimeValue,
    ) -> Result<bool, ExecutionError> {
        loop {
            if object == target {
                return Ok(true);
            }
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let Some(prototype) = cell.prototype else {
                return Ok(false);
            };
            object = prototype;
        }
    }

    pub(crate) fn set_function_super(
        &mut self,
        function: RuntimeValue,
        super_base: RuntimeValue,
        super_constructor: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        if self.find(super_base).is_none() {
            return Err(ExecutionError::ExpectedObject);
        }
        let Some(super_constructor_cell) = self.find(super_constructor) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if super_constructor_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        let Some(function_cell) = self.find_mut(function) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if function_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        function_cell.super_base = Some(super_base);
        function_cell.super_constructor = Some(super_constructor);
        Ok(())
    }

    pub(crate) fn set_function_super_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        function: RuntimeValue,
        super_base: RuntimeValue,
        super_constructor: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.apply_value_store_write_barrier(heap, function, super_base)?;
        self.apply_value_store_write_barrier(heap, function, super_constructor)?;
        self.set_function_super(function, super_base, super_constructor)
    }

    pub(crate) fn function_super_base(
        &self,
        function: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(function_cell) = self.find(function) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if function_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        function_cell
            .super_base
            .ok_or(ExecutionError::MissingSuperBinding)
    }

    pub(crate) fn function_super_constructor(
        &self,
        function: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(function_cell) = self.find(function) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if function_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        function_cell
            .super_constructor
            .ok_or(ExecutionError::MissingSuperBinding)
    }

    pub(crate) fn mark_default_derived_constructor(
        &mut self,
        function: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let Some(function_cell) = self.find_mut(function) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if function_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        function_cell.is_default_derived_constructor = true;
        Ok(())
    }

    pub(crate) fn is_default_derived_constructor(&self, function: RuntimeValue) -> bool {
        self.find(function).is_some_and(|function_cell| {
            function_cell.kind == CoreObjectKind::Function
                && function_cell.is_default_derived_constructor
        })
    }

    pub(crate) fn has_super_constructor(&self, function: RuntimeValue) -> bool {
        self.find(function).is_some_and(|function_cell| {
            function_cell.kind == CoreObjectKind::Function
                && function_cell.super_constructor.is_some()
        })
    }

    pub(crate) fn add_instance_field(
        &mut self,
        constructor: RuntimeValue,
        key: CorePropertyKey,
        initializer: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let initializer = if initializer.kind() == ValueKind::Undefined {
            None
        } else {
            let Some(initializer_cell) = self.find(initializer) else {
                return Err(ExecutionError::ExpectedFunction);
            };
            if initializer_cell.kind != CoreObjectKind::Function {
                return Err(ExecutionError::ExpectedFunction);
            }
            Some(initializer)
        };
        // Validate the constructor is a function (immutable find) BEFORE interning or
        // mutating, so the error path has no side effects — as in the original.
        let Some(constructor_cell) = self.find(constructor) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if constructor_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        let handle = constructor_cell.instance_fields;
        // gc-r4 R4 POD-ification (captures unit, SD-2): store the key as a POD `AtomId`
        // uid (interned via the SAME `intern_property_uid` the Structure graph uses) so the
        // relocated record is POD — no Drop-bearing `String` on the cell path. The reverse
        // map (`property_keys_by_uid`) reconstructs the `CorePropertyKey` on read.
        let key_uid = self.intern_property_uid(&key);
        let record = CoreInstanceFieldRecord {
            key_uid,
            initializer,
        };
        // Warm path: this constructor already has an instance-field slab slot — push.
        if handle != AuxiliaryHandle::INVALID {
            self.instance_field_lists[handle.0].push(record);
            return Ok(());
        }
        // Cold path (first field on this constructor): lazily allocate its slab slot
        // (mirroring `push_promise_reaction`), record the POD handle on the cell, then
        // push. The re-`find_mut` is needed because `allocate_instance_fields` borrows the
        // whole store. The slab preserves insertion order, so class-field init order (read
        // back by `instance_fields`) is unchanged.
        let new_handle = self.allocate_instance_fields();
        let Some(constructor_cell) = self.find_mut(constructor) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        constructor_cell.instance_fields = new_handle;
        self.instance_field_lists[new_handle.0].push(record);
        Ok(())
    }

    /// Allocate a fresh empty instance-field-record slab slot and return its POD handle.
    ///
    /// gc-r4 R4 POD-ification (captures unit): the pre-R4 store-owned-slab analog of
    /// allocating a class constructor's out-of-line `[[Fields]]` backing. Lazily called by
    /// `add_instance_field` on a constructor's FIRST field (most cells never have one), so
    /// the slab stays small — mirroring `allocate_promise_reactions`.
    fn allocate_instance_fields(&mut self) -> AuxiliaryHandle {
        let index = self.instance_field_lists.len();
        self.instance_field_lists.push(Vec::new());
        AuxiliaryHandle(index)
    }

    pub(crate) fn add_instance_field_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        constructor: RuntimeValue,
        key: CorePropertyKey,
        initializer: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        if initializer.kind() != ValueKind::Undefined {
            self.apply_value_store_write_barrier(heap, constructor, initializer)?;
        }
        self.add_instance_field(constructor, key, initializer)
    }

    pub(crate) fn instance_fields(
        &self,
        constructor: RuntimeValue,
    ) -> Result<Vec<CoreInstanceField>, ExecutionError> {
        let Some(constructor_cell) = self.find(constructor) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if constructor_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        let handle = constructor_cell.instance_fields;
        // gc-r4 R4 POD-ification (captures unit): no slab slot == no instance fields.
        if handle == AuxiliaryHandle::INVALID {
            return Ok(Vec::new());
        }
        // Reconstruct each `CoreInstanceField` from its POD slab record: map the interned
        // key uid back to its `CorePropertyKey` via `property_keys_by_uid` (the same reverse
        // map `structure_property_keys` uses). The slab preserves insertion order, so the
        // returned class-field init order is identical to the old per-cell Vec.
        let fields = self.instance_field_lists[handle.0]
            .iter()
            .map(|record| CoreInstanceField {
                // Invariant: every uid stored here was just interned by `add_instance_field`,
                // which inserts into `property_keys_by_uid` in lockstep, so the reverse
                // lookup always hits (a miss would mean a corrupted intern table).
                key: self
                    .property_keys_by_uid
                    .get(&record.key_uid)
                    .cloned()
                    .expect("instance-field key uid must be interned by add_instance_field"),
                initializer: record.initializer,
            })
            .collect();
        Ok(fields)
    }

    pub(crate) fn array_buffer_byte_length(
        &self,
        buffer: RuntimeValue,
    ) -> Result<usize, ExecutionError> {
        let Some(buffer) = self.find(buffer) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if buffer.kind != CoreObjectKind::ArrayBuffer {
            return Err(ExecutionError::ExpectedObject);
        }
        // gc-r4 ArrayBuffer unit: read the byte length from the store-owned backing via
        // the cell's POD handle (was the inline `Vec<u8>::len()`).
        Ok(self.array_buffer_bytes(buffer.array_buffer_data).len())
    }

    pub(crate) fn array_buffer_slice(
        &mut self,
        buffer: RuntimeValue,
        start: usize,
        end: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let bytes = {
            let Some(buffer) = self.find(buffer) else {
                return Err(ExecutionError::ExpectedObject);
            };
            if buffer.kind != CoreObjectKind::ArrayBuffer {
                return Err(ExecutionError::ExpectedObject);
            }
            // gc-r4 ArrayBuffer unit: read the source bytes from the store-owned backing
            // via the cell's POD handle (the cell `&self` borrow ends at the handle copy,
            // so the second shared `array_buffer_bytes` borrow is sound).
            let data = self.array_buffer_bytes(buffer.array_buffer_data);
            let start = start.min(data.len());
            let end = end.min(data.len()).max(start);
            data[start..end].to_vec()
        };
        let result = self.allocate_array_buffer(bytes.len());
        // Write the sliced bytes into the result's freshly zero-allocated backing (same
        // length), reached through its POD handle — the relocation of the former
        // `result_buffer.array_buffer_data = bytes` assignment.
        let Some(result_buffer) = self.find(result) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let result_handle = result_buffer.array_buffer_data;
        self.array_buffer_bytes_mut(result_handle)
            .copy_from_slice(&bytes);
        Ok(result)
    }

    pub(crate) fn uint8_array_layout(
        &self,
        value: RuntimeValue,
    ) -> Result<(RuntimeValue, usize, usize), ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Uint8Array {
            return Err(ExecutionError::ExpectedObject);
        }
        let Some(buffer) = object.view_buffer else {
            return Err(ExecutionError::ExpectedObject);
        };
        Ok((buffer, object.view_byte_offset, object.view_length))
    }

    /// Element kind of a typed-array view, mirroring C++ JSArrayBufferView's
    /// TypedArrayType. Only valid for CoreObjectKind::Uint8Array view cells.
    pub(crate) fn typed_array_element_kind(
        &self,
        value: RuntimeValue,
    ) -> Result<TypedArrayElementKind, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Uint8Array {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok(object.view_element_kind)
    }

    pub(crate) fn data_view_layout(
        &self,
        value: RuntimeValue,
    ) -> Result<(RuntimeValue, usize, usize), ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::DataView {
            return Err(ExecutionError::ExpectedObject);
        }
        let Some(buffer) = object.view_buffer else {
            return Err(ExecutionError::ExpectedObject);
        };
        Ok((buffer, object.view_byte_offset, object.view_byte_length))
    }

    /// Read element `index` of a typed-array view, returning the JS Number per
    /// C++ `Adaptor::toJSValue` (TypedArrayAdaptors.h). Scales the byte index by
    /// the element size and reinterprets the native bytes. Returns Ok(None) when
    /// out of bounds (mirrors C++ integer-indexed get returning undefined).
    pub(crate) fn read_typed_element(
        &self,
        value: RuntimeValue,
        index: usize,
    ) -> Result<Option<RuntimeValue>, ExecutionError> {
        let (buffer, byte_offset, length) = self.uint8_array_layout(value)?;
        if index >= length {
            return Ok(None);
        }
        let element_kind = self.typed_array_element_kind(value)?;
        let element_size = usize::from(typed_array_element_size(element_kind));
        let Some(buffer) = self.find(buffer) else {
            return Err(ExecutionError::ExpectedObject);
        };
        // gc-r4 ArrayBuffer unit: the bytes live in the store-owned backing; fetch the
        // POD handle off the cell (its `&self` borrow ends here) then read the slab.
        let data = self.array_buffer_bytes(buffer.array_buffer_data);
        let start = byte_offset.saturating_add(index.saturating_mul(element_size));
        let Some(bytes) = data.get(start..start.saturating_add(element_size)) else {
            return Ok(None);
        };
        let number = typed_array_load_value_f64(element_kind, bytes);
        Ok(Some(runtime_number_from_f64(number)))
    }

    /// Write the already-ToNumber-coerced `number` to element `index`, mirroring
    /// C++ `Adaptor::toNativeFromDouble` + `setIndexQuicklyToNativeValue`
    /// (JSGenericTypedArrayViewInlines.h). Scales by element size and serializes
    /// the native bytes. Returns Ok(false) when out of bounds (the C++ integer-
    /// indexed set silently drops out-of-bounds writes).
    pub(crate) fn write_typed_element(
        &mut self,
        value: RuntimeValue,
        index: usize,
        number: f64,
    ) -> Result<bool, ExecutionError> {
        let (buffer, byte_offset, length) = self.uint8_array_layout(value)?;
        if index >= length {
            return Ok(false);
        }
        let element_kind = self.typed_array_element_kind(value)?;
        let element_size = usize::from(typed_array_element_size(element_kind));
        let native = typed_array_store_native_bytes(element_kind, number);
        // gc-r4 ArrayBuffer unit: read the POD backing handle off the cell (a single
        // `find`, no extra hashmap lookup), then mutate the store-owned slab in place —
        // raw bytes, so no write barrier.
        let handle = {
            let Some(buffer) = self.find(buffer) else {
                return Err(ExecutionError::ExpectedObject);
            };
            buffer.array_buffer_data
        };
        let start = byte_offset.saturating_add(index.saturating_mul(element_size));
        let Some(slot) = self
            .array_buffer_bytes_mut(handle)
            .get_mut(start..start.saturating_add(element_size))
        else {
            return Ok(false);
        };
        slot.copy_from_slice(&native);
        Ok(true)
    }

    pub(crate) fn read_data_view_byte(
        &self,
        value: RuntimeValue,
        byte_offset: usize,
    ) -> Result<u8, ExecutionError> {
        let (buffer, view_offset, byte_length) = self.data_view_layout(value)?;
        if byte_offset >= byte_length {
            return Err(ExecutionError::ExpectedArrayIndex);
        }
        let Some(buffer) = self.find(buffer) else {
            return Err(ExecutionError::ExpectedObject);
        };
        // gc-r4 ArrayBuffer unit: read the byte from the store-owned backing via the
        // cell's POD handle (was the inline `Vec<u8>` indexing).
        self.array_buffer_bytes(buffer.array_buffer_data)
            .get(view_offset.saturating_add(byte_offset))
            .copied()
            .ok_or(ExecutionError::ExpectedArrayIndex)
    }

    pub(crate) fn write_data_view_byte(
        &mut self,
        value: RuntimeValue,
        byte_offset: usize,
        byte: u8,
    ) -> Result<(), ExecutionError> {
        let (buffer, view_offset, byte_length) = self.data_view_layout(value)?;
        if byte_offset >= byte_length {
            return Err(ExecutionError::ExpectedArrayIndex);
        }
        // gc-r4 ArrayBuffer unit: fetch the POD backing handle off the cell, then mutate
        // the store-owned slab byte in place (raw bytes, no write barrier).
        let handle = {
            let Some(buffer) = self.find(buffer) else {
                return Err(ExecutionError::ExpectedObject);
            };
            buffer.array_buffer_data
        };
        let Some(slot) = self
            .array_buffer_bytes_mut(handle)
            .get_mut(view_offset.saturating_add(byte_offset))
        else {
            return Err(ExecutionError::ExpectedArrayIndex);
        };
        *slot = byte;
        Ok(())
    }

    pub(crate) fn typed_array_byte_length(
        &self,
        value: RuntimeValue,
    ) -> Result<usize, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if !matches!(
            object.kind,
            CoreObjectKind::Uint8Array | CoreObjectKind::DataView
        ) {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok(object.view_byte_length)
    }

    pub(crate) fn typed_array_byte_offset(
        &self,
        value: RuntimeValue,
    ) -> Result<usize, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if !matches!(
            object.kind,
            CoreObjectKind::Uint8Array | CoreObjectKind::DataView
        ) {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok(object.view_byte_offset)
    }

    pub(crate) fn typed_array_buffer(
        &self,
        value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if !matches!(
            object.kind,
            CoreObjectKind::Uint8Array | CoreObjectKind::DataView
        ) {
            return Err(ExecutionError::ExpectedObject);
        }
        object.view_buffer.ok_or(ExecutionError::ExpectedObject)
    }

    pub(crate) fn get_index(
        &self,
        object: RuntimeValue,
        index: i32,
    ) -> Result<RuntimeValue, ExecutionError> {
        let index = usize::try_from(index).map_err(|_| ExecutionError::ExpectedArrayIndex)?;
        if self.is_uint8_array(object) {
            return self
                .read_typed_element(object, index)
                .map(|value| value.unwrap_or_else(RuntimeValue::undefined));
        }
        self.get_index_from_prototype_chain(object, index)
    }

    pub(crate) fn get_index_with_lookup_record(
        &self,
        object: RuntimeValue,
        index: i32,
        site: CorePropertyLookupSite,
    ) -> Result<(RuntimeValue, CorePropertyLookupRecord), ExecutionError> {
        let index = usize::try_from(index).map_err(|_| ExecutionError::ExpectedArrayIndex)?;
        self.get_index_from_prototype_chain_with_lookup_record(object, index, site)
    }

    pub(crate) fn put_index(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        index: i32,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let index = usize::try_from(index).map_err(|_| ExecutionError::ExpectedArrayIndex)?;
        if self.is_uint8_array(object) {
            self.write_typed_element(object, index, typed_array_store_input_number(value)?)?;
            return Ok(());
        }
        self.put_array_element_with_write_barrier(heap, object, index, value)
    }

    pub(crate) fn put_array_element(
        &mut self,
        object: RuntimeValue,
        index: usize,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let Some(handle) = self.find(object).map(|object| object.butterfly) else {
            return Err(ExecutionError::ExpectedObject);
        };
        // butterfly_elem_put grows the indexed side with hole fill, then stores —
        // exactly the old resize-then-set.
        self.butterfly_elem_put(handle, index, value);
        Ok(())
    }

    pub(crate) fn put_array_element_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        index: usize,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        self.apply_value_store_write_barrier(heap, object, value)?;
        self.put_array_element(object, index, value)
    }

    pub(crate) fn push_array_element(
        &mut self,
        object: RuntimeValue,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let Some((kind, handle)) = self
            .find(object)
            .map(|object| (object.kind, object.butterfly))
        else {
            return Err(ExecutionError::ExpectedObject);
        };
        if kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        self.butterfly_elem_push(handle, value);
        Ok(())
    }

    pub(crate) fn push_array_element_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let Some(array) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if array.kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        self.apply_value_store_write_barrier(heap, object, value)?;
        self.push_array_element(object, value)
    }

    pub(crate) fn delete_index(
        &mut self,
        object: RuntimeValue,
        index: i32,
    ) -> Result<bool, ExecutionError> {
        let index = usize::try_from(index).map_err(|_| ExecutionError::ExpectedArrayIndex)?;
        let Some((kind, handle)) = self
            .find(object)
            .map(|object| (object.kind, object.butterfly))
        else {
            return Err(ExecutionError::ExpectedObject);
        };
        if kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        self.butterfly_elem_clear(handle, index);
        Ok(true)
    }

    pub(crate) fn pop_array_element(
        &mut self,
        object: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some((kind, handle)) = self
            .find(object)
            .map(|object| (object.kind, object.butterfly))
        else {
            return Err(ExecutionError::ExpectedObject);
        };
        if kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok(self
            .butterfly_elem_pop(handle)
            .unwrap_or_else(RuntimeValue::undefined))
    }

    pub(crate) fn resize_array_elements(
        &mut self,
        object: RuntimeValue,
        length: usize,
    ) -> Result<(), ExecutionError> {
        let Some((kind, handle)) = self
            .find(object)
            .map(|object| (object.kind, object.butterfly))
        else {
            return Err(ExecutionError::ExpectedObject);
        };
        if kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        self.butterfly_elem_resize(handle, length);
        Ok(())
    }

    pub(crate) fn array_length(
        &self,
        object_value: RuntimeValue,
    ) -> Result<Option<RuntimeValue>, ExecutionError> {
        let Some(object) = self.find(object_value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind == CoreObjectKind::Array {
            return Ok(Some(RuntimeValue::from_i32(
                self.butterfly_elem_len(object.butterfly)
                    .try_into()
                    .unwrap_or(i32::MAX),
            )));
        }
        if object.kind == CoreObjectKind::Uint8Array {
            return Ok(Some(RuntimeValue::from_i32(
                object.view_length.try_into().unwrap_or(i32::MAX),
            )));
        }
        // C++ JSC: toLength(thisObj->get(exec, propertyNames.length))
        // For non-Array objects (e.g. arguments objects), read the "length"
        // property generically so that Array.prototype methods work on any
        // array-like object.
        let length_key = CorePropertyKey::String("length".into());
        match self.get_property(object_value, &length_key)? {
            CorePropertyGet::Data(value) => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    pub(crate) fn array_number_values(
        &self,
        object_value: RuntimeValue,
    ) -> Result<Vec<f64>, ExecutionError> {
        let Some(object) = self.find(object_value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        self.butterfly_elements(object.butterfly)
            .iter()
            .map(|slot| {
                let value = slot.unwrap_or_else(RuntimeValue::undefined);
                let Some(number) = value.as_number() else {
                    return Err(ExecutionError::ExpectedInt32);
                };
                Ok(number_to_f64(number))
            })
            .collect()
    }

    pub(crate) fn get_closure_cell(
        &self,
        value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::ClosureCell {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok(object.binding_value)
    }

    pub(crate) fn put_closure_cell(
        &mut self,
        cell: RuntimeValue,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let Some(object) = self.find_mut(cell) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::ClosureCell {
            return Err(ExecutionError::ExpectedObject);
        }
        object.binding_value = value;
        Ok(())
    }

    pub(crate) fn put_closure_cell_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        cell: RuntimeValue,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        let Some(object) = self.find(cell) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::ClosureCell {
            return Err(ExecutionError::ExpectedObject);
        }
        self.apply_value_store_write_barrier(heap, cell, value)?;
        self.put_closure_cell(cell, value)
    }

    pub(crate) fn function_call_target(
        &self,
        value: RuntimeValue,
    ) -> Result<CoreFunctionCallTarget, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        match object.kind {
            CoreObjectKind::Function => {
                let function_index = object
                    .function_index
                    .ok_or(ExecutionError::ExpectedFunction)?;
                Ok(CoreFunctionCallTarget::Bytecode {
                    function_index,
                    // gc-r4 R4 POD-ification (captures unit): the captured values live in
                    // the store-owned `captures_backings` slab now; read them through the
                    // cell's POD handle (always real for a Function cell) and snapshot the
                    // dispatch-local Vec exactly as before.
                    captures: self.captures_slice(object.captures).to_vec(),
                })
            }
            CoreObjectKind::NativeFunction => object
                .native_function
                .map(|native| CoreFunctionCallTarget::Native {
                    native,
                    callee: value,
                })
                .ok_or(ExecutionError::ExpectedFunction),
            // C++ JSC JSBoundFunction is callable; resolve through to the bound
            // target so callability checks succeed. Argument prepending and
            // boundThis substitution happen in
            // execute_function_value_with_completion.
            CoreObjectKind::BoundFunction => {
                let target = object
                    .bound_target
                    .ok_or(ExecutionError::ExpectedFunction)?;
                self.function_call_target(target)
            }
            CoreObjectKind::Ordinary
            | CoreObjectKind::Array
            | CoreObjectKind::ClosureCell
            | CoreObjectKind::Map
            | CoreObjectKind::Set
            | CoreObjectKind::WeakMap
            | CoreObjectKind::WeakSet
            | CoreObjectKind::RegExp
            | CoreObjectKind::Promise
            | CoreObjectKind::Date
            | CoreObjectKind::ArrayBuffer
            | CoreObjectKind::Uint8Array
            | CoreObjectKind::DataView
            // GetterSetter is an internal accessor cell, never callable.
            | CoreObjectKind::GetterSetter => Err(ExecutionError::ExpectedFunction),
            CoreObjectKind::Proxy => {
                let target = object
                    .proxy_target
                    .ok_or(ExecutionError::ExpectedFunction)?;
                self.function_call_target(target)
            }
        }
    }

    pub(crate) fn native_function_for_value(
        &self,
        value: RuntimeValue,
    ) -> Option<CoreNativeFunction> {
        let object = self.find(value)?;
        (object.kind == CoreObjectKind::NativeFunction)
            .then_some(object.native_function)
            .flatten()
    }

    pub(crate) fn function_call_target_value(
        &self,
        value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        match object.kind {
            CoreObjectKind::Function | CoreObjectKind::NativeFunction => Ok(value),
            // C++ JSC JSBoundFunction is itself a callable value.
            CoreObjectKind::BoundFunction => {
                let _ = object
                    .bound_target
                    .ok_or(ExecutionError::ExpectedFunction)?;
                Ok(value)
            }
            CoreObjectKind::Proxy => {
                let target = object
                    .proxy_target
                    .ok_or(ExecutionError::ExpectedFunction)?;
                self.function_call_target_value(target)
            }
            CoreObjectKind::Ordinary
            | CoreObjectKind::Array
            | CoreObjectKind::ClosureCell
            | CoreObjectKind::Map
            | CoreObjectKind::Set
            | CoreObjectKind::WeakMap
            | CoreObjectKind::WeakSet
            | CoreObjectKind::RegExp
            | CoreObjectKind::Promise
            | CoreObjectKind::Date
            | CoreObjectKind::ArrayBuffer
            | CoreObjectKind::Uint8Array
            | CoreObjectKind::DataView
            // GetterSetter is an internal accessor cell, never callable.
            | CoreObjectKind::GetterSetter => Err(ExecutionError::ExpectedFunction),
        }
    }

    pub(crate) fn function_capture(
        &self,
        value: RuntimeValue,
        index: u32,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if object.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        // gc-r4 R4 POD-ification (captures unit): the captured values live in the
        // store-owned `captures_backings` slab now; index it through the cell's POD handle
        // (always real for a Function cell). Same out-of-range -> MissingCapture as before.
        self.captures_slice(object.captures)
            .get(usize::try_from(index).unwrap_or(usize::MAX))
            .copied()
            .ok_or(ExecutionError::MissingCapture(index))
    }

    pub(crate) fn expect_function(&self, value: RuntimeValue) -> Result<(), ExecutionError> {
        if self.is_function(value) {
            Ok(())
        } else {
            Err(ExecutionError::ExpectedFunction)
        }
    }

    pub(crate) fn is_function(&self, value: RuntimeValue) -> bool {
        self.find(value).is_some_and(|object| {
            matches!(
                object.kind,
                // C++ JSC: a JSBoundFunction is callable, so `typeof` reports
                // "function" and callability checks succeed.
                CoreObjectKind::Function
                    | CoreObjectKind::NativeFunction
                    | CoreObjectKind::BoundFunction
            )
        })
    }

    pub(crate) fn function_construct_ability(
        &self,
        value: RuntimeValue,
    ) -> Result<ConstructAbility, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        match object.kind {
            CoreObjectKind::Function | CoreObjectKind::NativeFunction => {
                Ok(object.construct_ability)
            }
            CoreObjectKind::Proxy => {
                let target = object
                    .proxy_target
                    .ok_or(ExecutionError::ExpectedFunction)?;
                self.function_construct_ability(target)
            }
            // C++ JSC: JSBoundFunction inherits its construct ability from the
            // bound target ([[Construct]] forwards to [[BoundTargetFunction]]).
            CoreObjectKind::BoundFunction => {
                let target = object
                    .bound_target
                    .ok_or(ExecutionError::ExpectedFunction)?;
                self.function_construct_ability(target)
            }
            CoreObjectKind::Ordinary
            | CoreObjectKind::Array
            | CoreObjectKind::ClosureCell
            | CoreObjectKind::Map
            | CoreObjectKind::Set
            | CoreObjectKind::WeakMap
            | CoreObjectKind::WeakSet
            | CoreObjectKind::RegExp
            | CoreObjectKind::Promise
            | CoreObjectKind::Date
            | CoreObjectKind::ArrayBuffer
            | CoreObjectKind::Uint8Array
            | CoreObjectKind::DataView
            // GetterSetter is an internal accessor cell, never callable.
            | CoreObjectKind::GetterSetter => Err(ExecutionError::ExpectedFunction),
        }
    }

    pub(crate) fn function_not_constructor_message(&self, value: RuntimeValue) -> &'static str {
        let Some(object) = self.find(value) else {
            return "Value is not a constructor";
        };
        match object.kind {
            CoreObjectKind::NativeFunction => object
                .native_function
                .map(CoreNativeFunction::not_a_constructor_message)
                .unwrap_or("Function is not a constructor"),
            CoreObjectKind::Function => "Function is not a constructor",
            CoreObjectKind::Proxy => object
                .proxy_target
                .map(|target| self.function_not_constructor_message(target))
                .unwrap_or("Function is not a constructor"),
            _ => "Value is not a constructor",
        }
    }

    pub(crate) fn is_array(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::Array)
    }

    pub(crate) fn is_object(&self, value: RuntimeValue) -> bool {
        self.find(value).is_some()
    }

    pub(crate) fn is_constructor_return_value(&self, value: RuntimeValue) -> bool {
        self.find(value).is_some_and(|object| {
            matches!(
                object.kind,
                CoreObjectKind::Ordinary
                    | CoreObjectKind::Array
                    | CoreObjectKind::Function
                    | CoreObjectKind::NativeFunction
                    | CoreObjectKind::Map
                    | CoreObjectKind::Set
                    | CoreObjectKind::WeakMap
                    | CoreObjectKind::WeakSet
                    | CoreObjectKind::RegExp
                    | CoreObjectKind::Promise
                    | CoreObjectKind::Date
                    | CoreObjectKind::ArrayBuffer
                    | CoreObjectKind::Uint8Array
                    | CoreObjectKind::DataView
                    | CoreObjectKind::Proxy
            )
        })
    }

    pub(crate) fn is_map(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::Map)
    }

    pub(crate) fn is_set(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::Set)
    }

    pub(crate) fn is_weak_map(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::WeakMap)
    }

    pub(crate) fn is_weak_set(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::WeakSet)
    }

    pub(crate) fn is_regexp(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::RegExp)
    }

    pub(crate) fn is_promise(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::Promise)
    }

    pub(crate) fn is_date(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::Date)
    }

    pub(crate) fn is_array_buffer(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::ArrayBuffer)
    }

    pub(crate) fn is_uint8_array(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::Uint8Array)
    }

    pub(crate) fn is_data_view(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::DataView)
    }

    pub(crate) fn is_proxy(&self, value: RuntimeValue) -> bool {
        self.find(value)
            .is_some_and(|object| object.kind == CoreObjectKind::Proxy)
    }

    /// C++ JSC JSBoundFunction accessors: returns ([[BoundTargetFunction]],
    /// [[BoundThis]], [[BoundArguments]]) when `value` is a bound function.
    pub(crate) fn bound_function_data(
        &self,
        value: RuntimeValue,
    ) -> Option<(RuntimeValue, RuntimeValue, Vec<RuntimeValue>)> {
        let object = self.find(value)?;
        if object.kind != CoreObjectKind::BoundFunction {
            return None;
        }
        let target = object.bound_target?;
        let bound_this = object.bound_this;
        // gc-r4 POD-ification: the [[BoundArguments]] array now lives in the store-owned
        // slab, reached through the cell's POD handle. Copy the handle out, then read the
        // slab (both shared `&self` borrows). Clone the array because the caller needs it
        // owned after this borrow ends (it is consumed across a later `&mut self` call).
        let bound_args = self.bound_args_slice(object.bound_args).to_vec();
        Some((target, bound_this, bound_args))
    }

    pub(crate) fn proxy_target_handler(
        &self,
        value: RuntimeValue,
    ) -> Result<(RuntimeValue, RuntimeValue), ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Proxy {
            return Err(ExecutionError::ExpectedObject);
        }
        let target = object.proxy_target.ok_or(ExecutionError::ExpectedObject)?;
        let handler = object.proxy_handler.ok_or(ExecutionError::ExpectedObject)?;
        Ok((target, handler))
    }

    pub(crate) fn proxy_bound_to_revoke(
        &self,
        value: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if object.native_function != Some(CoreNativeFunction::ProxyRevoke) {
            return Err(ExecutionError::ExpectedFunction);
        }
        object
            .native_bound_proxy
            .ok_or(ExecutionError::ExpectedObject)
    }

    pub(crate) fn revoke_proxy(&mut self, value: RuntimeValue) -> Result<(), ExecutionError> {
        let Some(object) = self.find_mut(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Proxy {
            return Err(ExecutionError::ExpectedObject);
        }
        object.proxy_target = None;
        object.proxy_handler = None;
        Ok(())
    }

    pub(crate) fn regexp_source_and_flags(
        &self,
        value: RuntimeValue,
    ) -> Result<(String, RegexFlags, String), ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::RegExp {
            return Err(ExecutionError::ExpectedObject);
        }
        // Copy out the POD handle + flag bits, then resolve the pattern string from
        // the store-owned slab and recompute the canonical-order flags text from the
        // bits (C++ derives the flags string via `Yarr::flagsString`; there is no
        // stored flags text). Both `self.find` and `self.regexp_source_str` are
        // shared borrows, so they coexist.
        let source_handle = object.regexp_source;
        let flags = object.regexp_flags;
        Ok((
            self.regexp_source_str(source_handle).to_string(),
            flags,
            regexp_canonical_flags_string(flags),
        ))
    }

    pub(crate) fn promise_state_and_result(
        &self,
        value: RuntimeValue,
    ) -> Result<(PromiseState, RuntimeValue), ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Promise {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok((object.promise_state, object.promise_result))
    }

    pub(crate) fn promise_resolving_binding(
        &self,
        value: RuntimeValue,
    ) -> Result<(RuntimeValue, CorePromiseResolvingKind), ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if object.kind != CoreObjectKind::NativeFunction
            || object.native_function != Some(CoreNativeFunction::PromiseResolvingFunction)
        {
            return Err(ExecutionError::ExpectedFunction);
        }
        let promise = object
            .native_bound_promise
            .ok_or(ExecutionError::ExpectedObject)?;
        let kind = object
            .promise_resolving_kind
            .ok_or(ExecutionError::ExpectedFunction)?;
        Ok((promise, kind))
    }

    pub(crate) fn take_promise_reactions(
        &mut self,
        promise: RuntimeValue,
        state: PromiseState,
        result: RuntimeValue,
    ) -> Result<Vec<CorePromiseReaction>, ExecutionError> {
        let Some(object) = self.find_mut(promise) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Promise {
            return Err(ExecutionError::ExpectedObject);
        }
        if object.promise_state != PromiseState::Pending {
            return Ok(Vec::new());
        }
        object.promise_state = state;
        object.promise_result = result;
        let handle = object.promise_reactions;
        // gc-r4 R4 POD-ification (Promise unit): the pending reaction records live in
        // the store-owned `promise_reaction_lists` slab now. An INVALID handle == a
        // pending promise that never had a reaction enqueued (empty
        // `[[..Reactions]]`); otherwise drain the slot (settling a promise consumes
        // its reaction list — C++ `JSPromise::reject`/`resolve` clears the fields).
        // `mem::take` leaves an empty Vec in the slot; the now-settled promise never
        // enqueues again, so the slot stays empty.
        if handle == PromiseReactionsHandle::INVALID {
            return Ok(Vec::new());
        }
        Ok(std::mem::take(&mut self.promise_reaction_lists[handle.0]))
    }

    pub(crate) fn take_promise_reactions_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        promise: RuntimeValue,
        state: PromiseState,
        result: RuntimeValue,
    ) -> Result<Vec<CorePromiseReaction>, ExecutionError> {
        self.apply_value_store_write_barrier(heap, promise, result)?;
        self.take_promise_reactions(promise, state, result)
    }

    /// Allocate a fresh empty reaction-list slab slot and return its handle.
    ///
    /// C++ JSC: a pending promise's `[[..Reactions]]` records are GC-heap
    /// allocations linked off the promise; this is the pre-R4 store-owned-slab
    /// analog of allocating that out-of-line record backing (mirrors
    /// `allocate_butterfly`). Lazily called by `push_promise_reaction` on a pending
    /// promise's FIRST reaction (most promises settle without one, so the slab stays
    /// small — unlike the butterfly slab, this is per-pending-promise, not per-cell).
    fn allocate_promise_reactions(&mut self) -> PromiseReactionsHandle {
        let index = self.promise_reaction_lists.len();
        self.promise_reaction_lists.push(Vec::new());
        PromiseReactionsHandle(index)
    }

    pub(crate) fn push_promise_reaction(
        &mut self,
        promise: RuntimeValue,
        reaction: CorePromiseReaction,
    ) -> Result<(), ExecutionError> {
        let Some(object) = self.find_mut(promise) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Promise {
            return Err(ExecutionError::ExpectedObject);
        }
        // gc-r4 R4 POD-ification (Promise unit): the reaction records live in the
        // store-owned `promise_reaction_lists` slab now. Warm path: the promise
        // already has a slab slot — push into it (single lookup).
        let handle = object.promise_reactions;
        if handle != PromiseReactionsHandle::INVALID {
            self.promise_reaction_lists[handle.0].push(reaction);
            return Ok(());
        }
        // Cold path (first reaction on this pending promise): lazily allocate its slab
        // slot (C++ JSPromise's `[[..Reactions]]` records materialize on first
        // enqueue), record the handle on the cell, then push. The re-`find_mut` is
        // needed because `allocate_promise_reactions` borrows the whole store.
        let new_handle = self.allocate_promise_reactions();
        let Some(object) = self.find_mut(promise) else {
            return Err(ExecutionError::ExpectedObject);
        };
        object.promise_reactions = new_handle;
        self.promise_reaction_lists[new_handle.0].push(reaction);
        Ok(())
    }

    pub(crate) fn push_promise_reaction_with_write_barrier(
        &mut self,
        heap: &mut Heap,
        promise: RuntimeValue,
        reaction: CorePromiseReaction,
    ) -> Result<(), ExecutionError> {
        self.apply_value_store_write_barrier(heap, promise, reaction.result_promise)?;
        self.apply_value_store_write_barrier(heap, promise, reaction.on_fulfilled)?;
        self.apply_value_store_write_barrier(heap, promise, reaction.on_rejected)?;
        self.push_promise_reaction(promise, reaction)
    }

    pub(crate) fn date_value(&self, value: RuntimeValue) -> Result<f64, ExecutionError> {
        let Some(object) = self.find(value) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Date {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok(object.date_value)
    }

    pub(crate) fn constructor_instance_prototype(
        &self,
        constructor: RuntimeValue,
        prototype_property_key: &CorePropertyKey,
    ) -> Option<RuntimeValue> {
        let cell = self.find(constructor)?;
        let prototype = match self
            .own_property_from_shape(cell, prototype_property_key)?
            .kind
        {
            CorePropertyKind::Data(value) => value,
            CorePropertyKind::Accessor { .. } => return None,
        };
        self.find(prototype).map(|_| prototype)
    }

    pub(crate) fn instance_of(
        &self,
        value: RuntimeValue,
        constructor: RuntimeValue,
        prototype_property_key: &CorePropertyKey,
    ) -> Result<bool, ExecutionError> {
        let Some(constructor_cell) = self.find(constructor) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if !matches!(
            constructor_cell.kind,
            CoreObjectKind::Function | CoreObjectKind::NativeFunction
        ) {
            return Err(ExecutionError::ExpectedFunction);
        }
        let Some(prototype) =
            self.constructor_instance_prototype(constructor, prototype_property_key)
        else {
            return Ok(false);
        };
        let Some(mut current) = self.find(value).and_then(|cell| cell.prototype) else {
            return Ok(false);
        };
        loop {
            if current == prototype {
                return Ok(true);
            }
            let Some(next) = self.find(current).and_then(|cell| cell.prototype) else {
                return Ok(false);
            };
            current = next;
        }
    }

    pub(crate) fn get_symbol_prototype_property(
        &mut self,
        key: &CorePropertyKey,
    ) -> Result<CorePropertyGet, ExecutionError> {
        let prototype = self.ensure_symbol_prototype();
        self.get_property(prototype, key)
    }

    pub(crate) fn get_bigint_prototype_property(
        &mut self,
        key: &CorePropertyKey,
    ) -> Result<CorePropertyGet, ExecutionError> {
        let prototype = self.ensure_bigint_prototype();
        self.get_property(prototype, key)
    }

    pub(crate) fn get_number_prototype_property(
        &mut self,
        key: &CorePropertyKey,
    ) -> Result<CorePropertyGet, ExecutionError> {
        let prototype = self.ensure_number_prototype();
        self.get_property(prototype, key)
    }

    pub(crate) fn get_boolean_prototype_property(
        &mut self,
        key: &CorePropertyKey,
    ) -> Result<CorePropertyGet, ExecutionError> {
        let prototype = self.ensure_boolean_prototype();
        self.get_property(prototype, key)
    }

    pub(crate) fn get_property_from_prototype_chain(
        &self,
        mut object: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Result<CorePropertyGet, ExecutionError> {
        loop {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            if let Some(property) = self.own_property_from_shape(cell, key) {
                return Ok(match property.kind {
                    CorePropertyKind::Data(value) => CorePropertyGet::Data(value),
                    CorePropertyKind::Accessor {
                        getter: Some(getter),
                        ..
                    } => CorePropertyGet::Getter(getter),
                    CorePropertyKind::Accessor { getter: None, .. } => {
                        CorePropertyGet::AccessorWithoutGetter
                    }
                });
            }
            // gc-r4 B-iv: array-index-named data properties live in indexed butterfly
            // storage for EVERY object kind (not just arrays).
            if cell.kind != CoreObjectKind::Uint8Array {
                if let Some(index) = key_array_index(key) {
                    if let Some(value) = self.butterfly_elem_get(cell.butterfly, index) {
                        return Ok(CorePropertyGet::Data(value));
                    }
                }
            }
            if cell.kind == CoreObjectKind::Uint8Array {
                if let Some(index) = key_array_index(key) {
                    if let Some(value) = self.read_typed_element(object, index)? {
                        return Ok(CorePropertyGet::Data(value));
                    }
                }
            }
            // C++ JSC: exotic OWN `length` of Array / TypedArray, held outside the
            // property table; get_by_id-lowered reads (e.g. `arr.length++/--`) must
            // see it instead of walking off the end of the chain to undefined.
            if key.is_string("length")
                && matches!(
                    cell.kind,
                    CoreObjectKind::Array | CoreObjectKind::Uint8Array
                )
            {
                let length = if cell.kind == CoreObjectKind::Array {
                    self.butterfly_elem_len(cell.butterfly)
                } else {
                    cell.view_length
                };
                return Ok(CorePropertyGet::Data(RuntimeValue::from_i32(
                    length.try_into().unwrap_or(i32::MAX),
                )));
            }
            let Some(prototype) = cell.prototype else {
                return Ok(CorePropertyGet::Missing);
            };
            object = prototype;
        }
    }

    pub(crate) fn get_property_from_prototype_chain_with_lookup_record(
        &self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        site: CorePropertyLookupSite,
    ) -> Result<(CorePropertyGet, CorePropertyLookupRecord), ExecutionError> {
        let Some(base_cell) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let base_structure = Some(base_cell.structure_id);

        let mut current = object;
        let mut prototype_depth = 0;
        let mut chain = Vec::new();
        loop {
            let Some(cell) = self.find(current) else {
                return Err(ExecutionError::ExpectedObject);
            };
            chain.push(CorePropertyLookupChainEntry {
                object: current,
                structure: cell.structure_id,
            });
            if let Some(property) = self.own_property_from_shape(cell, key) {
                let found_structure = cell.structure_id;
                return Ok(match property.kind {
                    CorePropertyKind::Data(value) => {
                        let mut record = CorePropertyLookupRecord::from_object_lookup(
                            site,
                            object,
                            key,
                            Some(current),
                            prototype_depth,
                            if prototype_depth == 0 {
                                CorePropertyLookupClassification::OwnData
                            } else {
                                CorePropertyLookupClassification::PrototypeData
                            },
                        );
                        record.base_structure = base_structure;
                        record.offset = self.structure_offset(found_structure, key);
                        record.returned_value = Some(value);
                        record.chain = chain.clone();
                        (CorePropertyGet::Data(value), record)
                    }
                    CorePropertyKind::Accessor {
                        getter: Some(getter),
                        ..
                    } => {
                        let mut record = CorePropertyLookupRecord::from_object_lookup(
                            site,
                            object,
                            key,
                            Some(current),
                            prototype_depth,
                            if prototype_depth == 0 {
                                CorePropertyLookupClassification::OwnAccessorGetter
                            } else {
                                CorePropertyLookupClassification::PrototypeAccessorGetter
                            },
                        );
                        record.base_structure = base_structure;
                        record.getter = Some(getter);
                        record.chain = chain.clone();
                        (CorePropertyGet::Getter(getter), record)
                    }
                    CorePropertyKind::Accessor { getter: None, .. } => {
                        let mut record = CorePropertyLookupRecord::from_object_lookup(
                            site,
                            object,
                            key,
                            Some(current),
                            prototype_depth,
                            CorePropertyLookupClassification::AccessorWithoutGetter,
                        );
                        record.base_structure = base_structure;
                        record.returned_value = Some(RuntimeValue::undefined());
                        record.chain = chain.clone();
                        (CorePropertyGet::AccessorWithoutGetter, record)
                    }
                });
            }
            // gc-r4 B-iv: array-index-named data properties live in indexed butterfly
            // storage for EVERY object kind (not just arrays).
            if cell.kind != CoreObjectKind::Uint8Array {
                if let Some(index) = key_array_index(key) {
                    if let Some(value) = self.butterfly_elem_get(cell.butterfly, index) {
                        let mut record = CorePropertyLookupRecord::from_object_lookup(
                            site,
                            object,
                            key,
                            Some(current),
                            prototype_depth,
                            CorePropertyLookupClassification::IndexedOrTypedArray,
                        );
                        record.base_structure = base_structure;
                        record.returned_value = Some(value);
                        record.chain = chain.clone();
                        return Ok((CorePropertyGet::Data(value), record));
                    }
                }
            }
            if cell.kind == CoreObjectKind::Uint8Array {
                if let Some(index) = key_array_index(key) {
                    if let Some(value) = self.read_typed_element(current, index)? {
                        let mut record = CorePropertyLookupRecord::from_object_lookup(
                            site,
                            object,
                            key,
                            Some(current),
                            prototype_depth,
                            CorePropertyLookupClassification::IndexedOrTypedArray,
                        );
                        record.base_structure = base_structure;
                        record.returned_value = Some(value);
                        record.chain = chain.clone();
                        return Ok((CorePropertyGet::Data(value), record));
                    }
                }
            }
            // C++ JSC: `length` is an exotic OWN value property of Array /
            // TypedArray (JSArray::m_butterfly publicLength, JSArrayBufferView
            // length), NOT a property-table entry. The dedicated `op_get_length`
            // opcode special-cases it, but `obj.length++/--` (and other
            // get_by_id-lowered reads) funnel through here; without this they walk
            // the length-less property chain and return undefined -> ToNumber NaN.
            // OpaqueOrUncacheable so the get_by_id IC never arms a bogus monomorphic
            // property-offset load for it.
            if key.is_string("length")
                && matches!(
                    cell.kind,
                    CoreObjectKind::Array | CoreObjectKind::Uint8Array
                )
            {
                let length = if cell.kind == CoreObjectKind::Array {
                    self.butterfly_elem_len(cell.butterfly)
                } else {
                    cell.view_length
                };
                let value = RuntimeValue::from_i32(length.try_into().unwrap_or(i32::MAX));
                let mut record = CorePropertyLookupRecord::from_object_lookup(
                    site,
                    object,
                    key,
                    Some(current),
                    prototype_depth,
                    CorePropertyLookupClassification::OpaqueOrUncacheable,
                );
                record.base_structure = base_structure;
                record.returned_value = Some(value);
                record.chain = chain.clone();
                return Ok((CorePropertyGet::Data(value), record));
            }
            let Some(prototype) = cell.prototype else {
                let mut record = CorePropertyLookupRecord::from_object_lookup(
                    site,
                    object,
                    key,
                    None,
                    prototype_depth,
                    CorePropertyLookupClassification::Missing,
                );
                record.base_structure = base_structure;
                record.returned_value = Some(RuntimeValue::undefined());
                record.chain = chain.clone();
                return Ok((CorePropertyGet::Missing, record));
            };
            current = prototype;
            prototype_depth = prototype_depth.saturating_add(1);
        }
    }

    pub(crate) fn get_index_from_prototype_chain(
        &self,
        mut object: RuntimeValue,
        index: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let key = CorePropertyKey::String(index.to_string());
        loop {
            let Some(cell) = self.find(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            if cell.kind == CoreObjectKind::Uint8Array {
                return self
                    .read_typed_element(object, index)
                    .map(|value| value.unwrap_or_else(RuntimeValue::undefined));
            }
            if let Some(value) = self.butterfly_elem_get(cell.butterfly, index) {
                return Ok(value);
            }
            if let Some(property) = self.own_property_from_shape(cell, &key) {
                if let CorePropertyKind::Data(value) = property.kind {
                    return Ok(value);
                }
            }
            let Some(prototype) = cell.prototype else {
                return Ok(RuntimeValue::undefined());
            };
            object = prototype;
        }
    }

    pub(crate) fn get_index_from_prototype_chain_with_lookup_record(
        &self,
        object: RuntimeValue,
        index: usize,
        site: CorePropertyLookupSite,
    ) -> Result<(RuntimeValue, CorePropertyLookupRecord), ExecutionError> {
        let key = CorePropertyKey::String(index.to_string());
        let Some(base_cell) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let base_structure = Some(base_cell.structure_id);

        let mut current = object;
        let mut prototype_depth = 0;
        let mut chain = Vec::new();
        loop {
            let Some(cell) = self.find(current) else {
                return Err(ExecutionError::ExpectedObject);
            };
            chain.push(CorePropertyLookupChainEntry {
                object: current,
                structure: cell.structure_id,
            });

            if cell.kind == CoreObjectKind::Uint8Array {
                let value = self
                    .read_typed_element(current, index)?
                    .unwrap_or_else(RuntimeValue::undefined);
                let mut record = CorePropertyLookupRecord::from_object_lookup(
                    site,
                    object,
                    &key,
                    Some(current),
                    prototype_depth,
                    CorePropertyLookupClassification::IndexedOrTypedArray,
                );
                record.base_structure = base_structure;
                record.returned_value = Some(value);
                record.chain = chain.clone();
                return Ok((value, record));
            }
            if let Some(value) = self.butterfly_elem_get(cell.butterfly, index) {
                let mut record = CorePropertyLookupRecord::from_object_lookup(
                    site,
                    object,
                    &key,
                    Some(current),
                    prototype_depth,
                    CorePropertyLookupClassification::IndexedOrTypedArray,
                );
                record.base_structure = base_structure;
                record.returned_value = Some(value);
                record.chain = chain.clone();
                return Ok((value, record));
            }
            if let Some(property) = self.own_property_from_shape(cell, &key) {
                if let CorePropertyKind::Data(value) = property.kind {
                    let found_structure = cell.structure_id;
                    let mut record = CorePropertyLookupRecord::from_object_lookup(
                        site,
                        object,
                        &key,
                        Some(current),
                        prototype_depth,
                        if prototype_depth == 0 {
                            CorePropertyLookupClassification::OwnData
                        } else {
                            CorePropertyLookupClassification::PrototypeData
                        },
                    );
                    record.base_structure = base_structure;
                    record.returned_value = Some(value);
                    record.chain = chain.clone();
                    record.offset = self.structure_offset(found_structure, &key);
                    return Ok((value, record));
                }
            }
            let Some(prototype) = cell.prototype else {
                let value = RuntimeValue::undefined();
                let mut record = CorePropertyLookupRecord::from_object_lookup(
                    site,
                    object,
                    &key,
                    None,
                    prototype_depth,
                    CorePropertyLookupClassification::Missing,
                );
                record.base_structure = base_structure;
                record.returned_value = Some(value);
                record.chain = chain.clone();
                return Ok((value, record));
            };
            current = prototype;
            prototype_depth = prototype_depth.saturating_add(1);
        }
    }

    pub(crate) fn find(&self, value: RuntimeValue) -> Option<&CoreObjectCell> {
        let payload = value.as_cell()?.pointer_payload_bits();
        let index = self.object_indices_by_payload.get(&payload).copied()?;
        let object = self.objects.get(index)?.as_ref().get_ref();
        debug_assert_eq!(core::ptr::from_ref(object) as usize, payload);
        // Cross-check the new in-cell JSCell::m_type (runtime/JSCell.h:298) against the
        // existing object_indices_by_payload type gate: a cell reached through the
        // object index MUST report an object-range JSType (C++ `m_type >= ObjectType`,
        // runtime/JSType.h:204). Exercises the header on every object lookup; debug-only
        // so release behavior is unchanged.
        debug_assert!(
            object.js_type.is_object(),
            "object reached via object_indices_by_payload must carry an object JSType"
        );
        let _ = object.cell_id;
        Some(object)
    }

    /// LLInt monomorphic GET fast path read, mirroring `performGetByIDHelper`'s
    /// `.opGetByIdDefault` arm (LowLevelInterpreter64.asm:1639): structure guard,
    /// then `loadPropertyAtVariableOffset` from out-of-line storage.
    ///
    /// C++ FAST PATH: `loadi JSCell::m_structureID[t3]` -> compare to the cached
    /// `defaultMode.structureID` -> `loadPropertyAtVariableOffset` from the
    /// Butterfly. The Rust mirror reuses the SAME cell layout: `structure_id` and
    /// `out_of_line_storage` are exactly what the slow path maintains in lockstep,
    /// so a structure match implies the cached offset is valid (invariant b) and
    /// the slot value equals what the slow path would return (invariant c).
    ///
    /// This deliberately resolves the cell through `object_indices_by_payload` (an
    /// integer-keyed probe) as the SOUNDNESS GATE before dereferencing: a payload
    /// from a non-object cell (string/symbol/bigint, allocated in a different
    /// `Pin<Box<T>>` store with a different layout) must never be read as a
    /// `CoreObjectCell`, and the structure-id compare alone cannot prove the
    /// pointer's type. DIVERGENCE from the frozen "deref the Pin<Box> directly"
    /// note: the integer-keyed membership probe is kept as the type/liveness gate
    /// for memory safety; it is far cheaper than the slow path it replaces (no
    /// `CorePropertyKey` String allocation, no `properties`/`property_offsets`
    /// key-hash lookups, no proxy/symbol/primitive guards, no observation /
    /// completion-context build). Returns `None` on any miss so the caller falls
    /// to the unchanged slow path and refills.
    pub(crate) fn llint_get_by_id_fast(
        &self,
        receiver: RuntimeValue,
        cached_structure_id: StructureId,
        cached_offset: PropertyOffset,
    ) -> Option<RuntimeValue> {
        let payload = receiver.as_cell()?.pointer_payload_bits();
        let index = self.object_indices_by_payload.get(&payload).copied()?;
        let cell = self.objects.get(index)?.as_ref().get_ref();
        if cell.structure_id != cached_structure_id {
            return None;
        }
        // Structure match => same (kind, prototype, shape) => the cached offset is
        // a live own-data slot (invariant a/b). Read it directly from the butterfly
        // slab `props` mirror (store-owned) with NO key comparison or HashMap scan.
        let handle = cell.butterfly;
        self.butterfly_prop_get(handle, cached_offset)
    }

    /// LLInt monomorphic PUT replace-existing fast path, mirroring the
    /// `storePropertyAtVariableOffset` store after a structure guard
    /// (LowLevelInterpreter64.asm:1581). ONLY the replace-existing case: the
    /// structure is UNCHANGED by the write (no transition), so the cached offset
    /// stays valid. Returns `true` if it stored on the fast path, `false` on any
    /// miss (caller takes the unchanged slow put + refills).
    ///
    /// Same soundness gate as `llint_get_by_id_fast`. The structure guard proves
    /// `cached_key` is the live OWN DATA property at `cached_offset`; writing it
    /// does not change `structure_id` (a replace, not an add — invariant a), so
    /// the cache stays valid for the next iteration.
    ///
    /// Updates BOTH the value-authoritative `properties` HashMap (via the cached
    /// key — one hash lookup, NO allocation) and the `out_of_line_storage` mirror
    /// (invariant c), so a later slow-path read sees the new value. Refuses
    /// (returns false) if the guarded property is not actually a writable own
    /// data property at the cached offset — a defensive re-check that keeps the
    /// fast path from serving a write the slow path would reject (read-only) or
    /// mis-target (accessor / shape drift).
    pub(crate) fn llint_put_by_id_replace_fast(
        &mut self,
        heap: &mut Heap,
        receiver: RuntimeValue,
        cached_structure_id: StructureId,
        cached_offset: PropertyOffset,
        cached_key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<bool, ExecutionError> {
        let Some(payload) = receiver.as_cell().map(|cell| cell.pointer_payload_bits()) else {
            return Ok(false);
        };
        let Some(index) = self.object_indices_by_payload.get(&payload).copied() else {
            return Ok(false);
        };
        // Read-only structure/writability checks first (immutable borrow), so a
        // miss bails BEFORE touching the GC write barrier.
        {
            let Some(cell) = self.objects.get(index) else {
                return Ok(false);
            };
            let cell = cell.as_ref().get_ref();
            if cell.structure_id != cached_structure_id {
                return Ok(false);
            }
            // The structure match guarantees the cached_key is an own data property
            // at cached_offset. Re-confirm data-kind + writability before storing:
            // the structure invariant already implies this, but the explicit check
            // guards a read-only/accessor target (which the slow put would leave
            // untouched / route to a setter) and keeps the fast path from diverging
            // from slow-path semantics. gc-r4 B-iv: read the SHAPE (offset/attribute
            // authority), not the deleted per-cell HashMap.
            match self.structure_property(cell.structure_id, cached_key) {
                Some((_, attributes))
                    if attributes & PROPERTY_ATTRIBUTE_ACCESSOR == 0
                        && core_attributes_from_u32(attributes).writable => {}
                _ => return Ok(false),
            }
        }
        // GC write barrier, identical to the slow store's
        // set_data_own_with_write_barrier -> apply_value_store_write_barrier. MUST
        // run on the fast path too: storing a heap value into an object field is a
        // barriered mutator field write regardless of whether an IC served it.
        self.apply_value_store_write_barrier(heap, receiver, value)?;
        // Re-validate the structure after the barrier (the barrier path does not mutate
        // this cell's shape, but the re-fetch keeps the store self-contained) and capture
        // the butterfly handle; the butterfly slot IS the value authority post-flip.
        let handle = {
            let Some(cell) = self.objects.get(index) else {
                return Ok(false);
            };
            let cell = cell.as_ref().get_ref();
            if cell.structure_id != cached_structure_id {
                return Ok(false);
            }
            cell.butterfly
        };
        // putDirectOffset analog (invariant c): write the value into the butterfly slab
        // `props` side at the cached offset. The slot already exists (the structure match
        // proves the shape), so this is an in-place store.
        self.butterfly_prop_put(handle, cached_offset, value);
        Ok(true)
    }

    pub(crate) fn find_by_object_id(&self, object_id: ObjectId) -> Option<&CoreObjectCell> {
        if object_id == ObjectId::default() {
            return None;
        }
        self.objects
            .iter()
            .map(|object| object.as_ref().get_ref())
            .find(|object| object.cell_id == object_id.0)
    }

    // Raw, pinned `CoreObjectCell*` (as `usize` bits) for an object id, the value a
    // resident prototype DataIC bakes as its holder pointer. The cell is a
    // `Pin<Box<_>>` and never moves, so this address is stable while the cell is
    // live. Returns `None` for an unknown id or a Proxy (opaque) holder, which the
    // resident DataIC must not bake (no fixed structure/offset layout). The address
    // matches `value.as_cell().pointer_payload_bits()` for the cell's boxed value,
    // the same equality `find` debug-asserts.
    pub(crate) fn holder_cell_pointer_for_object_id(&self, object_id: ObjectId) -> Option<u64> {
        let cell = self.find_by_object_id(object_id)?;
        if cell.kind == CoreObjectKind::Proxy {
            return None;
        }
        Some(core::ptr::from_ref(cell) as usize as u64)
    }

    pub(crate) fn find_mut(&mut self, value: RuntimeValue) -> Option<&mut CoreObjectCell> {
        let payload = value.as_cell()?.pointer_payload_bits();
        let index = self.object_indices_by_payload.get(&payload).copied()?;
        let object = self.objects.get_mut(index)?.as_mut().get_mut();
        debug_assert_eq!(core::ptr::from_ref(object) as usize, payload);
        Some(object)
    }

    #[cfg(test)]
    pub(crate) fn cell_id(&self, value: RuntimeValue) -> Option<CellId> {
        self.find(value).map(|cell| cell.cell_id)
    }

    #[cfg(test)]
    pub(crate) fn structure_id(&self, value: RuntimeValue) -> Option<StructureId> {
        self.find(value).map(|cell| cell.structure_id)
    }

    #[cfg(test)]
    pub(crate) fn property_offset(
        &self,
        value: RuntimeValue,
        key: &CorePropertyKey,
    ) -> Option<PropertyOffset> {
        let structure = self.find(value)?.structure_id;
        self.structure_offset(structure, key)
    }
}

pub(crate) fn allocate_object_interpreter_cell_id(
    heap: &mut Heap,
) -> Result<CellId, ExecutionError> {
    let metadata = static_cell_metadata_registry()
        .metadata_for_type(CellType::Object)
        .map(|descriptor| descriptor.metadata)
        .ok_or(ExecutionError::MissingStaticCellMetadata(CellType::Object))?;
    let allocation = heap.allocate_record(HeapAllocationRequest {
        heap: heap.id(),
        subspace: "object",
        metadata,
        byte_size: std::mem::size_of::<CoreObjectCell>().max(1),
        mode: AllocationMode::Normal,
        may_trigger_collection: false,
    })?;
    Ok(allocation.cell)
}

#[cfg(test)]
mod butterfly_values_cutover_tests {
    //! gc-r4 Butterfly-values cutover verification.
    //!
    //! These prove the de-self-reference is faithful: out-of-line property VALUES
    //! and indexed ELEMENTS live in the store-owned butterfly slab reached by the
    //! cell's `ButterflyHandle` (no self-referential interior pointer), the slab
    //! `props` mirror agrees with the `properties` value authority across the inline
    //! ->out-of-line offset boundary and across growth/realloc, the element API
    //! covers put/get/holes/delete/resize/pop, and the snapshot clone path yields an
    //! INDEPENDENT slab. (Accessor-in-slot is DEFERRED — see the cutover PAUSE.)
    use super::*;

    fn ident(n: u32) -> CorePropertyKey {
        CorePropertyKey::Identifier(n)
    }

    // (a) 6->64 boundary: cross INLINE_CAPACITY (6) into the out-of-line band with
    // distinct values; read each back BOTH through the offset/butterfly path and the
    // get_own_property VALUE path, proving no neighbor bleed and mirror==authority.
    #[test]
    fn butterfly_values_boundary_inline_to_out_of_line_no_neighbor_bleed() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        const N: u32 = 9; // 0..5 inline, 6..8 out-of-line
        for i in 0..N {
            store
                .put_data_own(obj, &ident(i), RuntimeValue::from_i32(i as i32 * 7 + 1))
                .unwrap();
        }
        let handle = store.find(obj).unwrap().butterfly;
        let sid = store.find(obj).unwrap().structure_id;
        for i in 0..N {
            let expected = RuntimeValue::from_i32(i as i32 * 7 + 1);
            let offset = store
                .structure_offset(sid, &ident(i))
                .expect("named offset");
            // offset/butterfly mirror path
            assert_eq!(
                store.butterfly_prop_get(handle, offset),
                Some(expected),
                "butterfly slot for prop {i}"
            );
            // value-authority path
            let prop = store
                .get_own_property(obj, &ident(i))
                .unwrap()
                .expect("own property");
            assert_eq!(
                prop.kind,
                CorePropertyKind::Data(expected),
                "value path for prop {i}"
            );
        }
    }

    // (b) GROWTH-SURVIVAL (the de-self-reference proof): write V at an early offset,
    // then add many properties to force the slab `props` Vec to realloc; the SAME
    // handle + offset still read V. A self-referential `*const` into the cell's own
    // Vec would dangle across this realloc; a slab handle does not.
    #[test]
    fn butterfly_values_growth_survival_offset_stable_across_realloc() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        store
            .put_data_own(obj, &ident(0), RuntimeValue::from_i32(0x0BEE))
            .unwrap();
        let handle = store.find(obj).unwrap().butterfly;
        let sid0 = store.find(obj).unwrap().structure_id;
        let off0 = store.structure_offset(sid0, &ident(0)).unwrap();
        assert_eq!(
            store.butterfly_prop_get(handle, off0),
            Some(RuntimeValue::from_i32(0x0BEE))
        );
        for i in 1..64 {
            store
                .put_data_own(obj, &ident(i), RuntimeValue::from_i32(i as i32))
                .unwrap();
        }
        // same handle, same offset, value preserved through every grow/realloc
        assert_eq!(
            store.butterfly_prop_get(handle, off0),
            Some(RuntimeValue::from_i32(0x0BEE)),
            "early offset must survive butterfly props realloc"
        );
        // and a late property reads back correctly (mirror==authority)
        let sid_late = store.find(obj).unwrap().structure_id;
        let off_late = store.structure_offset(sid_late, &ident(63)).unwrap();
        assert_eq!(
            store.butterfly_prop_get(handle, off_late),
            Some(RuntimeValue::from_i32(63))
        );
    }

    // (d) elements: put/get/hole/out-of-range/len, then delete (hole), resize (shrink),
    // push (append), pop — through BOTH the store array methods (which the cutover
    // re-routed) and the butterfly_elem_* slab API.
    #[test]
    fn butterfly_values_elements_put_get_delete_holes_resize_pop() {
        let mut store = CoreObjectStore::default();
        let arr = store.allocate_array();
        let handle = store.find(arr).unwrap().butterfly;

        store
            .put_array_element(arr, 0, RuntimeValue::from_i32(10))
            .unwrap();
        store
            .put_array_element(arr, 2, RuntimeValue::from_i32(30))
            .unwrap(); // index 1 = hole
        assert_eq!(store.get_index(arr, 0).unwrap(), RuntimeValue::from_i32(10));
        assert_eq!(
            store.butterfly_elem_get(handle, 0),
            Some(RuntimeValue::from_i32(10))
        );
        assert_eq!(store.butterfly_elem_get(handle, 1), None, "hole");
        assert_eq!(
            store.butterfly_elem_get(handle, 2),
            Some(RuntimeValue::from_i32(30))
        );
        assert_eq!(store.butterfly_elem_get(handle, 9), None, "out of range");
        assert_eq!(store.butterfly_elem_len(handle), 3);

        // delete arr[2] -> hole
        assert!(store.delete_index(arr, 2).unwrap());
        assert_eq!(store.butterfly_elem_get(handle, 2), None, "deleted -> hole");

        // push appends on the right
        store
            .push_array_element(arr, RuntimeValue::from_i32(40))
            .unwrap();
        assert_eq!(
            store.butterfly_elem_get(handle, 3),
            Some(RuntimeValue::from_i32(40))
        );
        assert_eq!(store.butterfly_elem_len(handle), 4);

        // pop removes the last
        assert_eq!(
            store.pop_array_element(arr).unwrap(),
            RuntimeValue::from_i32(40)
        );
        assert_eq!(store.butterfly_elem_len(handle), 3);

        // resize (shrink) drops the tail; offset 0 preserved
        store.resize_array_elements(arr, 1).unwrap();
        assert_eq!(store.butterfly_elem_len(handle), 1);
        assert_eq!(
            store.butterfly_elem_get(handle, 0),
            Some(RuntimeValue::from_i32(10))
        );
    }

    // (e) CLONE-INDEPENDENCE: the snapshot path is `CoreObjectStore::clone()`, which
    // deep-clones the WHOLE `butterflies` slab alongside `objects`. Handles are slab
    // indices preserved across the clone, so the cloned cell's butterfly is the SAME
    // index into an INDEPENDENT slab. Mutating the ORIGINAL's butterfly must not touch
    // the clone's. (Proves the shallow per-cell handle copy is sound.)
    #[test]
    fn butterfly_values_clone_independence_via_store_snapshot() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        store
            .put_data_own(obj, &ident(0), RuntimeValue::from_i32(111))
            .unwrap();
        let handle = store.find(obj).unwrap().butterfly;
        let sid = store.find(obj).unwrap().structure_id;
        let off0 = store.structure_offset(sid, &ident(0)).unwrap();
        store.butterfly_elem_put(handle, 0, RuntimeValue::from_i32(222));

        // snapshot (deep-clones objects + butterflies slab; handle index preserved)
        let clone = store.clone();

        // mutate the ORIGINAL's butterfly (both sides)
        store.butterfly_prop_put(handle, off0, RuntimeValue::from_i32(999));
        store.butterfly_elem_put(handle, 0, RuntimeValue::from_i32(888));

        // the CLONE's same-index butterfly is an INDEPENDENT allocation -> unchanged
        assert_eq!(
            clone.butterfly_prop_get(handle, off0),
            Some(RuntimeValue::from_i32(111)),
            "clone prop slot must be independent of the original"
        );
        assert_eq!(
            clone.butterfly_elem_get(handle, 0),
            Some(RuntimeValue::from_i32(222)),
            "clone element slot must be independent of the original"
        );
        // the ORIGINAL observes its own mutations
        assert_eq!(
            store.butterfly_prop_get(handle, off0),
            Some(RuntimeValue::from_i32(999))
        );
        assert_eq!(
            store.butterfly_elem_get(handle, 0),
            Some(RuntimeValue::from_i32(888))
        );
    }
}

#[cfg(test)]
mod getter_setter_prereq_tests {
    //! gc-r4 GetterSetter (B-i/B-ii/B-iii) verification.
    //!
    //! Proves the Structure+butterfly value model that the B-iv flip made authoritative:
    //! fresh-key accessor + Symbol-keyed properties take REAL Structure offsets (B-i
    //! Accessor bit + B-iii un-gate), an accessor's butterfly slot holds
    //! `from_cell(GetterSetter)` (B-ii), symbol-keyed siblings converge, and the
    //! Structure+butterfly value (now read back via `own_property_from_shape`, the per-cell
    //! `properties` HashMap being deleted) is internally consistent across data,
    //! symbol-keyed data, and accessor.
    use super::*;

    fn func(store: &mut CoreObjectStore, index: u32) -> RuntimeValue {
        store.allocate_function(index, Vec::new(), None)
    }

    // (a) A FRESH-key accessor and a Symbol-keyed data property now get REAL Structure
    // offsets; the accessor's structure attributes carry PropertyAttribute::Accessor
    // (1<<4), the symbol data property does NOT.
    #[test]
    fn accessor_and_symbol_get_real_offsets_with_accessor_bit() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let getter = func(&mut store, 0);
        let setter = func(&mut store, 1);
        let akey = CorePropertyKey::Identifier(7);
        store
            .define_accessor(obj, &akey, Some(getter), Some(setter))
            .unwrap();

        let sid = store.find(obj).unwrap().structure_id;
        let (aoff, aattrs) = store
            .structure_property(sid, &akey)
            .expect("fresh accessor must have a real structure offset");
        assert!(aoff.raw() >= 0, "accessor offset must be a real slot");
        assert_ne!(
            aattrs & PROPERTY_ATTRIBUTE_ACCESSOR,
            0,
            "structure attributes must carry the Accessor bit for an accessor"
        );

        // A Symbol-keyed DATA property also gets a real offset, with NO Accessor bit.
        let skey = CorePropertyKey::Symbol(0xBEEF);
        store
            .put_data_own(obj, &skey, RuntimeValue::from_i32(99))
            .unwrap();
        let sid2 = store.find(obj).unwrap().structure_id;
        let (soff, sattrs) = store
            .structure_property(sid2, &skey)
            .expect("symbol-keyed property must have a real structure offset");
        assert!(soff.raw() >= 0, "symbol offset must be a real slot");
        assert_eq!(
            sattrs & PROPERTY_ATTRIBUTE_ACCESSOR,
            0,
            "a data property (even symbol-keyed) must NOT carry the Accessor bit"
        );
    }

    // (b) Sibling convergence: two objects adding the SAME Symbol key from the same
    // empty shape converge on ONE structure id AND one offset (the monomorphic-IC
    // guarantee, now extended to symbols).
    #[test]
    fn symbol_keyed_siblings_converge_on_one_structure() {
        let mut store = CoreObjectStore::default();
        let a = store.allocate();
        let b = store.allocate();
        let skey = CorePropertyKey::Symbol(0x1234_5678);
        store
            .put_data_own(a, &skey, RuntimeValue::from_i32(1))
            .unwrap();
        store
            .put_data_own(b, &skey, RuntimeValue::from_i32(2))
            .unwrap();
        let sid_a = store.find(a).unwrap().structure_id;
        let sid_b = store.find(b).unwrap().structure_id;
        assert_eq!(
            sid_a, sid_b,
            "same symbol key from the same shape must converge on one structure"
        );
        assert_eq!(
            store.structure_offset(sid_a, &skey),
            store.structure_offset(sid_b, &skey),
            "converged siblings must share the symbol's offset"
        );
    }

    // (c) The GetterSetter cell stores the getter+setter, and the accessor's butterfly
    // slot holds `from_cell(getter_setter)` exactly as C++ stores a GetterSetter*.
    #[test]
    fn getter_setter_cell_lives_in_the_accessor_butterfly_slot() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let getter = func(&mut store, 0);
        let setter = func(&mut store, 1);
        let akey = CorePropertyKey::Identifier(3);
        store
            .define_accessor(obj, &akey, Some(getter), Some(setter))
            .unwrap();

        let handle = store.find(obj).unwrap().butterfly;
        let sid = store.find(obj).unwrap().structure_id;
        let (aoff, _) = store.structure_property(sid, &akey).unwrap();
        let slot = store
            .butterfly_prop_get(handle, aoff)
            .expect("accessor butterfly slot must be populated");
        let gs = store
            .find(slot)
            .expect("the butterfly slot value must be a cell ref (the GetterSetter)");
        assert_eq!(
            gs.kind,
            CoreObjectKind::GetterSetter,
            "the slot must reference a GetterSetter cell"
        );
        assert_eq!(gs.getter_value, Some(getter), "GetterSetter.m_getter");
        assert_eq!(gs.setter_value, Some(setter), "GetterSetter.m_setter");
    }

    // (d) Dual-write consistency: the structure+butterfly MIRROR agrees with the
    // `properties` HashMap authority across data, symbol-keyed data, and accessor — and
    // the structure attributes equal the encoder for each kind.
    #[test]
    fn dual_write_mirror_matches_hashmap_authority() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();

        let dkey = CorePropertyKey::Identifier(1);
        store
            .put_data_own(obj, &dkey, RuntimeValue::from_i32(42))
            .unwrap();
        let skey = CorePropertyKey::Symbol(0x77);
        store
            .put_data_own(obj, &skey, RuntimeValue::from_i32(7))
            .unwrap();
        let getter = func(&mut store, 0);
        let akey = CorePropertyKey::Identifier(2);
        store
            .define_accessor(obj, &akey, Some(getter), None)
            .unwrap();

        let handle = store.find(obj).unwrap().butterfly;
        let sid = store.find(obj).unwrap().structure_id;

        // DATA: structure attrs == data encoding; butterfly value == HashMap value.
        let (doff, dattrs) = store.structure_property(sid, &dkey).unwrap();
        assert_eq!(
            dattrs,
            core_attributes_to_u32(CorePropertyAttributes::DATA_DEFAULT, false)
        );
        assert_eq!(
            store.butterfly_prop_get(handle, doff),
            Some(RuntimeValue::from_i32(42))
        );
        assert_eq!(
            store.get_own_property(obj, &dkey).unwrap().unwrap().kind,
            CorePropertyKind::Data(RuntimeValue::from_i32(42))
        );

        // SYMBOL data: same invariants.
        let (soff, sattrs) = store.structure_property(sid, &skey).unwrap();
        assert_eq!(
            sattrs,
            core_attributes_to_u32(CorePropertyAttributes::DATA_DEFAULT, false)
        );
        assert_eq!(
            store.butterfly_prop_get(handle, soff),
            Some(RuntimeValue::from_i32(7))
        );
        assert_eq!(
            store.get_own_property(obj, &skey).unwrap().unwrap().kind,
            CorePropertyKind::Data(RuntimeValue::from_i32(7))
        );

        // ACCESSOR: structure attrs == accessor encoding (Accessor bit, no ReadOnly);
        // the butterfly GetterSetter's getter/setter == the HashMap authority's.
        let (aoff, aattrs) = store.structure_property(sid, &akey).unwrap();
        assert_eq!(
            aattrs,
            core_attributes_to_u32(CorePropertyAttributes::ACCESSOR_DEFAULT, true)
        );
        let gs = store
            .find(store.butterfly_prop_get(handle, aoff).unwrap())
            .unwrap();
        assert_eq!(gs.kind, CoreObjectKind::GetterSetter);
        match store.get_own_property(obj, &akey).unwrap().unwrap().kind {
            CorePropertyKind::Accessor { getter, setter } => {
                assert_eq!(gs.getter_value, getter, "mirror getter == authority getter");
                assert_eq!(gs.setter_value, setter, "mirror setter == authority setter");
            }
            CorePropertyKind::Data(_) => panic!("authority must be an accessor"),
        }
    }
}

#[cfg(test)]
mod b_iv_flip_tests {
    //! gc-r4 B-iv: the per-cell `properties` HashMap is DELETED — the Structure
    //! (offset + attributes) plus the butterfly (data value, or `from_cell(GetterSetter)`
    //! for an accessor) is the SOLE value authority. These prove reads reconstruct from
    //! the shape (own_property_from_shape), the in-place data<->accessor CONVERSION keeps
    //! the offset (the property does NOT vanish), symbol keys round-trip, enumeration
    //! order comes from the PropertyTable entry order, and non-configurable delete is
    //! still rejected.
    use super::*;

    fn func(store: &mut CoreObjectStore, index: u32) -> RuntimeValue {
        store.allocate_function(index, Vec::new(), None)
    }

    // (a) accessor get returns the getter; put routes to the setter — shape-driven.
    #[test]
    fn accessor_get_returns_getter_and_put_routes_to_setter() {
        let mut store = CoreObjectStore::default();
        let mut heap = Heap::new();
        let obj = store.allocate();
        let getter = func(&mut store, 0);
        let setter = func(&mut store, 1);
        let key = CorePropertyKey::Identifier(5);
        store
            .define_accessor(obj, &key, Some(getter), Some(setter))
            .unwrap();

        assert_eq!(
            store.get_property_from_prototype_chain(obj, &key).unwrap(),
            CorePropertyGet::Getter(getter),
            "own accessor get must surface the getter"
        );
        assert_eq!(
            store
                .put(&mut heap, obj, &key, RuntimeValue::from_i32(9))
                .unwrap(),
            CorePropertyPut::Setter(setter),
            "own accessor put must route to the setter"
        );
    }

    // (b) define_getter THEN define_setter on one key -> both halves readable.
    #[test]
    fn define_getter_then_setter_merges_both_halves() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let getter = func(&mut store, 0);
        let setter = func(&mut store, 1);
        let key = CorePropertyKey::Identifier(6);
        store
            .define_accessor(obj, &key, Some(getter), None)
            .unwrap();
        store
            .define_accessor(obj, &key, None, Some(setter))
            .unwrap();
        match store.get_own_property(obj, &key).unwrap().unwrap().kind {
            CorePropertyKind::Accessor {
                getter: g,
                setter: s,
            } => {
                assert_eq!(g, Some(getter), "getter half preserved across the merge");
                assert_eq!(s, Some(setter), "setter half merged in");
            }
            CorePropertyKind::Data(_) => panic!("expected accessor"),
        }
    }

    // (c) symbol-keyed data get/set round-trips through the butterfly.
    #[test]
    fn symbol_keyed_get_set_round_trips() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let skey = CorePropertyKey::Symbol(0xABCD);
        store
            .put_data_own(obj, &skey, RuntimeValue::from_i32(123))
            .unwrap();
        assert_eq!(
            store.get_own_property(obj, &skey).unwrap().unwrap().kind,
            CorePropertyKind::Data(RuntimeValue::from_i32(123))
        );
        store
            .put_data_own(obj, &skey, RuntimeValue::from_i32(456))
            .unwrap();
        assert_eq!(
            store.get_own_property(obj, &skey).unwrap().unwrap().kind,
            CorePropertyKind::Data(RuntimeValue::from_i32(456)),
            "overwrite must update the butterfly slot at the same offset"
        );
    }

    // (d) Object.keys order == PropertyTable insertion order over string + symbol +
    // deleted + re-added keys (a re-added key moves to the END, a fresh entry).
    #[test]
    fn own_property_keys_follow_property_table_insertion_order() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let a = CorePropertyKey::String("alpha".into());
        let b = CorePropertyKey::String("beta".into());
        let sym = CorePropertyKey::Symbol(0x9);
        let c = CorePropertyKey::String("gamma".into());
        for (k, v) in [(&a, 1), (&b, 2), (&sym, 3), (&c, 4)] {
            store
                .put_data_own(obj, k, RuntimeValue::from_i32(v))
                .unwrap();
        }
        assert!(store.delete_property(obj, &b).unwrap());
        store
            .put_data_own(obj, &b, RuntimeValue::from_i32(5))
            .unwrap();
        assert_eq!(
            store.own_property_keys(obj).unwrap(),
            vec![a.clone(), sym.clone(), c.clone(), b.clone()],
            "deleted+re-added key moves to the end of the entry order"
        );
    }

    // (e) THE CONVERSION TEST: data -> accessor get returns the getter (NOT None, the
    // property must not vanish), offset preserved; accessor -> data get returns the data
    // value. The load-bearing offset-stable attributeChange.
    #[test]
    fn data_accessor_conversion_keeps_property_visible_and_offset_stable() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let key = CorePropertyKey::Identifier(7);
        store
            .put_data_own(obj, &key, RuntimeValue::from_i32(10))
            .unwrap();
        let sid_data = store.find(obj).unwrap().structure_id;
        let off_data = store.structure_offset(sid_data, &key).expect("data offset");

        // data -> accessor
        let getter = func(&mut store, 0);
        store
            .define_accessor(obj, &key, Some(getter), None)
            .unwrap();
        match store.get_own_property(obj, &key).unwrap() {
            Some(property) => match property.kind {
                CorePropertyKind::Accessor { getter: g, .. } => {
                    assert_eq!(g, Some(getter), "conversion must surface the getter")
                }
                CorePropertyKind::Data(_) => panic!("expected accessor after conversion"),
            },
            None => panic!("data->accessor conversion made the property VANISH"),
        }
        let sid_acc = store.find(obj).unwrap().structure_id;
        assert_eq!(
            store.structure_offset(sid_acc, &key),
            Some(off_data),
            "accessor conversion must keep the property's offset (attributeChange)"
        );

        // accessor -> data
        store
            .put_data_own(obj, &key, RuntimeValue::from_i32(20))
            .unwrap();
        assert_eq!(
            store.get_own_property(obj, &key).unwrap().unwrap().kind,
            CorePropertyKind::Data(RuntimeValue::from_i32(20)),
            "accessor->data conversion must surface the data value"
        );
        let sid_back = store.find(obj).unwrap().structure_id;
        assert_eq!(
            store.structure_offset(sid_back, &key),
            Some(off_data),
            "accessor->data conversion must keep the offset"
        );
    }

    // (f) non-configurable delete still rejected; the property stays visible.
    #[test]
    fn non_configurable_property_delete_rejected() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let key = CorePropertyKey::Identifier(8);
        store
            .define_data_property(
                obj,
                &key,
                RuntimeValue::from_i32(1),
                CorePropertyAttributes {
                    writable: true,
                    enumerable: true,
                    configurable: false,
                },
            )
            .unwrap();
        assert!(
            !store.delete_property(obj, &key).unwrap(),
            "non-configurable delete must return false"
        );
        assert!(
            store.get_own_property(obj, &key).unwrap().is_some(),
            "the property must remain after a rejected delete"
        );
    }

    // Integer-string keys on an ORDINARY object route to INDEXED butterfly storage (the
    // faithful JSC model): NO named offset (so the named-property IC never arms), but the
    // value round-trips through get_own_property and enumerates numeric-first. Pre-flip
    // these were HashMap-only and would orphan once the HashMap is deleted.
    #[test]
    fn integer_string_key_on_ordinary_object_routes_to_indexed_storage() {
        let mut store = CoreObjectStore::default();
        let obj = store.allocate();
        let s = CorePropertyKey::String("name".into());
        let i = CorePropertyKey::String("5".into());
        store
            .put_data_own(obj, &s, RuntimeValue::from_i32(1))
            .unwrap();
        store
            .put_data_own(obj, &i, RuntimeValue::from_i32(2))
            .unwrap();
        let sid = store.find(obj).unwrap().structure_id;
        assert!(
            store.structure_offset(sid, &i).is_none(),
            "an integer-string key must NOT take a named offset (it is indexed)"
        );
        assert_eq!(
            store.get_own_property(obj, &i).unwrap().unwrap().kind,
            CorePropertyKind::Data(RuntimeValue::from_i32(2)),
            "the indexed value must round-trip through get_own_property"
        );
        // numeric index enumerates first (numeric order), then string keys.
        assert_eq!(
            store.own_enumerable_string_property_names(obj).unwrap(),
            vec!["5".to_string(), "name".to_string()]
        );
    }

    // (g) THE FLIP GATE: a randomized, fixed-seed property-based EQUIVALENCE oracle.
    //
    // Deleting the per-cell `properties` HashMap (the named-property VALUE authority) is
    // IRREVERSIBLE, so it must be gated by a technical refutation attempt, not a handful of
    // hand-picked cases. A deterministic PRNG drives a long sequence of own-property
    // mutations on a REAL object cell across MANY distinct shapes, using Identifier, String,
    // Symbol, AND integer-string keys. An in-test reference ORACLE (a plain `HashMap`
    // key->entry + an ordered live-key `Vec`) is advanced in lockstep by mirroring each
    // store PRIMITIVE's exact observable semantics. After EVERY op we assert the store
    // reconstructs the SAME observable own-property behavior from the Structure
    // (offset+attributes) + butterfly slot (the data value, or `from_cell(GetterSetter)` for
    // an accessor) that the oracle records:
    //   (a) every own get matches — accessor gets route to the getter
    //       (`get_property_from_prototype_chain`), sets route to the setter (`put`);
    //   (b) `own_property_keys` order == the oracle's ordered key list (indexed
    //       numeric-first, then named PropertyTable entry order; a re-added key moves to the
    //       END);
    //   (c) deleted / never-present keys read as ABSENT.
    // The sequence forces deletes+re-adds (offset recycling via
    // `PropertyTable::m_deletedOffsets`) and data<->accessor / attribute changes (the
    // offset-stable `convert_property_in_place` attributeChange path).
    //
    // FAITHFULNESS: offsets are an internal detail — the oracle models only JSC's OBSERVABLE
    // own-property semantics (JSObject [[Get]] / [[OwnPropertyKeys]] / [[DefineOwnProperty]],
    // runtime/JSObject.cpp), mirroring each store primitive: `put_data_own` == putDirect
    // (forces a DATA_DEFAULT data slot); `define_data_property` == a full-descriptor data
    // [[DefineOwnProperty]] (incl. the non-configurable ValidateAndApply rejection rules);
    // `define_accessor` == __defineGetter__/__defineSetter__ (ACCESSOR_DEFAULT attrs, merges
    // into an existing accessor's other half); integer-string keys route to indexed butterfly
    // storage (DATA_DEFAULT, numeric-first enumeration).
    #[test]
    fn randomized_shape_oracle_equivalence_after_each_op() {
        use std::collections::BTreeMap;

        // Deterministic xorshift64 — NOT rand/thread_rng — so the run is fully reproducible.
        struct Xorshift64(u64);
        impl Xorshift64 {
            fn next_u64(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.0 = x;
                x
            }
            fn below(&mut self, n: u64) -> u64 {
                self.next_u64() % n
            }
        }
        const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

        enum OracleKind {
            Data(RuntimeValue),
            Accessor {
                getter: Option<RuntimeValue>,
                setter: Option<RuntimeValue>,
            },
        }
        struct OracleEntry {
            kind: OracleKind,
            attrs: CorePropertyAttributes,
        }

        let mut store = CoreObjectStore::default();
        let mut heap = Heap::new();
        let mut rng = Xorshift64(SEED);

        // A tiny fixed function pool reused as getters/setters (cells never free — keep the
        // count small). Identity (RuntimeValue equality) is what accessor get-routing checks.
        let fns: Vec<RuntimeValue> = (0u32..4).map(|i| func(&mut store, i)).collect();

        // Key pool, partitioned by storage region:
        //   named -> PropertyTable named offset (Identifier / Symbol / non-index String)
        //   index -> indexed butterfly storage (integer-string keys; DATA_DEFAULT only)
        let named_keys: Vec<CorePropertyKey> = vec![
            CorePropertyKey::Identifier(101),
            CorePropertyKey::Identifier(102),
            CorePropertyKey::String("foo".into()),
            CorePropertyKey::String("bar".into()),
            CorePropertyKey::Symbol(0x5001),
            CorePropertyKey::Symbol(0x5002),
        ];
        let index_keys: Vec<CorePropertyKey> = vec![
            CorePropertyKey::String("0".into()),
            CorePropertyKey::String("2".into()),
            CorePropertyKey::String("5".into()),
        ];
        let all_keys: Vec<CorePropertyKey> = named_keys
            .iter()
            .chain(index_keys.iter())
            .cloned()
            .collect();

        // Mirrors the store's array-index routing (`parse_array_index_name`) for THIS pool:
        // only the integer-string keys parse, and they carry no leading zeros / huge values,
        // so a plain parse agrees exactly with `key_array_index` on every pool member.
        let index_of = |k: &CorePropertyKey| -> Option<usize> {
            match k {
                CorePropertyKey::String(s) => s.parse::<usize>().ok(),
                _ => None,
            }
        };

        const SHAPES: u32 = 20;
        const OPS_PER_SHAPE: u32 = 50;
        let mut total_ops = 0u32;

        for _shape in 0..SHAPES {
            let obj = store.allocate();
            // Oracle state for this shape's object.
            let mut entries: HashMap<CorePropertyKey, OracleEntry> = HashMap::new();
            let mut named_order: Vec<CorePropertyKey> = Vec::new();
            let mut index_live: BTreeMap<usize, RuntimeValue> = BTreeMap::new();

            for _op in 0..OPS_PER_SHAPE {
                total_ops += 1;
                let key = all_keys[rng.below(all_keys.len() as u64) as usize].clone();
                let index = index_of(&key);
                // Op selection. `define_accessor` on an integer-string key is a faithful
                // no-op in the store (it has no named offset), so index keys only see the
                // data-put / define-data / delete primitives.
                let op = if index.is_some() {
                    rng.below(3)
                } else {
                    rng.below(4)
                };
                match op {
                    0 => {
                        // putDirect: force key -> Data(value) with DATA_DEFAULT attributes.
                        let v = RuntimeValue::from_i32(rng.below(1000) as i32);
                        store.put_data_own(obj, &key, v).unwrap();
                        if let Some(i) = index {
                            index_live.insert(i, v);
                        } else {
                            if !entries.contains_key(&key) {
                                named_order.push(key.clone());
                            }
                            entries.insert(
                                key.clone(),
                                OracleEntry {
                                    kind: OracleKind::Data(v),
                                    attrs: CorePropertyAttributes::DATA_DEFAULT,
                                },
                            );
                        }
                    }
                    1 => {
                        // [[DefineOwnProperty]] with a full data descriptor.
                        let v = RuntimeValue::from_i32(rng.below(1000) as i32);
                        let attrs = CorePropertyAttributes {
                            writable: rng.below(2) == 1,
                            enumerable: rng.below(2) == 1,
                            configurable: rng.below(2) == 1,
                        };
                        let store_ok = store.define_data_property(obj, &key, v, attrs).unwrap();
                        if let Some(i) = index {
                            // Index keys route to indexed storage (DATA_DEFAULT); the
                            // requested attributes are ignored and the define always succeeds.
                            assert!(store_ok, "define_data on an index key always succeeds");
                            index_live.insert(i, v);
                        } else {
                            // Mirror the store's non-configurable ValidateAndApply rejection.
                            let oracle_ok = match entries.get(&key) {
                                Some(cur) if !cur.attrs.configurable => {
                                    if attrs.configurable
                                        || attrs.enumerable != cur.attrs.enumerable
                                    {
                                        false
                                    } else {
                                        match &cur.kind {
                                            OracleKind::Accessor { .. } => false,
                                            OracleKind::Data(cur_v) => {
                                                !(!cur.attrs.writable
                                                    && (attrs.writable || *cur_v != v))
                                            }
                                        }
                                    }
                                }
                                _ => true,
                            };
                            assert_eq!(
                                store_ok, oracle_ok,
                                "define_data_property accept/reject must match the oracle"
                            );
                            if oracle_ok {
                                if !entries.contains_key(&key) {
                                    named_order.push(key.clone());
                                }
                                entries.insert(
                                    key.clone(),
                                    OracleEntry {
                                        kind: OracleKind::Data(v),
                                        attrs,
                                    },
                                );
                            }
                        }
                    }
                    2 => {
                        // delete.
                        let store_ok = store.delete_property(obj, &key).unwrap();
                        if let Some(i) = index {
                            assert!(store_ok, "index delete always succeeds (hole punch)");
                            index_live.remove(&i);
                        } else {
                            let non_conf =
                                matches!(entries.get(&key), Some(cur) if !cur.attrs.configurable);
                            assert_eq!(
                                store_ok, !non_conf,
                                "delete returns false iff the property is non-configurable"
                            );
                            if store_ok && entries.remove(&key).is_some() {
                                named_order.retain(|k| k != &key);
                            }
                        }
                    }
                    _ => {
                        // __defineGetter__/__defineSetter__: ACCESSOR_DEFAULT attrs; merges
                        // into an existing accessor's other half, else replaces. (named keys
                        // only — guaranteed by the op-selection branch above.)
                        let pick = rng.below(3); // 0 getter-only, 1 setter-only, 2 both
                        let getter = (pick != 1).then(|| fns[rng.below(fns.len() as u64) as usize]);
                        let setter = (pick != 0).then(|| fns[rng.below(fns.len() as u64) as usize]);
                        store.define_accessor(obj, &key, getter, setter).unwrap();
                        let (mut g, mut s) = match entries.get(&key) {
                            Some(OracleEntry {
                                kind: OracleKind::Accessor { getter, setter },
                                ..
                            }) => (*getter, *setter),
                            _ => (None, None),
                        };
                        if getter.is_some() {
                            g = getter;
                        }
                        if setter.is_some() {
                            s = setter;
                        }
                        if !entries.contains_key(&key) {
                            named_order.push(key.clone());
                        }
                        entries.insert(
                            key.clone(),
                            OracleEntry {
                                kind: OracleKind::Accessor {
                                    getter: g,
                                    setter: s,
                                },
                                attrs: CorePropertyAttributes::ACCESSOR_DEFAULT,
                            },
                        );
                    }
                }

                // ---- Equivalence assertions after EVERY op ----

                // (a) + (c): probe EVERY pool key — live -> Some(matching kind+attrs),
                // deleted/never-present -> None.
                for probe in &all_keys {
                    let got = store.get_own_property(obj, probe).unwrap();
                    if let Some(i) = index_of(probe) {
                        match index_live.get(&i) {
                            Some(v) => {
                                let p = got.expect("live index key must be present");
                                assert_eq!(
                                    p.kind,
                                    CorePropertyKind::Data(*v),
                                    "index value mismatch"
                                );
                                assert_eq!(
                                    p.attributes,
                                    CorePropertyAttributes::DATA_DEFAULT,
                                    "index key attributes must be DATA_DEFAULT"
                                );
                            }
                            None => {
                                assert!(got.is_none(), "deleted index key must read absent")
                            }
                        }
                        continue;
                    }
                    match entries.get(probe) {
                        None => assert!(
                            got.is_none(),
                            "deleted / never-present named key must read absent"
                        ),
                        Some(entry) => {
                            let p = got.expect("live named key must be present");
                            assert_eq!(p.attributes, entry.attrs, "attributes mismatch");
                            match &entry.kind {
                                OracleKind::Data(v) => assert_eq!(
                                    p.kind,
                                    CorePropertyKind::Data(*v),
                                    "data value mismatch"
                                ),
                                OracleKind::Accessor { getter, setter } => {
                                    assert_eq!(
                                        p.kind,
                                        CorePropertyKind::Accessor {
                                            getter: *getter,
                                            setter: *setter,
                                        },
                                        "accessor halves mismatch"
                                    );
                                    // (a) get routes to the getter; put routes to the setter.
                                    // Both calls are non-mutating for an accessor slot.
                                    let read = store
                                        .get_property_from_prototype_chain(obj, probe)
                                        .unwrap();
                                    match getter {
                                        Some(g) => assert_eq!(
                                            read,
                                            CorePropertyGet::Getter(*g),
                                            "accessor get must surface the getter"
                                        ),
                                        None => assert_eq!(
                                            read,
                                            CorePropertyGet::AccessorWithoutGetter,
                                            "getter-less accessor get"
                                        ),
                                    }
                                    let put = store
                                        .put(&mut heap, obj, probe, RuntimeValue::from_i32(7))
                                        .unwrap();
                                    match setter {
                                        Some(st) => assert_eq!(
                                            put,
                                            CorePropertyPut::Setter(*st),
                                            "accessor put must route to the setter"
                                        ),
                                        None => assert_eq!(
                                            put,
                                            CorePropertyPut::IgnoredGetterOnly,
                                            "setter-less accessor put is ignored"
                                        ),
                                    }
                                }
                            }
                        }
                    }
                }

                // (b): own enumeration order == indexed (numeric order) ++ named (entry order).
                let mut expected: Vec<CorePropertyKey> = index_live
                    .keys()
                    .map(|i| CorePropertyKey::String(i.to_string()))
                    .collect();
                expected.extend(named_order.iter().cloned());
                assert_eq!(
                    store.own_property_keys(obj).unwrap(),
                    expected,
                    "own enumeration order must match the oracle's ordered key list"
                );
            }
        }

        // Bounded, deterministic exercise volume (offset recycling + convert-in-place) with
        // no allocation explosion — a fast unit test, not a fuzzer.
        assert_eq!(total_ops, SHAPES * OPS_PER_SHAPE);
    }
}

#[cfg(test)]
mod trace_cell_gap_a_tests {
    //! gc-r4 GAP A — `CoreObjectStore::trace_cell` (the live `CoreObjectCell`
    //! `visitChildren`) fidelity. These prove the trace visits EVERY `RuntimeValue`
    //! GC edge (inline slots + the butterfly + the per-kind store-owned aux slabs)
    //! and ONLY those: non-cell immediates, butterfly holes, and the non-edge slabs
    //! (`regexp_sources` text, `array_buffer_backings` bytes) contribute nothing.
    //! No collection is run (R4-gated); a RECORDING visitor stands in for the real
    //! SlotVisitor and never dereferences an edge.
    use super::*;
    use core::ptr::NonNull;

    /// Records the cell-payload bits of every edge the trace appends.
    #[derive(Default)]
    struct RecordingEdgeVisitor {
        visited: Vec<usize>,
    }

    impl CellEdgeVisitor for RecordingEdgeVisitor {
        fn visit_cell_edge(&mut self, cell: CellValue) {
            self.visited.push(cell.pointer_payload_bits());
        }
    }

    /// A recognizable cell-tagged `RuntimeValue` whose `pointer_payload_bits()`
    /// round-trips to `addr` under both the transitional and `s4_raw_cell`
    /// encodings.
    fn fake_cell(addr: usize) -> RuntimeValue {
        // SAFETY: `from_cell`/`pointer_payload_bits` only encode and round-trip the
        // bits; the trace and the recording visitor NEVER dereference the edge, so
        // the live/pinned-cell precondition of `GcRef::from_non_null` is vacuous.
        let ptr = NonNull::new(addr as *mut u8).expect("non-null fake cell address");
        RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
    }

    fn traced(store: &CoreObjectStore, cell: &CoreObjectCell) -> Vec<usize> {
        let mut visitor = RecordingEdgeVisitor::default();
        store.trace_cell(cell, &mut visitor);
        let mut visited = visitor.visited;
        visited.sort_unstable();
        visited
    }

    fn sorted(mut v: Vec<usize>) -> Vec<usize> {
        v.sort_unstable();
        v
    }

    #[test]
    fn default_cell_has_no_edges() {
        // INVALID handles (every aux slab) + Empty-sentinel inline slots + None
        // optionals => zero edges. Proves no spurious edge and that INVALID-handle
        // slabs and the Empty sentinel are all skipped.
        let store = CoreObjectStore::default();
        let cell = CoreObjectCell::default();
        assert!(traced(&store, &cell).is_empty());
    }

    #[test]
    fn object_prototype_and_butterfly_edges_visited_holes_and_immediates_skipped() {
        let mut store = CoreObjectStore::default();
        let butterfly = store.allocate_butterfly();
        // Out-of-line property storage (left side): two cell values.
        store.butterflies[butterfly.0].props = vec![fake_cell(0x1000), fake_cell(0x2000)];
        // Indexed elements (right side): a cell, a hole (None), an immediate.
        store.butterflies[butterfly.0].elements = vec![
            Some(fake_cell(0x3000)),
            None,
            Some(RuntimeValue::from_i32(7)),
        ];

        let cell = CoreObjectCell {
            butterfly,
            prototype: Some(fake_cell(0x10)),
            getter_value: Some(fake_cell(0x11)),
            // A non-Option inline slot holding an immediate must be filtered out.
            binding_value: RuntimeValue::from_i32(99),
            ..CoreObjectCell::default()
        };

        // 0x3000 element (cell) visited; the hole and the immediate element are not.
        let expected = sorted(vec![0x10, 0x11, 0x1000, 0x2000, 0x3000]);
        assert_eq!(traced(&store, &cell), expected);
    }

    #[test]
    fn per_kind_aux_slab_edges_visited_and_non_edge_slabs_skipped() {
        let mut store = CoreObjectStore::default();

        // Map entries: both sides are edges; a cell key with an immediate value,
        // and an immediate key with a cell value, prove BOTH sides are filtered.
        store.map_entry_lists.push(vec![
            (fake_cell(0x20), fake_cell(0x21)),
            (fake_cell(0x22), RuntimeValue::from_i32(5)),
            (RuntimeValue::from_i32(50), fake_cell(0x34)),
        ]);
        let map_entries = AuxiliaryHandle(store.map_entry_lists.len() - 1);

        // Set values: one cell, one immediate (skipped).
        store
            .set_value_lists
            .push(vec![fake_cell(0x23), RuntimeValue::from_i32(6)]);
        let set_values = AuxiliaryHandle(store.set_value_lists.len() - 1);

        // Bound-function [[BoundArguments]]: one cell, one immediate.
        store
            .bound_args_backings
            .push(vec![fake_cell(0x24), RuntimeValue::from_i32(8)]);
        let bound_args = AuxiliaryHandle(store.bound_args_backings.len() - 1);

        // Closure captures.
        store.captures_backings.push(vec![fake_cell(0x25)]);
        let captures = AuxiliaryHandle(store.captures_backings.len() - 1);

        // Class instance fields: the interned key uid is NOT an edge; the present
        // initializer is; a `None` initializer is skipped.
        store.instance_field_lists.push(vec![
            CoreInstanceFieldRecord {
                key_uid: AtomId::from_table_slot(1),
                initializer: Some(fake_cell(0x26)),
            },
            CoreInstanceFieldRecord {
                key_uid: AtomId::from_table_slot(2),
                initializer: None,
            },
        ]);
        let instance_fields = AuxiliaryHandle(store.instance_field_lists.len() - 1);

        // Pending promise reaction: result_promise + on_fulfilled are edges;
        // on_rejected here is an immediate (skipped); `kind` is not an edge.
        store.promise_reaction_lists.push(vec![CorePromiseReaction {
            kind: CorePromiseReactionKind::Then,
            result_promise: fake_cell(0x27),
            on_fulfilled: fake_cell(0x28),
            on_rejected: RuntimeValue::from_i32(9),
        }]);
        let promise_reactions = PromiseReactionsHandle(store.promise_reaction_lists.len() - 1);

        // NON-EDGE slabs that MUST be skipped: a RegExp pattern String and raw
        // ArrayBuffer bytes. Planted with real handles so the test proves the trace
        // ignores them (they contribute zero edges), not that the handles are absent.
        store.regexp_sources.push("ab+".to_string());
        let regexp_source = AuxiliaryHandle(store.regexp_sources.len() - 1);
        store.array_buffer_backings.push(vec![1_u8, 2, 3]);
        let array_buffer_data = AuxiliaryHandle(store.array_buffer_backings.len() - 1);

        let cell = CoreObjectCell {
            map_entries,
            set_values,
            bound_args,
            captures,
            instance_fields,
            promise_reactions,
            regexp_source,
            array_buffer_data,
            // A spread of inline RuntimeValue edges across kinds.
            super_base: Some(fake_cell(0x31)),
            super_constructor: Some(fake_cell(0x32)),
            native_bound_promise: Some(fake_cell(0x2e)),
            primitive_value: Some(fake_cell(0x2f)),
            view_buffer: Some(fake_cell(0x30)),
            proxy_target: Some(fake_cell(0x2b)),
            proxy_handler: Some(fake_cell(0x2c)),
            bound_target: Some(fake_cell(0x29)),
            bound_this: fake_cell(0x2a),
            promise_result: fake_cell(0x2d),
            setter_value: Some(fake_cell(0x33)),
            ..CoreObjectCell::default()
        };

        let expected = sorted(vec![
            // map: keys + values (immediate value of entry 2 and immediate key of
            // entry 3 skipped)
            0x20, 0x21, 0x22, 0x34, //
            0x23, // set value (immediate skipped)
            0x24, // bound arg (immediate skipped)
            0x25, // capture
            0x26, // instance-field initializer (None skipped)
            0x27, 0x28, // promise result_promise + on_fulfilled (on_rejected skipped)
            // inline slots
            0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33,
        ]);
        // Exact set equality also proves the regexp String + ArrayBuffer bytes
        // slabs contributed NOTHING (no extra entries appeared).
        assert_eq!(traced(&store, &cell), expected);
    }
}
