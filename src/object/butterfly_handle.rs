//! The LIVE Butterfly representation, over `RuntimeValue`.
//!
//! C++ target: `runtime/Butterfly.h` (the `Butterfly` class, :134-150). A
//! JSObject's variable-size out-of-line state lives behind a single butterfly
//! pointer (`JSObject::m_butterfly`, JSObject.h:1167): named-property storage
//! grows LEFT of the base (negative offsets), indexed elements grow RIGHT
//! (positive offsets), with the `IndexingHeader` sitting between them
//! (Butterfly.h:140-150, `totalSize`/`fromBase`). `ButterflyAllocation` below is
//! the Rust home of that out-of-line region as two `RuntimeValue` vectors.
//!
//! BOUNDARY (B1a, gc-r4): the LIVE butterfly rep is THIS module, over
//! `RuntimeValue` (value/repr.rs, the NaN-boxed `JsValue` the interpreter runs
//! on). `object/storage.rs`'s `Butterfly`/`OutOfLineStorage`/`IndexedStorage`/
//! `InlineStorage`/`ButterflyLayout` are NON-LIVE contract/skeleton types over
//! the separate `JsValue` representation used by the unwired `object/identity.rs`
//! `JsObject` skeleton and the `runtime/property.rs`/`runtime/array.rs` contract
//! surfaces; they are retired in a later GAP-D value-type reconciliation cleanup
//! (see docs/design/gc-r4.md, GAP D). Only `ButterflyHandle` is shared, and it
//! is moved HERE because it is a value-type-agnostic slab index.

// C++ JSC butterflies hold `WriteBarrier<Unknown>` / `EncodedJSValue` slots; the
// live interpreter value is `JsValue` (re-exported elsewhere as `RuntimeValue`),
// so the LIVE butterfly stores `RuntimeValue` directly. We alias to keep the
// gc-r4 cutover vocabulary ("the butterfly is the value authority over
// RuntimeValue").
use crate::value::JsValue as RuntimeValue;

/// Handle to one object's auxiliary butterfly storage in the store-owned slab.
///
/// C++ JSC: a JSObject reaches its butterfly through the raw `m_butterfly`
/// pointer (JSObject.h:1167) into a GC Auxiliary allocation. Until R4 makes the
/// cell a raw arena address, the Rust analog is an index into a store-owned slab
/// (`CoreObjectStore::butterflies`); the handle is value-type-agnostic (a plain
/// index), which is why it lives here rather than beside either value rep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ButterflyHandle(pub usize);

impl ButterflyHandle {
    /// Sentinel "no butterfly assigned yet" handle.
    ///
    /// C++ JSC: a freshly constructed JSObject's `m_butterfly` is null until the
    /// allocator hands it an Auxiliary butterfly. The Rust analog: a cell built via
    /// `CoreObjectCell::default()` carries this sentinel until `allocate_cell`
    /// assigns a real slab handle via `allocate_butterfly()` at the single object
    /// allocation chokepoint. Never indexes the slab (allocate_cell overwrites it
    /// before the cell is published).
    pub const INVALID: Self = ButterflyHandle(usize::MAX);
}

/// One object's out-of-line butterfly region, over `RuntimeValue`.
///
/// C++ JSC `Butterfly` (Butterfly.h:134-150): a single allocation with named
/// property storage to the LEFT of the base and indexed element storage to the
/// RIGHT, IndexingHeader between. The Rust live rep splits that one allocation
/// into two vectors so each side grows independently and safely:
///   - `props`    — the property storage side (Butterfly.h `propertyStorage()`,
///     :183), a forward-indexed `[RuntimeValue]` mirroring the existing
///     `out_of_line_storage` mirror (object_store.rs); the inline band and the
///     out-of-line band are both packed forward here (the same packing
///     `offset_storage_index` already uses), so the property->slot mapping stays
///     identical across the cutover.
///   - `elements` — the indexed payload side (Butterfly.h `contiguous()`, :196),
///     `Vec<Option<RuntimeValue>>` where `None` is a hole, mirroring the existing
///     `elements` field.
///
/// DIVERGENCE (vs Butterfly.h): C++ keeps both sides in ONE allocation reached by
/// signed pointer arithmetic around a shared base; the Rust live rep uses two
/// `Vec`s because a single allocation with negative interior indices cannot be
/// expressed in safe Rust without exactly the self-referential interior pointer
/// (`storage_ptr`) that gc-r4 is removing. The packing of the property side is
/// unchanged, so this is representation-only, not a semantic divergence.
#[derive(Clone, Debug, Default)]
pub struct ButterflyAllocation {
    /// Property storage side (grows LEFT in C++; forward-packed here).
    pub props: Vec<RuntimeValue>,
    /// Indexed element storage side (grows RIGHT in C++); `None` == hole.
    pub elements: Vec<Option<RuntimeValue>>,
}

