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
            // gc-r4 B1a: deep-clone the whole butterfly slab by index so every
            // `ButterflyHandle` stays valid across the snapshot (handles are slab
            // indices; the slab is currently unwired, so this is empty in practice
            // until the cutover threads butterflies onto cells).
            butterflies: self.butterflies.clone(),
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
//   - storage_ptr SECOND  => STORAGE_PTR_DISP == 8 (the JSObject Butterfly-pointer
//     slot analog), a cached pointer into out_of_line_storage so the codegen can
//     load [base + STORAGE_PTR_DISP] then [storage_ptr + offset*8] with no Vec
//     bookkeeping. The header order is LOAD-BEARING and enforced by the
//     const offset_of! asserts below.
// DIVERGENCE: Clone is hand-written (see impl Clone) because storage_ptr is a raw
// pointer into this cell's own out_of_line_storage and must be RECOMPUTED for a
// clone, never copied (a copied ptr would dangle/alias into the source cell's Vec).
// Default is likewise hand-written because *const has no derivable Default.
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
    // C++ JSC: the JSObject Butterfly pointer slot (runtime/JSObject.h:1572-1577).
    // Rust butterfly-pointer analog: a cached `out_of_line_storage.as_ptr()` kept
    // coherent by refresh_storage_ptr at every Vec mutation. MUST stay the second
    // declared field; STORAGE_PTR_DISP asserts it is at byte 8 (after the 4-byte
    // structure_id + 4-byte pad to pointer alignment). NEVER dereferenced without a
    // prior matching structure guard: for an empty Vec this is a dangling-but-aligned
    // pointer (Vec::as_ptr on a 0-capacity Vec), and a 0-slot shape has no valid
    // offset to read, so the guard makes that pointer unreachable.
    pub(crate) storage_ptr: *const RuntimeValue,
    pub(crate) cell_id: CellId,
    pub(crate) kind: CoreObjectKind,
    pub(crate) prototype: Option<RuntimeValue>,
    pub(crate) function_index: Option<u32>,
    pub(crate) native_function: Option<CoreNativeFunction>,
    pub(crate) construct_ability: ConstructAbility,
    pub(crate) super_base: Option<RuntimeValue>,
    pub(crate) super_constructor: Option<RuntimeValue>,
    pub(crate) is_default_derived_constructor: bool,
    pub(crate) instance_fields: Vec<CoreInstanceField>,
    pub(crate) captures: Vec<RuntimeValue>,
    pub(crate) binding_value: RuntimeValue,
    pub(crate) properties: HashMap<CorePropertyKey, CoreProperty>,
    // C++ JSC: a JSObject's named data properties live in either inline storage
    // (offset < firstOutOfLineOffset) or the Butterfly out-of-line region
    // (JSObject.h locationForOffset:711, Butterfly.h), and putDirectOffset writes
    // the value at offsetInRespectiveStorage(offset). `out_of_line_storage` is the
    // Rust mirror of that Butterfly property region: a contiguous [RuntimeValue]
    // (RuntimeValue == EncodedJsValue == 8 bytes), indexable as [base + idx*8], which
    // is what the batch-3 machine-code GET_BY_ID will mov from. The HashMap
    // `properties` remains the authoritative VALUE store this batch; this Vec is
    // written in lockstep (the putDirectOffset analog) and read by the offset path.
    // The property->OFFSET map is no longer per-cell (gc-r4 Batch 2): the offset is a
    // function of the cell's `structure_id` via the store's `structure_table`
    // (Structure::PropertyTable), exactly as C++ JSC keeps it per-shape.
    //
    // DIVERGENCE: C++ indexes the out-of-line region with NEGATIVE indices growing
    // backward from the Butterfly base (offsetInOutOfLineStorage returns
    // -(offset-firstOutOfLineOffset)-1). The Rust mirror uses a FORWARD-indexed Vec
    // (slot index = offset_storage_index(offset)); for the batch-3 base register the
    // sign of the displacement is a codegen detail, and forward indexing is the
    // natural Rust spill. With INLINE_CAPACITY == 6 (the JSFinalObject default), the
    // inline band [0,6) and the out-of-line band [64,...) are both packed into this
    // single forward Vec; the real inline-slot/Butterfly split is deferred to Batch 5.
    pub(crate) out_of_line_storage: Vec<RuntimeValue>,
    // C++ JSC: PropertyTable::m_deletedOffsets (PropertyTable.h) records offsets
    // freed by deletion so a later addition can reuse them instead of growing
    // storage. The Rust mirror records freed offsets here; reuse is not yet wired
    // into the offset allocator (the allocator still monotonically increments
    // next_property_offset), so this currently only tracks freed slots and clears
    // them. Faithful reuse is deferred with the inline-split work.
    pub(crate) deleted_offsets: Vec<PropertyOffset>,
    pub(crate) property_order: Vec<CorePropertyKey>,
    pub(crate) elements: Vec<Option<RuntimeValue>>,
    pub(crate) map_entries: Vec<(RuntimeValue, RuntimeValue)>,
    pub(crate) set_values: Vec<RuntimeValue>,
    pub(crate) regexp_source: String,
    pub(crate) regexp_flags: RegexFlags,
    pub(crate) regexp_flags_text: String,
    pub(crate) promise_state: PromiseState,
    pub(crate) promise_result: RuntimeValue,
    pub(crate) promise_reactions: Vec<CorePromiseReaction>,
    pub(crate) promise_resolving_kind: Option<CorePromiseResolvingKind>,
    pub(crate) native_bound_promise: Option<RuntimeValue>,
    pub(crate) native_bound_proxy: Option<RuntimeValue>,
    /// C++ JSC: NumberObject/BooleanObject/StringObject internal value.
    /// Mirrors JSC's NumberObject::internalValue() / BooleanObject::internalValue().
    pub(crate) primitive_value: Option<RuntimeValue>,
    pub(crate) date_value: f64,
    pub(crate) array_buffer_data: Vec<u8>,
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
    pub(crate) bound_target: Option<RuntimeValue>,
    pub(crate) bound_this: RuntimeValue,
    pub(crate) bound_args: Vec<RuntimeValue>,
}

