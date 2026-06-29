//! Per-kind auxiliary backing allocations (scaffold only — NOT used in B1a).
//!
//! C++ target: the GC Auxiliary subspace. JSObject's variable-size per-kind
//! state that is NOT named-property/indexed butterfly storage lives in its own
//! auxiliary allocations or cells: an ArrayBuffer's bytes (`ArrayBufferContents`,
//! ArrayBuffer.h:126), a Map/Set's insertion-ordered entries
//! (`JSOrderedHashTable::Storage`, JSOrderedHashTable.h:164), etc. Like the
//! butterfly, these come from store-owned slabs keyed by an index handle until R4
//! makes cells raw arena addresses.
//!
//! This module is the future home for those per-kind relocations (the gc-r4
//! per-kind vertical slices: ArrayBuffer, Map/Set). B1a lands the module + the
//! STUB signatures only; the slabs and bodies arrive with each per-kind unit.

use crate::object::butterfly_handle::ButterflyHandle;

/// Handle to a store-owned auxiliary backing allocation.
///
/// C++ JSC: an Auxiliary-subspace allocation reached by a raw pointer; the Rust
/// analog (pre-R4) is an index into a per-kind store-owned slab, exactly like
/// `ButterflyHandle`. Defined here so the per-kind units share one aux-handle
/// vocabulary.
///
/// POD: `Copy` (a plain slab index), so a cell field of this type adds no `Drop` —
/// the whole point of the gc-r4 POD-ification relocations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct AuxiliaryHandle(pub usize);

impl AuxiliaryHandle {
    /// Sentinel "no auxiliary backing assigned" handle.
    ///
    /// C++ JSC: a per-kind out-of-line pointer (e.g. `JSBoundFunction::m_boundArgs`,
    /// JSBoundFunction.h:133) is null when the cell has no such payload. Unlike the
    /// butterfly (every JSObject gets one in `allocate_cell`), a per-kind aux backing
    /// is allocated ONLY by the kind that owns it — every other cell keeps this
    /// sentinel and never indexes the slab. The owning kind overwrites it with a real
    /// handle at its own allocation site (e.g. `allocate_bound_function`).
    pub const INVALID: Self = AuxiliaryHandle(usize::MAX);
}

/// Handle to one promise's store-owned reaction-record backing.
///
/// C++ JSC: a `JSPromise` reaches its pending reaction records through its
/// internal `[[PromiseFulfillReactions]]`/`[[PromiseRejectReactions]]` fields
/// (JSPromise.h:35); those reaction records are out-of-line, held off the cell.
/// The Rust analog (pre-R4) is an index into the store-owned
/// `CoreObjectStore::promise_reaction_lists` slab, exactly like `ButterflyHandle`.
/// It is POD (`Copy`) so the owning promise cell carries no `Drop` field and stays
/// sweep-eligible for R4 (gc-r4 R4 POD-ification). The records themselves
/// (`CorePromiseReaction`) are already `Copy` GC-edge bundles; a later collector
/// trace visits the slab to mark those edges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PromiseReactionsHandle(pub usize);

impl PromiseReactionsHandle {
    /// Sentinel "no reaction backing allocated yet" handle.
    ///
    /// C++ JSC: a fresh `JSPromise`'s reaction fields start empty — no out-of-line
    /// record backing exists until the first reaction is enqueued. The Rust analog:
    /// a promise cell carries this sentinel until `push_promise_reaction` lazily
    /// allocates its slab slot at first enqueue. Never indexes the slab.
    pub const INVALID: Self = PromiseReactionsHandle(usize::MAX);
}

// gc-r4 ArrayBuffer unit (LANDED): the ArrayBuffer byte-backing relocation
// (`ArrayBufferContents::m_data`, ArrayBuffer.h:126) shipped as the store-owned
// `CoreObjectStore::array_buffer_backings` slab + `allocate_array_buffer_backing` /
// `array_buffer_bytes` / `array_buffer_bytes_mut` methods (interpreter/object_store.rs),
// mirroring the landed BoundFunction/Promise/RegExp slabs which ALSO live on the store
// (not as free functions here). The former free-function SCAFFOLD stub was removed so
// there is ONE mechanism, not two. Raw bytes are not GC edges: no write barrier, and the
// R4 collector trace need not visit that slab.

/// Allocate the insertion-ordered backing for a Map/Set.
///
/// C++ JSC `JSOrderedHashTable::Storage` (JSOrderedHashTable.h:164, a
/// `JSCellButterfly` held by `m_storage`): the insertion-ordered entry table
/// backing `JSOrderedHashMap`/`Set`. The per-kind Map/Set slice relocates
/// `map_entries`/`set_values` to an aux backing for POD-ness now; the faithful
/// `JSOrderedHashTable` is a deferred correctness/perf batch (gc-r4 rank-5).
///
/// SCAFFOLD: signature only. Body lands with the Map/Set per-kind unit. Returns a
/// `ButterflyHandle` because the C++ Storage IS a `JSCellButterfly`.
#[allow(dead_code, unused_variables)]
pub fn allocate_ordered_hash_storage() -> ButterflyHandle {
    // TODO(gc-r4 Map/Set unit): allocate the ordered-hash entry backing and
    // return its handle.
    unimplemented!("ordered-hash storage lands with the Map/Set per-kind unit")
}