#[allow(dead_code)]
impl ButterflyAllocation {
    /// Read the property slot at the already-mapped storage `index`.
    ///
    /// C++ JSC `JSObject::getDirect(offset)` via `locationForOffset`
    /// (JSObject.h:711): load the value at the offset's slot. The offset->slot
    /// band mapping is applied by the store-level wrapper; this side takes the
    /// raw slot index. Out-of-range yields `None` (no such slot for this shape).
    pub fn prop_get(&self, index: usize) -> Option<RuntimeValue> {
        self.props.get(index).copied()
    }

    /// Write a value into the property slot at the already-mapped storage
    /// `index`, growing the property side with `undefined` fill so the slot
    /// exists.
    ///
    /// C++ JSC `JSObject::putDirectOffset` (JSObject.h:711): store the value at
    /// `offsetInRespectiveStorage(offset)`. Mirrors the existing
    /// `write_data_property_offset_slot` grow logic (object_store.rs): grow the
    /// Vec to `index + 1` with `RuntimeValue::undefined()` fill, then store —
    /// the analog of Butterfly property-storage growth on out-of-line property
    /// addition.
    pub fn prop_put(&mut self, index: usize, value: RuntimeValue) {
        if index >= self.props.len() {
            self.props.resize(index + 1, RuntimeValue::undefined());
        }
        self.props[index] = value;
    }

    /// Clear the property slot at the already-mapped storage `index` back to
    /// `undefined` (property deletion or data->accessor conversion).
    ///
    /// C++ JSC: the freed PropertyOffset is recorded for reuse in the
    /// `Structure::PropertyTable` (handled at the store/structure layer) and the
    /// storage slot is cleared. In-bounds only (a recycled offset re-reads as
    /// `undefined`); no-op otherwise.
    pub fn prop_clear(&mut self, index: usize) {
        if let Some(slot) = self.props.get_mut(index) {
            *slot = RuntimeValue::undefined();
        }
    }

    /// Number of occupied property slots (Butterfly property-storage length).
    pub fn prop_len(&self) -> usize {
        self.props.len()
    }

    /// Read the indexed element at `index`; `None` for a hole or out-of-range.
    ///
    /// C++ JSC `Butterfly::contiguous()` indexed load (Butterfly.h:196). Flattens
    /// hole (inner `None`) and out-of-range (outer `None`) to `None` — the common
    /// "present element value or nothing" read.
    pub fn elem_get(&self, index: usize) -> Option<RuntimeValue> {
        self.elements.get(index).copied().flatten()
    }

    /// Write a value into the indexed element at `index`, growing the element
    /// side with hole (`None`) fill so the slot exists.
    ///
    /// C++ JSC `Butterfly::contiguous()` indexed store (Butterfly.h:196): the
    /// vector grows on the RIGHT and the gap between the old and new lengths is
    /// hole-filled.
    pub fn elem_put(&mut self, index: usize, value: RuntimeValue) {
        if index >= self.elements.len() {
            self.elements.resize(index + 1, None);
        }
        self.elements[index] = Some(value);
    }

    /// Resize the indexed element side to `len` slots, hole-filling growth.
    ///
    /// C++ JSC butterfly vectorLength resize (Butterfly.h:187-189,
    /// `setVectorLength`); growth introduces holes, shrink drops trailing slots.
    pub fn elem_resize(&mut self, len: usize) {
        self.elements.resize(len, None);
    }

    /// Number of indexed element slots (the Butterfly vectorLength analog,
    /// Butterfly.h:187).
    pub fn elem_len(&self) -> usize {
        self.elements.len()
    }

    /// Append a value to the indexed element side (push onto the RIGHT).
    ///
    /// C++ JSC contiguous append (the `JSArray::push` fast path appends at
    /// publicLength and grows vectorLength as needed, Butterfly.h:186-189).
    pub fn elem_push(&mut self, value: RuntimeValue) {
        self.elements.push(Some(value));
    }

    /// Clear the indexed element at `index` to a hole (`None`); no-op out of range.
    ///
    /// C++ JSC indexed `deleteProperty` punches a hole in the contiguous storage
    /// (the slot becomes empty), mirroring `delete arr[i]`. In-bounds only.
    pub fn elem_clear(&mut self, index: usize) {
        if let Some(slot) = self.elements.get_mut(index) {
            *slot = None;
        }
    }

    /// Pop the last indexed element (`Array.prototype.pop` fast path); flattens a
    /// trailing hole to `None`. C++ JSC `JSArray::pop` shrinks vectorLength by one.
    pub fn elem_pop(&mut self) -> Option<RuntimeValue> {
        self.elements.pop().flatten()
    }

    /// Borrow the indexed element side as a slice (for enumeration / length /
    /// snapshot reads). C++ JSC `Butterfly::contiguous()` span (Butterfly.h:196).
    pub fn elements_slice(&self) -> &[Option<RuntimeValue>] {
        &self.elements
    }
}
