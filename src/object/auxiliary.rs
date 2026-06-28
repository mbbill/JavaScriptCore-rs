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
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct AuxiliaryHandle(pub usize);

/// Allocate the byte backing for a typed-array/`ArrayBuffer` view.
///
/// C++ JSC `ArrayBufferContents` (ArrayBuffer.h:126): owns `void* m_data` of
/// `sizeInBytes`. The per-kind ArrayBuffer slice relocates the cell's
/// `array_buffer_data` here (gc-r4 rank-4 unit); no write barrier (raw bytes,
/// not GC edges).
///
/// SCAFFOLD: signature only. Body lands with the ArrayBuffer per-kind unit.
#[allow(dead_code, unused_variables)]
pub fn allocate_array_buffer_backing(byte_length: usize) -> AuxiliaryHandle {
    // TODO(gc-r4 ArrayBuffer unit): push a zeroed `Vec<u8>` of `byte_length`
    // into the store's array-buffer aux slab and return its handle.
    unimplemented!("ArrayBuffer aux backing lands with the ArrayBuffer per-kind unit")
}

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