// C++ JSC JSCell::structureIDOffset()==0 (runtime/JSCell.h:293): the StructureID
// (a 4-byte id) is the first header word so a guard can `load32 [base+0]; cmp32`.
// The batch-3 assembler takes structure_id_offset as a parameter; this const is the
// value it must be given, and the assert pins the field at byte 0 so a silent
// field-reorder cannot desynchronize the codegen from the layout.
const STRUCTURE_ID_OFFSET: usize = std::mem::offset_of!(CoreObjectCell, structure_id);
// C++ JSC: the JSObject Butterfly pointer slot (runtime/JSObject.h:1572-1577) read
// at a constant displacement. STORAGE_PTR_DISP is the Rust analog displacement the
// codegen uses to fetch the storage base before the offset-indexed property load.
const STORAGE_PTR_DISP: usize = std::mem::offset_of!(CoreObjectCell, storage_ptr);

// Compile-time layout guards. These fail the build if the #[repr(C)] header order
// changes, if alignment padding shifts the storage pointer, or if RuntimeValue stops
// being an 8-byte EncodedJsValue (the [storage_ptr + offset*8] stride assumption).
const _: () = assert!(
    STRUCTURE_ID_OFFSET == 0,
    "CoreObjectCell::structure_id must be at offset 0 (JSCell::structureIDOffset()==0)"
);
const _: () = assert!(
    STORAGE_PTR_DISP == 8,
    "CoreObjectCell::storage_ptr must be at byte 8 (JSObject Butterfly-pointer slot analog)"
);
const _: () = assert!(
    std::mem::size_of::<RuntimeValue>() == 8,
    "RuntimeValue must be 8 bytes (EncodedJsValue) for the [storage_ptr + offset*8] stride"
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
        // Build with a dangling-but-aligned storage_ptr, then point it at this cell's
        // own (empty) out_of_line_storage. C++ has no exact analog (a fresh JSObject's
        // Butterfly is null until out-of-line storage is needed); the Rust mirror keeps
        // storage_ptr always pointing into its own Vec so refresh_storage_ptr has a
        // single invariant to maintain. The empty-Vec pointer is never read without a
        // prior matching structure guard (a 0-slot shape has no valid offset).
        let mut cell = Self {
            structure_id: StructureId::default(),
            // Default kind is Ordinary => FinalObject; allocate_cell overwrites this
            // from cell.kind.js_type() for every published cell, so the tag always
            // matches the final kind regardless of how the cell was built.
            js_type: JsType::FinalObject,
            storage_ptr: core::ptr::null(),
            cell_id: CellId::default(),
            kind: CoreObjectKind::default(),
            prototype: None,
            function_index: None,
            native_function: None,
            construct_ability: ConstructAbility::default(),
            super_base: None,
            super_constructor: None,
            is_default_derived_constructor: false,
            instance_fields: Vec::new(),
            captures: Vec::new(),
            binding_value: RuntimeValue::default(),
            properties: HashMap::new(),
            out_of_line_storage: Vec::new(),
            deleted_offsets: Vec::new(),
            property_order: Vec::new(),
            elements: Vec::new(),
            map_entries: Vec::new(),
            set_values: Vec::new(),
            regexp_source: String::new(),
            regexp_flags: RegexFlags::default(),
            regexp_flags_text: String::new(),
            promise_state: PromiseState::default(),
            promise_result: RuntimeValue::default(),
            promise_reactions: Vec::new(),
            promise_resolving_kind: None,
            native_bound_promise: None,
            native_bound_proxy: None,
            primitive_value: None,
            date_value: 0.0,
            array_buffer_data: Vec::new(),
            view_buffer: None,
            view_byte_offset: 0,
            view_byte_length: 0,
            view_length: 0,
            view_element_kind: TypedArrayElementKind::default(),
            proxy_target: None,
            proxy_handler: None,
            bound_target: None,
            bound_this: RuntimeValue::default(),
            bound_args: Vec::new(),
        };
        cell.refresh_storage_ptr();
        cell
    }
}

impl Clone for CoreObjectCell {
    fn clone(&self) -> Self {
        // Clone every field normally, but RECOMPUTE storage_ptr from the CLONE's own
        // out_of_line_storage. Copying self.storage_ptr would alias/dangle into the
        // source cell's Vec (a different heap allocation), which the batch-3 codegen
        // would then dereference. This is the one field that must NOT be a value copy.
        // Vec's heap buffer pointer is stable across the subsequent move of this struct
        // (into Box/Pin on the snapshot path), so computing from the new Vec here stays
        // valid after the cell is re-pinned.
        let mut cloned = Self {
            structure_id: self.structure_id,
            // Copy the type tag normally (unlike storage_ptr, it is layout/identity-
            // independent); a clone of an object cell is the same JSType.
            js_type: self.js_type,
            storage_ptr: core::ptr::null(),
            cell_id: self.cell_id,
            kind: self.kind,
            prototype: self.prototype,
            function_index: self.function_index,
            native_function: self.native_function.clone(),
            construct_ability: self.construct_ability,
            super_base: self.super_base,
            super_constructor: self.super_constructor,
            is_default_derived_constructor: self.is_default_derived_constructor,
            instance_fields: self.instance_fields.clone(),
            captures: self.captures.clone(),
            binding_value: self.binding_value,
            properties: self.properties.clone(),
            out_of_line_storage: self.out_of_line_storage.clone(),
            deleted_offsets: self.deleted_offsets.clone(),
            property_order: self.property_order.clone(),
            elements: self.elements.clone(),
            map_entries: self.map_entries.clone(),
            set_values: self.set_values.clone(),
            regexp_source: self.regexp_source.clone(),
            regexp_flags: self.regexp_flags,
            regexp_flags_text: self.regexp_flags_text.clone(),
            promise_state: self.promise_state,
            promise_result: self.promise_result,
            promise_reactions: self.promise_reactions.clone(),
            promise_resolving_kind: self.promise_resolving_kind,
            native_bound_promise: self.native_bound_promise,
            native_bound_proxy: self.native_bound_proxy,
            primitive_value: self.primitive_value,
            date_value: self.date_value,
            array_buffer_data: self.array_buffer_data.clone(),
            view_buffer: self.view_buffer,
            view_byte_offset: self.view_byte_offset,
            view_byte_length: self.view_byte_length,
            view_length: self.view_length,
            view_element_kind: self.view_element_kind,
            proxy_target: self.proxy_target,
            proxy_handler: self.proxy_handler,
            bound_target: self.bound_target,
            bound_this: self.bound_this,
            bound_args: self.bound_args.clone(),
        };
        cloned.refresh_storage_ptr();
        cloned
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
            | CoreObjectKind::Proxy => JsType::Object,
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
    pub(crate) fn matches(self, key: &CorePropertyKey) -> bool {
        match (self, key) {
            (Self::Identifier(expected), CorePropertyKey::Identifier(actual)) => {
                expected == *actual
            }
            (Self::String(expected), CorePropertyKey::String(actual)) => expected == actual,
            _ => false,
        }
    }

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
    cell: &CoreObjectCell,
    key: GeneratedPropertyLoadCoreKey<'_>,
) -> bool {
    cell.properties
        .keys()
        .any(|stored_key| key.matches(stored_key))
}

pub(crate) fn generated_property_load_cell_data_property_at_offset(
    cell: &CoreObjectCell,
    key: GeneratedPropertyLoadCoreKey<'_>,
    expected_offset: PropertyOffset,
) -> Option<RuntimeValue> {
    // C++ JSC JSObject::getDirect(offset)/locationForOffset (JSObject.h:711,748):
    // once the structure guard holds (verified by the caller against
    // entry.structure / base_structure), the value is read directly at the
    // structure-assigned offset with NO key comparison or HashMap scan. This is
    // exactly the offset-indexed load batch 3 will emit as
    // `mov reg <- [storage_base + offset*8]` from out_of_line_storage.
    //
    // gc-r4 Batch 2: the offset is now owned by the cell's Structure::PropertyTable
    // (per-shape), so the structure guard ALONE proves `expected_offset` is the live
    // own-data slot for this shape (the faithful JSC IC invariant). The former
    // per-cell offset-map recheck is removed with `property_offsets`; `key` is kept in
    // the signature for the call shape but is not needed once the shape is guarded.
    let _ = key;
    cell.read_data_property_offset_slot(expected_offset)
}

pub(crate) fn generated_property_load_offset_miss_reason(
    cell: &CoreObjectCell,
    key: GeneratedPropertyLoadCoreKey<'_>,
    expected_offset: PropertyOffset,
    actual_offset: Option<PropertyOffset>,
) -> GeneratedPropertyLoadProbeMissReason {
    use GeneratedPropertyLoadProbeMissReason as Miss;

    // Diagnostic-only classification when the offset-indexed read returned None.
    // `properties` (the authoritative VALUE store, kept this batch) localizes whether
    // the key is absent or non-data; `actual_offset` is the key's real offset in the
    // cell's Structure::PropertyTable (the offset authority), supplied by the caller so
    // a cached offset that disagrees with the structure is reported as KeyOffsetMismatch.
    let Some((_stored_key, property)) = cell
        .properties
        .iter()
        .find(|(stored_key, _)| key.matches(stored_key))
    else {
        return Miss::MissingProperty;
    };
    if !matches!(property.kind, CorePropertyKind::Data(_)) {
        return Miss::NonDataProperty;
    }
    match actual_offset {
        Some(actual) if actual != expected_offset => Miss::KeyOffsetMismatch,
        _ => Miss::MissingOrInvalidOffset,
    }
}

pub(crate) fn core_property_key_supports_named_property_offset(key: &CorePropertyKey) -> bool {
    matches!(
        key,
        CorePropertyKey::Identifier(_) | CorePropertyKey::String(_)
    ) && key_array_index(key).is_none()
}

/// Encode `CorePropertyAttributes` as the `unsigned attributes` bitfield the ported
/// Structure transition table + PropertyTable key on (C++ runtime/PropertyAttribute.h:
/// ReadOnly == 1<<1, DontEnum == 1<<2, DontDelete == 1<<3). Only the writable/
/// enumerable/configurable trio the interpreter models is encoded; the mapping is
/// injective over those 8 combinations so distinct attribute sets produce distinct
/// transition edges (the authoritative attribute VALUES stay in `properties`).
pub(crate) fn core_attributes_to_u32(attributes: CorePropertyAttributes) -> u32 {
    let mut bits = 0u32;
    if !attributes.writable {
        bits |= 1 << 1; // ReadOnly
    }
    if !attributes.enumerable {
        bits |= 1 << 2; // DontEnum
    }
    if !attributes.configurable {
        bits |= 1 << 3; // DontDelete
    }
    bits
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

impl CoreObjectCell {
    /// Re-point storage_ptr at the current out_of_line_storage buffer.
    ///
    /// C++ JSC keeps the JSObject Butterfly pointer (runtime/JSObject.h:1572-1577)
    /// coherent whenever the Butterfly is (re)allocated. storage_ptr is the Rust
    /// analog and MUST be refreshed after EVERY out_of_line_storage mutation that can
    /// move the buffer (clear, resize/realloc); all such sites route through here.
    /// Vec::as_ptr on an empty Vec yields a dangling-but-aligned pointer that is never
    /// read without a prior matching structure guard (a 0-slot shape has no offset).
    pub(crate) fn refresh_storage_ptr(&mut self) {
        self.storage_ptr = self.out_of_line_storage.as_ptr();
    }

    /// Reset the out-of-line VALUE mirror to empty before a cell is (re)seeded.
    ///
    /// C++ JSC: a cell born with initial own properties reaches its shape by applying
    /// addPropertyTransition per property; the Butterfly is sized/filled in lockstep.
    /// gc-r4 Batch 2: the offsets come from the store's structure_table now, so the
    /// store seeds the structure and then fills this mirror (see
    /// `CoreObjectStore::fill_initial_property_storage`). This only clears the mirror.
    pub(crate) fn reset_property_storage_mirror(&mut self) {
        self.out_of_line_storage.clear();
        // clear() can leave a non-coherent storage_ptr; refresh so a cell with no data
        // properties still publishes a coherent Butterfly-pointer analog.
        self.refresh_storage_ptr();
        self.deleted_offsets.clear();
    }

    /// Write a data value into the out-of-line storage mirror at the
    /// structure-assigned `offset` (the offset is sourced by the caller from the
    /// store's structure_table — the per-shape Structure::PropertyTable).
    ///
    /// C++ JSC JSObject::putDirectOffset / locationForOffset (JSObject.h:711): store
    /// the value at offsetInRespectiveStorage(offset). The HashMap `properties`
    /// remains the authoritative VALUE store this batch; this mirror is what the
    /// offset read path (and batch-3 machine code) consumes. Grows the Vec with
    /// undefined fill so the slot exists, mirroring Butterfly growth on out-of-line
    /// property addition. No-op for an invalid offset.
    pub(crate) fn write_data_property_offset_slot(
        &mut self,
        offset: PropertyOffset,
        value: RuntimeValue,
    ) {
        if offset.raw() < 0 {
            return;
        }
        let index = offset_storage_index(offset);
        if index >= self.out_of_line_storage.len() {
            self.out_of_line_storage
                .resize(index + 1, RuntimeValue::undefined());
            // resize can reallocate (move) the buffer, so the cached Butterfly-pointer
            // analog must be refreshed; this is the centralized Vec-growth path that
            // every out-of-line property add routes through.
            self.refresh_storage_ptr();
        }
        self.out_of_line_storage[index] = value;
    }

    /// Clear the out-of-line storage slot for a freed offset (property deletion or
    /// data->accessor conversion). C++ JSC: the freed PropertyOffset is recorded in
    /// PropertyTable::m_deletedOffsets for reuse (handled in the structure_table) and
    /// the storage slot is cleared. Here we clear the value mirror so a recycled
    /// offset starts from undefined; no-op for an invalid offset.
    pub(crate) fn clear_data_property_offset_slot(&mut self, offset: PropertyOffset) {
        if offset.raw() < 0 {
            return;
        }
        let index = offset_storage_index(offset);
        if let Some(slot) = self.out_of_line_storage.get_mut(index) {
            *slot = RuntimeValue::undefined();
        }
        // In-place slot clear cannot realloc the Vec, so storage_ptr is already
        // coherent; refresh defensively (single rule: any Vec touch -> refresh).
        self.refresh_storage_ptr();
        self.deleted_offsets.push(offset);
    }

    /// Read the out-of-line storage mirror at `expected_offset`.
    ///
    /// C++ JSC JSObject::getDirect(offset) / locationForOffset: read the value at
    /// offsetInRespectiveStorage(offset). The structure guard upstream proves the
    /// offset is valid for this cell's shape; this is the offset-indexed load
    /// batch-3 will emit as `mov reg <- [storage_base + idx*8]`.
    pub(crate) fn read_data_property_offset_slot(
        &self,
        expected_offset: PropertyOffset,
    ) -> Option<RuntimeValue> {
        if expected_offset.raw() < 0 {
            return None;
        }
        self.out_of_line_storage
            .get(offset_storage_index(expected_offset))
            .copied()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CorePropertyPut {
    Stored,
    Setter(RuntimeValue),
    IgnoredGetterOnly,
    IgnoredReadOnly,
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
/// ADDITIVE this batch: nothing calls these yet (dead_code).
#[allow(dead_code)]
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
    pub(crate) fn clone_butterfly(&mut self, handle: ButterflyHandle) -> ButterflyHandle {
        let copy = self.butterflies[handle.0].clone();
        let index = self.butterflies.len();
        self.butterflies.push(copy);
        ButterflyHandle(index)
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
        let mut properties = HashMap::new();
        let mut property_order = Vec::new();
        let mut instance_prototype = None;
        if let Some(key) = prototype_property_key {
            let prototype = self.allocate();
            instance_prototype = Some(prototype);
            property_order.push(key.clone());
            properties.insert(
                key,
                CoreProperty {
                    kind: CorePropertyKind::Data(prototype),
                    attributes: CorePropertyAttributes {
                        writable: true,
                        enumerable: false,
                        configurable: false,
                    },
                },
            );
        }
        let function = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Function,
            prototype: Some(function_prototype),
            function_index: Some(function_index),
            captures,
            properties,
            property_order,
            construct_ability,
            ..CoreObjectCell::default()
        });
        if let Some(prototype) = instance_prototype {
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
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Map,
            prototype: Some(prototype),
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_regexp(
        &mut self,
        source: String,
        flags: RegexFlags,
        flags_text: String,
    ) -> RuntimeValue {
        let prototype = self.ensure_regexp_prototype();
        let object = self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::RegExp,
            prototype: Some(prototype),
            regexp_source: source,
            regexp_flags: flags,
            regexp_flags_text: flags_text,
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
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::ArrayBuffer,
            prototype: Some(prototype),
            array_buffer_data: vec![0; byte_length],
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
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::Set,
            prototype: Some(prototype),
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_weak_map(&mut self) -> RuntimeValue {
        let prototype = self.ensure_weak_map_prototype();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::WeakMap,
            prototype: Some(prototype),
            ..CoreObjectCell::default()
        })
    }

    pub(crate) fn allocate_weak_set(&mut self) -> RuntimeValue {
        let prototype = self.ensure_weak_set_prototype();
        self.allocate_cell(CoreObjectCell {
            kind: CoreObjectKind::WeakSet,
            prototype: Some(prototype),
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
        // Reset the out-of-line VALUE mirror; the structure (and its offsets) is seeded
        // below and the mirror is then filled from the structure's PropertyTable.
        cell.reset_property_storage_mirror();
        if cell.structure_id == StructureId::INVALID {
            // C++ JSC: a fresh object adopts the shared empty Structure for its
            // class+prototype instead of a private one, so same-shape siblings can
            // converge under one property-transition graph. seed_structure_id
            // reconstructs that shared root (see its comment); the prior behavior
            // here minted a private id per object, defeating cross-instance ICs.
            //
            // FIX 3: some cells are born with initial own properties already
            // installed (e.g. allocate_function_with_construct_ability builds the
            // `.prototype` own-property BEFORE allocate_cell). C++ JSC reaches such
            // a non-empty shape by applying addPropertyTransition once per initial
            // property from the empty (class, prototype) Structure, so the resulting
            // Structure reflects the real shape and same-shape siblings converge.
            // Taking the empty seed root would instead make a 0-property object and
            // a 1-property function share a structure id (different shapes, same id),
            // corrupting cross-instance ICs. seed_initial_shape_structure_id mirrors
            // C++ by chaining add_property_transition from the empty root over the
            // recorded initial-property order; for a 0-property cell it degenerates to
            // the plain empty seed root.
            cell.structure_id = self.seed_initial_shape_structure_id(&cell);
        }
        // Fill the out-of-line VALUE mirror at each initial data property's
        // structure-assigned offset (the putDirectOffset analog), now that the
        // structure (offset authority) is known.
        self.fill_initial_property_storage(&mut cell);
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

    /// The property offset assigned to `key` by structure `sid` — read straight from
    /// the structure's Structure::PropertyTable (owned, or materialized-on-miss by
    /// replaying the transition chain, Structure.cpp:456). The SINGLE offset authority
    /// (replacing the deleted per-cell `property_offsets`). Returns `None` for a key
    /// with no named offset in this shape (symbol/index, never added, deleted, or an
    /// accessor — those keys are taken out of the table).
    pub(crate) fn structure_offset(
        &self,
        sid: StructureId,
        key: &CorePropertyKey,
    ) -> Option<PropertyOffset> {
        if !self.is_live_structure(sid) {
            return None;
        }
        let uid = self.lookup_property_uid(key)?;
        let raw = match self.structure_table.structure(sid).property_table_or_null() {
            Some(table) => table.get(uid).0,
            // materialize-on-miss: the table was moved to a child via a transition;
            // rebuild it by replaying the chain (cache-back deferred, gc-r4 B2).
            None => {
                self.structure_table
                    .materialize_property_table(sid)
                    .get(uid)
                    .0
            }
        };
        if raw < 0 {
            None
        } else {
            Some(PropertyOffset::new(raw))
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
    /// guarantee. Symbol/index keys cannot key the transition table, so they take the
    /// conservative fresh-dictionary fallback with an invalid offset (the value lives
    /// only in the authoritative `properties` map; never a wrong slot).
    pub(crate) fn structure_add_property(
        &mut self,
        old: StructureId,
        key: &CorePropertyKey,
        attributes: CorePropertyAttributes,
    ) -> (StructureId, PropertyOffset) {
        if !core_property_key_supports_named_property_offset(key) {
            return (
                self.fresh_dictionary_structure(old, None),
                PropertyOffset::INVALID,
            );
        }
        let uid = self.intern_property_uid(key);
        let attributes_u32 = core_attributes_to_u32(attributes);
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

    /// Fill the out-of-line VALUE mirror at each initial data property's
    /// structure-assigned offset, after the cell's structure has been seeded.
    fn fill_initial_property_storage(&self, cell: &mut CoreObjectCell) {
        let sid = cell.structure_id;
        for key in cell.property_order.clone() {
            let Some(CorePropertyKind::Data(value)) =
                cell.properties.get(&key).map(|property| property.kind)
            else {
                continue;
            };
            if let Some(offset) = self.structure_offset(sid, &key) {
                cell.write_data_property_offset_slot(offset, value);
            }
        }
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

    /// Structure id for a cell that may be born with initial own properties.
    ///
    /// C++ JSC: an object with N initial own properties has the Structure reached by
    /// applying addPropertyTransition N times from the empty (class, prototype)
    /// Structure (Structure.cpp:561), so its Structure encodes the full shape. The Rust
    /// interpreter installs some initial properties before allocate_cell (e.g. a
    /// function's `.prototype`), so we mirror that by seeding the empty root and then
    /// chaining structure_add_property over the recorded initial-property order. The
    /// offsets COME FROM the transition (Structure::PropertyTable), so same-shape
    /// siblings converge on one structure id AND one offset set. For a 0-property cell
    /// this degenerates to the plain empty seed root.
    ///
    /// Symbol/indexed initial keys: structure_add_property folds them onto a fresh
    /// per-object dictionary (preserving the named offsets, never a wrong slot); their
    /// value lives only in the authoritative `properties` map. Fidelity gap deferred
    /// (no Octane consumer builds Symbol-keyed initial shapes on the hot path).
    pub(crate) fn seed_initial_shape_structure_id(&mut self, cell: &CoreObjectCell) -> StructureId {
        let mut structure_id = self.seed_structure_id(cell.kind, cell.prototype);
        for key in cell.property_order.clone() {
            let Some(property) = cell.properties.get(&key).copied() else {
                continue;
            };
            if !matches!(property.kind, CorePropertyKind::Data(_)) {
                // Accessors do not occupy a named-data offset; skip without disturbing
                // the shared shape chain.
                continue;
            }
            let (next_structure, _offset) =
                self.structure_add_property(structure_id, &key, property.attributes);
            structure_id = next_structure;
        }
        structure_id
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
            if cell.properties.contains_key(key) {
                return Ok(true);
            }
            if cell.kind == CoreObjectKind::Array {
                if key.is_string("length") {
                    return Ok(true);
                }
                if let Some(index) = key_array_index(key) {
                    if cell
                        .elements
                        .get(index)
                        .is_some_and(|element| element.is_some())
                    {
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
            if let Some(property) = cell.properties.get(key).copied() {
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
                        cell.elements
                            .get(index)
                            .is_some_and(|element| element.is_some())
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
        let own_property = cell.properties.get(key);
        let has_own_property = own_property.is_some();
        let has_own_data_property =
            own_property.is_some_and(|property| matches!(property.kind, CorePropertyKind::Data(_)));
        let indexed_key = key_array_index(key);
        let is_dense_array_indexed_store =
            matches!(cell.kind, CoreObjectKind::Array) && indexed_key.is_some();
        let has_own_indexed_element = indexed_key.is_some_and(|index| {
            matches!(cell.kind, CoreObjectKind::Array)
                && cell
                    .elements
                    .get(index)
                    .is_some_and(|element| element.is_some())
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

    pub(crate) fn put(
        &mut self,
        heap: &mut Heap,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<CorePropertyPut, ExecutionError> {
        let Some(receiver) = self.find(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if let Some(property) = receiver.properties.get(key).copied() {
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
            key_array_index(key).is_some_and(|index| {
                receiver
                    .elements
                    .get(index)
                    .is_some_and(|element| element.is_some())
            })
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
            if let Some(property) = cell.properties.get(key).copied() {
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
            if let Some(property) = cell.properties.get(key).copied() {
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
                    object.elements.len().try_into().unwrap_or(i32::MAX),
                )),
                attributes: CorePropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: false,
                },
            }));
        }
        if let Some(property) = object.properties.get(key).copied() {
            return Ok(Some(property));
        }
        if object.kind == CoreObjectKind::Array {
            if let Some(index) = key_array_index(key) {
                if let Some(Some(value)) = object.elements.get(index).copied() {
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
        if object.kind == CoreObjectKind::Array {
            for (index, value) in object.elements.iter().enumerate() {
                if value.is_some() {
                    index_names.insert(index);
                }
            }
        } else if object.kind == CoreObjectKind::Uint8Array {
            for index in 0..object.view_length {
                index_names.insert(index);
            }
        }

        let mut string_names = Vec::new();
        let mut hidden_index_names = BTreeSet::new();
        for key in &object.property_order {
            let Some(name) = key_string_name(key) else {
                continue;
            };
            let Some(property) = object.properties.get(key) else {
                continue;
            };
            if let Some(index) = parse_array_index_name(name) {
                if property.attributes.enumerable {
                    index_names.insert(index);
                    hidden_index_names.remove(&index);
                } else {
                    index_names.remove(&index);
                    hidden_index_names.insert(index);
                }
            } else {
                string_names.push((name.to_owned(), property.attributes.enumerable));
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
        if object.kind == CoreObjectKind::Array {
            for (index, value) in object.elements.iter().enumerate() {
                if value.is_some() {
                    let key = CorePropertyKey::String(index.to_string());
                    seen.insert(key.clone());
                    keys.push(key);
                }
            }
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
        for key in &object.property_order {
            if object.properties.contains_key(key) && seen.insert(key.clone()) {
                keys.push(key.clone());
            }
        }
        Ok(keys)
    }

    pub(crate) fn set_data_own(
        &mut self,
        object: RuntimeValue,
        key: &CorePropertyKey,
        value: RuntimeValue,
    ) -> Result<(), ExecutionError> {
        // C++ JSC: a pure property addition (or an accessor->data kind change, which
        // adds a fresh named-data offset) routes through Structure::addPropertyTransition
        // so the offset comes from the per-shape Structure::PropertyTable and same-shape
        // siblings share one successor structure + offset. A same-shape value replace
        // keeps the structure and rewrites the existing offset slot.
        let (old_structure, shape_changed) = {
            let Some(object_cell) = self.find_mut(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let old_structure = object_cell.structure_id;
            let shape_changed = if let Some(property) = object_cell.properties.get_mut(key) {
                let was_data = matches!(property.kind, CorePropertyKind::Data(_));
                property.kind = CorePropertyKind::Data(value);
                !was_data // accessor -> data is a (offset-adding) shape change
            } else {
                object_cell.property_order.push(key.clone());
                object_cell.properties.insert(
                    key.clone(),
                    CoreProperty {
                        kind: CorePropertyKind::Data(value),
                        attributes: CorePropertyAttributes::DATA_DEFAULT,
                    },
                );
                true
            };
            (old_structure, shape_changed)
        };
        // Offset authority = structure_table. A shape change adds a new named offset via
        // the transition; a same-shape replace reuses the existing offset.
        let (new_structure, offset) = if shape_changed {
            self.structure_add_property(old_structure, key, CorePropertyAttributes::DATA_DEFAULT)
        } else {
            (
                old_structure,
                self.structure_offset(old_structure, key)
                    .unwrap_or(PropertyOffset::INVALID),
            )
        };
        if let Some(object_cell) = self.find_mut(object) {
            // putDirectOffset analog: write the value at the structure-assigned offset.
            object_cell.write_data_property_offset_slot(offset, value);
            if shape_changed {
                object_cell.structure_id = new_structure;
            }
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
        // C++ JSC: a property ADDITION (or accessor->data conversion, which adds a fresh
        // named-data offset) routes through Structure::addPropertyTransition so the
        // offset comes from the per-shape PropertyTable and siblings converge. An
        // attribute change on an existing data property keeps that property's offset but
        // is not a shareable transition, so it takes a fresh per-object dictionary
        // (preserving the surviving offsets). A same-shape value replace keeps both.
        let (old_structure, shape_changed, is_addition, was_accessor) = {
            let Some(object_cell) = self.find_mut(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let old_structure = object_cell.structure_id;
            let current = object_cell.properties.get(key).copied();
            let is_addition = current.is_none();
            let was_accessor = current
                .is_some_and(|current| matches!(current.kind, CorePropertyKind::Accessor { .. }));
            let shape_changed = match current {
                Some(current) => {
                    !matches!(current.kind, CorePropertyKind::Data(_))
                        || current.attributes != CorePropertyAttributes::DATA_DEFAULT
                }
                None => {
                    object_cell.property_order.push(key.clone());
                    true
                }
            };
            object_cell.properties.insert(
                key.clone(),
                CoreProperty {
                    kind: CorePropertyKind::Data(value),
                    attributes: CorePropertyAttributes::DATA_DEFAULT,
                },
            );
            (old_structure, shape_changed, is_addition, was_accessor)
        };
        let (new_structure, offset) = if !shape_changed {
            // Value-only replace: same structure, existing offset.
            (
                old_structure,
                self.structure_offset(old_structure, key)
                    .unwrap_or(PropertyOffset::INVALID),
            )
        } else if is_addition || was_accessor {
            // New named-data offset via a (shareable) transition.
            self.structure_add_property(old_structure, key, CorePropertyAttributes::DATA_DEFAULT)
        } else {
            // Attribute change on an existing data property: offset preserved, fresh
            // per-object dictionary structure.
            let offset = self
                .structure_offset(old_structure, key)
                .unwrap_or(PropertyOffset::INVALID);
            (self.fresh_dictionary_structure(old_structure, None), offset)
        };
        if let Some(object_cell) = self.find_mut(object) {
            // putDirectOffset analog.
            object_cell.write_data_property_offset_slot(offset, value);
            if shape_changed {
                object_cell.structure_id = new_structure;
            }
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
        // C++ JSC: defining a brand-new property is a property-addition transition
        // keyed by (uid, attributes) (StructureTransitionTable), so siblings defined
        // with the same key+attributes share a structure id. Redefining an existing
        // property (kind or attribute change) is out of scope and keeps a fresh id.
        let (old_structure, shape_changed, is_addition, was_accessor) = {
            let Some(object_cell) = self.find_mut(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let old_structure = object_cell.structure_id;
            let current = object_cell.properties.get(key).copied();
            let is_addition = current.is_none();
            if let Some(current) = current {
                if !current.attributes.configurable {
                    if attributes.configurable
                        || attributes.enumerable != current.attributes.enumerable
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
            let was_accessor = current
                .is_some_and(|current| matches!(current.kind, CorePropertyKind::Accessor { .. }));
            let shape_changed = match current {
                Some(current) => {
                    !matches!(current.kind, CorePropertyKind::Data(_))
                        || current.attributes != attributes
                }
                None => {
                    object_cell.property_order.push(key.clone());
                    true
                }
            };
            object_cell.properties.insert(
                key.clone(),
                CoreProperty {
                    kind: CorePropertyKind::Data(value),
                    attributes,
                },
            );
            (old_structure, shape_changed, is_addition, was_accessor)
        };
        // Offset authority = structure_table. A brand-new property (or accessor->data)
        // gets a fresh offset via the (uid, attributes)-keyed transition; an attribute
        // change on an existing data property keeps its offset under a fresh dictionary.
        let (new_structure, offset) = if !shape_changed {
            (
                old_structure,
                self.structure_offset(old_structure, key)
                    .unwrap_or(PropertyOffset::INVALID),
            )
        } else if is_addition || was_accessor {
            self.structure_add_property(old_structure, key, attributes)
        } else {
            let offset = self
                .structure_offset(old_structure, key)
                .unwrap_or(PropertyOffset::INVALID);
            (self.fresh_dictionary_structure(old_structure, None), offset)
        };
        if let Some(object_cell) = self.find_mut(object) {
            // putDirectOffset analog.
            object_cell.write_data_property_offset_slot(offset, value);
            if shape_changed {
                object_cell.structure_id = new_structure;
            }
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
        let (old_structure, removed) = {
            let Some(object_cell) = self.find_mut(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let old_structure = object_cell.structure_id;
            if object_cell
                .properties
                .get(key)
                .is_some_and(|property| !property.attributes.configurable)
            {
                return Ok(false);
            }
            if object_cell.kind == CoreObjectKind::Uint8Array {
                if let Some(index) = key_array_index(key) {
                    if index < object_cell.view_length {
                        return Ok(false);
                    }
                }
            }
            if object_cell.kind == CoreObjectKind::Array {
                if let Some(index) = key_array_index(key) {
                    if let Some(slot) = object_cell.elements.get_mut(index) {
                        *slot = None;
                    }
                }
            }
            let removed = if object_cell.properties.remove(key).is_some() {
                object_cell
                    .property_order
                    .retain(|ordered_key| ordered_key != key);
                true
            } else {
                false
            };
            (old_structure, removed)
        };
        if removed {
            // Free the deleted property's storage slot (its offset is read from the OLD
            // structure before the dictionary takes it out, then recycled by the table).
            let removed_offset = self.structure_offset(old_structure, key);
            let new_structure = self.fresh_dictionary_structure(old_structure, Some(key));
            if let Some(object_cell) = self.find_mut(object) {
                if let Some(offset) = removed_offset {
                    object_cell.clear_data_property_offset_slot(offset);
                }
                object_cell.structure_id = new_structure;
            }
            self.finish_structure_transition(old_structure);
        }
        Ok(true)
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
        let (old_structure, shape_changed) = {
            let Some(object_cell) = self.find_mut(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let old_structure = object_cell.structure_id;
            let current = object_cell.properties.get(key).copied();
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
            let shape_changed = match current {
                Some(current) => current != property,
                None => {
                    object_cell.property_order.push(key.clone());
                    true
                }
            };
            object_cell.properties.insert(key.clone(), property);
            (old_structure, shape_changed)
        };
        if shape_changed {
            // Installing an accessor takes any displaced data offset out of the shape
            // (the key no longer has a named-data slot) and frees it; surviving offsets
            // are preserved on the fresh per-object dictionary structure.
            let displaced_offset = self.structure_offset(old_structure, key);
            let new_structure = self.fresh_dictionary_structure(old_structure, Some(key));
            if let Some(object_cell) = self.find_mut(object) {
                if let Some(offset) = displaced_offset {
                    object_cell.clear_data_property_offset_slot(offset);
                }
                object_cell.structure_id = new_structure;
            }
            self.finish_structure_transition(old_structure);
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
        let (old_structure, shape_changed) = {
            let Some(object_cell) = self.find_mut(object) else {
                return Err(ExecutionError::ExpectedObject);
            };
            let old_structure = object_cell.structure_id;
            let current = object_cell.properties.get(key).copied();
            if let Some(current) = current {
                if !current.attributes.configurable {
                    if attributes.configurable
                        || attributes.enumerable != current.attributes.enumerable
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
                None => {
                    object_cell.property_order.push(key.clone());
                    true
                }
            };
            object_cell.properties.insert(key.clone(), property);
            (old_structure, shape_changed)
        };
        if shape_changed {
            let displaced_offset = self.structure_offset(old_structure, key);
            let new_structure = self.fresh_dictionary_structure(old_structure, Some(key));
            if let Some(object_cell) = self.find_mut(object) {
                if let Some(offset) = displaced_offset {
                    object_cell.clear_data_property_offset_slot(offset);
                }
                object_cell.structure_id = new_structure;
            }
            self.finish_structure_transition(old_structure);
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
            Some(cell) => (
                cell.kind,
                cell.properties.is_empty() && cell.property_order.is_empty(),
                cell.structure_id,
            ),
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
        let Some(constructor_cell) = self.find_mut(constructor) else {
            return Err(ExecutionError::ExpectedFunction);
        };
        if constructor_cell.kind != CoreObjectKind::Function {
            return Err(ExecutionError::ExpectedFunction);
        }
        constructor_cell
            .instance_fields
            .push(CoreInstanceField { key, initializer });
        Ok(())
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
        Ok(constructor_cell.instance_fields.clone())
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
        Ok(buffer.array_buffer_data.len())
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
            let start = start.min(buffer.array_buffer_data.len());
            let end = end.min(buffer.array_buffer_data.len()).max(start);
            buffer.array_buffer_data[start..end].to_vec()
        };
        let result = self.allocate_array_buffer(bytes.len());
        let Some(result_buffer) = self.find_mut(result) else {
            return Err(ExecutionError::ExpectedObject);
        };
        result_buffer.array_buffer_data = bytes;
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
        let start = byte_offset.saturating_add(index.saturating_mul(element_size));
        let Some(bytes) = buffer
            .array_buffer_data
            .get(start..start.saturating_add(element_size))
        else {
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
        let Some(buffer) = self.find_mut(buffer) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let start = byte_offset.saturating_add(index.saturating_mul(element_size));
        let Some(slot) = buffer
            .array_buffer_data
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
        buffer
            .array_buffer_data
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
        let Some(buffer) = self.find_mut(buffer) else {
            return Err(ExecutionError::ExpectedObject);
        };
        let Some(slot) = buffer
            .array_buffer_data
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
        let Some(object) = self.find_mut(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.elements.len() <= index {
            object.elements.resize(index.saturating_add(1), None);
        }
        object.elements[index] = Some(value);
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
        let Some(object) = self.find_mut(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        object.elements.push(Some(value));
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
        let Some(object) = self.find_mut(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        if let Some(slot) = object.elements.get_mut(index) {
            *slot = None;
        }
        Ok(true)
    }

    pub(crate) fn pop_array_element(
        &mut self,
        object: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(object) = self.find_mut(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        Ok(object
            .elements
            .pop()
            .flatten()
            .unwrap_or_else(RuntimeValue::undefined))
    }

    pub(crate) fn resize_array_elements(
        &mut self,
        object: RuntimeValue,
        length: usize,
    ) -> Result<(), ExecutionError> {
        let Some(object) = self.find_mut(object) else {
            return Err(ExecutionError::ExpectedObject);
        };
        if object.kind != CoreObjectKind::Array {
            return Err(ExecutionError::ExpectedObject);
        }
        object.elements.resize(length, None);
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
                object.elements.len().try_into().unwrap_or(i32::MAX),
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
        object
            .elements
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
                    captures: object.captures.clone(),
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
            | CoreObjectKind::DataView => Err(ExecutionError::ExpectedFunction),
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
            | CoreObjectKind::DataView => Err(ExecutionError::ExpectedFunction),
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
        object
            .captures
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
            | CoreObjectKind::DataView => Err(ExecutionError::ExpectedFunction),
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
        Some((target, object.bound_this, object.bound_args.clone()))
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
        Ok((
            object.regexp_source.clone(),
            object.regexp_flags,
            object.regexp_flags_text.clone(),
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
        Ok(std::mem::take(&mut object.promise_reactions))
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
        object.promise_reactions.push(reaction);
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
        let prototype = match self
            .find(constructor)?
            .properties
            .get(prototype_property_key)
            .copied()?
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
            if let Some(property) = cell.properties.get(key).copied() {
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
            if cell.kind == CoreObjectKind::Array {
                if let Some(index) = key_array_index(key) {
                    if let Some(Some(value)) = cell.elements.get(index).copied() {
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
            if let Some(property) = cell.properties.get(key).copied() {
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
            if cell.kind == CoreObjectKind::Array {
                if let Some(index) = key_array_index(key) {
                    if let Some(Some(value)) = cell.elements.get(index).copied() {
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
            if let Some(Some(value)) = cell.elements.get(index).copied() {
                return Ok(value);
            }
            if let Some(property) = cell.properties.get(&key) {
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
            if let Some(Some(value)) = cell.elements.get(index).copied() {
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
            if let Some(property) = cell.properties.get(&key) {
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
        // a live own-data slot (invariant a/b). Read it directly from the
        // out-of-line storage mirror with NO key comparison or HashMap scan.
        cell.read_data_property_offset_slot(cached_offset)
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
            // from slow-path semantics. One HashMap probe on the already-built key,
            // no allocation.
            match cell.properties.get(cached_key) {
                Some(property)
                    if property.attributes.writable
                        && matches!(property.kind, CorePropertyKind::Data(_)) => {}
                _ => return Ok(false),
            }
        }
        // GC write barrier, identical to the slow store's
        // set_data_own_with_write_barrier -> apply_value_store_write_barrier. MUST
        // run on the fast path too: storing a heap value into an object field is a
        // barriered mutator field write regardless of whether an IC served it.
        self.apply_value_store_write_barrier(heap, receiver, value)?;
        let Some(cell) = self.objects.get_mut(index) else {
            return Ok(false);
        };
        let cell = cell.as_mut().get_mut();
        // Re-validate after the barrier (the barrier path does not mutate this
        // cell's shape, but the re-fetch keeps the store self-contained).
        if cell.structure_id != cached_structure_id {
            return Ok(false);
        }
        let Some(property) = cell.properties.get_mut(cached_key) else {
            return Ok(false);
        };
        let CorePropertyKind::Data(slot) = &mut property.kind else {
            return Ok(false);
        };
        *slot = value;
        // Lockstep mirror update (invariant c): write the same value into
        // out_of_line_storage at the cached offset. The slot already exists (the
        // structure match proves the shape), so this never grows the Vec and the
        // cached storage_ptr stays coherent.
        let storage_index = offset_storage_index(cached_offset);
        if let Some(mirror) = cell.out_of_line_storage.get_mut(storage_index) {
            *mirror = value;
        }
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
