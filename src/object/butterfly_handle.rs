//! The LIVE Butterfly representation, over `RuntimeValue`.
//!
//! C++ target: `runtime/Butterfly.h` (the `Butterfly` class, :134-150). A
//! JSObject's variable-size out-of-line state lives behind a single butterfly
//! pointer (`JSObject::m_butterfly`, JSObject.h:1167): named-property storage
//! grows LEFT of the base (negative offsets), indexed elements grow RIGHT
//! (positive offsets), with the `IndexingHeader` sitting between them
//! (Butterfly.h:140-150, `totalSize`/`fromBase`). `ButterflyAllocation` below is
//! the Rust home of that out-of-line region as a SINGLE contiguous buffer.
//!
//! gc-r4 Batch 5 Step 2 — MACHINE-ADDRESSABLE single buffer. The two-Vec rep
//! (`props: Vec`, `elements: Vec<Option>`) is replaced by ONE stably-addressed
//! `Box<[RuntimeValue]>` laid out C++-faithfully so the cell's offset-8 raw base
//! pointer (the `m_butterfly` analog) is dereffable by emitted machine code:
//!   - named out-of-line property slots occupy the LOW part of the buffer and are
//!     addressed at NEGATIVE indices below the base (`base[-1]` = first OOL prop,
//!     offset 64), exactly C++ `outOfLineStorage()[offsetInOutOfLineStorage(off)]`
//!     (JSObject.h:711-723; `offsetInOutOfLineStorage(off) = -(off-64)-1`);
//!   - indexed element slots occupy the HIGH part at non-negative indices
//!     (`base[0]` = element 0), with an IN-BAND empty sentinel (`JSValue()` /
//!     `RuntimeValue::default()`, 0x0) for holes — NOT `Option`, so every slot is
//!     8 bytes and the buffer is one machine-strided array.
//! `base` (the `m_butterfly`/`fromBase` position) is `&buf[named_len]`. The buffer
//! is store-owned (`CoreObjectStore::butterflies`); the cell holds only the raw
//! exposed BASE ADDRESS (POD usize, `CoreObjectCell::butterfly_base` @8), kept
//! current under the growth barrier (createOrGrowPropertyStorage -> setButterfly).
//!
//! PROVENANCE DISCIPLINE (the miri Tree-Borrows gate): the cell's raw cell+8 base
//! pointer and this module BOTH address the SAME buffer, so to keep one consistent
//! Tree-Borrows access lineage the buffer's VALUE slots are read/written ONLY
//! through the EXPOSED address (`data_addr`, recovered with `with_exposed_provenance`
//! — the same int<->ptr discipline the arena cells use, object_store.rs
//! `allocate_blob`/`with_cell_mut`), NEVER through a safe `&mut buf[i]`. Mixing a
//! safe `&mut buf[i]` write with the exposed cell+8 access would DISABLE the exposed
//! tag (a sibling-write under Tree Borrows) and make the machine-code deref UB. The
//! ONLY safe `Box` access is REALLOCATION: a fresh `Box` is built + filled (before it
//! is exposed) and the old buffer is copied through its exposed address.
//!
//! DIVERGENCE (vs Butterfly.h): C++ keeps a preCapacity region and an
//! `IndexingHeader` slot between the property and element sides (`fromBase` adds
//! `+1`), carrying `publicLength`/`vectorLength` IN-BAND. The port omits the
//! IndexingHeader (so `base[-1]` is the first OOL prop, not the header) and tracks
//! `public_len`/`vector_cap` as struct fields instead; the faithful in-band
//! IndexingHeader + Auxiliary-subspace home is the deferred SD-4 follow-up
//! (docs/design/gc-r4-batch5.md). The negative-named / non-negative-element layout
//! and the `offsetInOutOfLineStorage` addressing ARE faithful.
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
use crate::object::property_offset::FIRST_OUT_OF_LINE_OFFSET;
use crate::value::JsValue as RuntimeValue;

/// Slot stride of the butterfly buffer: C++ `sizeof(EncodedJSValue)` == 8.
const SLOT_SIZE: usize = core::mem::size_of::<RuntimeValue>();

/// Handle to one object's auxiliary butterfly storage in the store-owned slab.
///
/// C++ JSC: a JSObject reaches its butterfly through the raw `m_butterfly`
/// pointer (JSObject.h:1167) into a GC Auxiliary allocation. The Rust analog is an
/// index into a store-owned slab (`CoreObjectStore::butterflies`) that OWNS each
/// buffer for free/Drop bookkeeping; the cell ALSO carries the raw base address
/// (gc-r4 Batch 5 Step 2 `CoreObjectCell::butterfly_base` @8) for machine-code
/// dereference. The handle is value-type-agnostic (a plain index), which is why it
/// lives here rather than beside either value rep.
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

/// One object's out-of-line butterfly region, over `RuntimeValue`, as the single
/// machine-addressable buffer (C++ `Butterfly`, Butterfly.h:134-150).
///
/// `buf` is one contiguous allocation: `[named props | indexed elements]`.
///   - `buf[0 .. named_len]` are the out-of-line property slots, reverse-packed so
///     the OOL property at offset `64 + k` lives at `buf[named_len - 1 - k]`
///     (= `base[-(k+1)]`). `named_len` == C++ `Structure::outOfLineSize`.
///   - `buf[named_len .. named_len + vector_cap]` are the indexed element slots;
///     `buf[named_len + i]` is element `i` (= `base[i]`). `public_len` is the live
///     element length (C++ `publicLength`); `vector_cap` is the allocated capacity
///     (C++ `vectorLength`); slots `[public_len .. vector_cap]` are spare holes.
/// A HOLE / never-written element is the in-band empty sentinel
/// (`RuntimeValue::default()`, `JSValue()` 0x0), NOT `Option`.
///
/// `data_addr` is the EXPOSED address of `buf[0]` (the buffer start); `base`
/// (the `m_butterfly`/`fromBase` position) is `data_addr + named_len*8`. All value
/// slots are read/written through `data_addr` (see the PROVENANCE DISCIPLINE note);
/// `data_addr` is recomputed (re-exposed) on every reallocation, `0` when empty.
#[derive(Debug, Default)]
pub struct ButterflyAllocation {
    /// The single contiguous `[named | elements]` buffer. Its data pointer is
    /// STABLE except across reallocation (growth) — the store registry `Vec` moves
    /// only this owning `Box` handle, never the heap buffer (the stably-addressed
    /// invariant the cell's raw base pointer relies on). Accessed for VALUE slots
    /// only through `data_addr` (exposed); the `Box` itself is the owner (Drop/free).
    buf: Box<[RuntimeValue]>,
    /// Live out-of-line property slots (C++ `Structure::outOfLineSize`).
    named_len: usize,
    /// Live indexed element length (C++ `Butterfly::publicLength`).
    public_len: usize,
    /// Allocated indexed element capacity (C++ `Butterfly::vectorLength`).
    vector_cap: usize,
    /// EXPOSED address of `buf[0]`, or `0` when `buf` is empty. Recomputed on every
    /// reallocation (see `recompute_data_addr`); every value-slot access derives a
    /// `with_exposed_provenance` pointer from it.
    data_addr: usize,
}

/// In-band hole / never-written-element sentinel: C++ `JSValue()` (ValueEmpty
/// 0x0). Distinct from a present `undefined` element (`arr[i] = undefined`), so a
/// hole and a stored `undefined` are not confused — exactly the distinction the
/// retired `Vec<Option<RuntimeValue>>` carried via `None` vs `Some(undefined)`.
#[inline]
fn hole() -> RuntimeValue {
    RuntimeValue::default()
}

#[inline]
fn is_hole(value: RuntimeValue) -> bool {
    value == hole()
}

impl Clone for ButterflyAllocation {
    fn clone(&self) -> Self {
        // C++ JSC copies a butterfly's storage when materializing a CopyOnWrite
        // array (JSObject.cpp). The clone is a FRESH allocation, so its `data_addr`
        // (an exposed pointer into THIS buffer) MUST be recomputed for the clone's
        // own buffer rather than copied verbatim (a derived-Clone would alias the
        // source's buffer address — the dangling-pointer landmine).
        let mut copy = ButterflyAllocation {
            buf: self.buf.clone(),
            named_len: self.named_len,
            public_len: self.public_len,
            vector_cap: self.vector_cap,
            data_addr: 0,
        };
        copy.recompute_data_addr();
        copy
    }
}

#[allow(dead_code)]
impl ButterflyAllocation {
    /// EXPOSED base address (the `m_butterfly`/`fromBase` position = `data_addr +
    /// named_len*8`), or `0` when the butterfly has no addressable storage yet. The
    /// cell's offset-8 raw pointer is synced to this; emitted machine code (Increment
    /// 2) loads `[cell+8]` to get it, then `[base + offsetInOutOfLineStorage*8]`.
    pub fn base_addr(&self) -> usize {
        if self.data_addr == 0 {
            0
        } else {
            self.data_addr + self.named_len * SLOT_SIZE
        }
    }

    /// Live out-of-line property slot count (C++ `Structure::outOfLineSize`).
    pub fn named_len(&self) -> usize {
        self.named_len
    }

    /// Re-EXPOSE the (re)allocated buffer and cache its start address. `expose_provenance`
    /// publishes the buffer's mutable provenance so every value-slot access (this module
    /// AND the cell's raw cell+8 deref) recovers it with `with_exposed_provenance`. `0`
    /// for an empty buffer (no `m_butterfly` yet).
    fn recompute_data_addr(&mut self) {
        self.data_addr = if self.buf.is_empty() {
            0
        } else {
            self.buf.as_mut_ptr().expose_provenance()
        };
    }

    /// Read a value slot through the exposed buffer address (the unified machine-load
    /// path; see PROVENANCE DISCIPLINE).
    ///
    /// SAFETY: `data_addr` is the exposed start of a live, non-empty butterfly buffer
    /// and `buf_index < buf.len()`, so the derived pointer is an interior `RuntimeValue`
    /// element; the buffer is single-mutator and never relocated by the non-moving GC.
    #[inline]
    unsafe fn load_at(data_addr: usize, buf_index: usize) -> RuntimeValue {
        debug_assert_ne!(data_addr, 0);
        unsafe {
            core::ptr::with_exposed_provenance::<RuntimeValue>(data_addr + buf_index * SLOT_SIZE)
                .read()
        }
    }

    /// Write a value slot through the exposed buffer address (the unified machine-store
    /// path; see PROVENANCE DISCIPLINE).
    ///
    /// SAFETY: as `load_at`, but the exposed address carries MUTABLE provenance
    /// (`as_mut_ptr`), so the write-through is valid; no safe `&`/`&mut` to the buffer is
    /// live during the write (the caller holds no aliasing borrow of `buf`).
    #[inline]
    unsafe fn store_at(data_addr: usize, buf_index: usize, value: RuntimeValue) {
        debug_assert_ne!(data_addr, 0);
        unsafe {
            core::ptr::with_exposed_provenance_mut::<RuntimeValue>(
                data_addr + buf_index * SLOT_SIZE,
            )
            .write(value);
        }
    }

    // ---- out-of-line named property side (negative indices below `base`) --------

    /// Buffer index of the OOL property at `offset` (>= firstOutOfLineOffset), or
    /// `None` if the shape never grew that slot. C++ `offsetInOutOfLineStorage(off) =
    /// -(off-64)-1`, mapped to `buf[named_len - 1 - (off-64)]` (= `base[-(off-64)-1]`).
    fn named_buf_index(&self, offset: i32) -> Option<usize> {
        debug_assert!(offset >= FIRST_OUT_OF_LINE_OFFSET);
        let k = (offset - FIRST_OUT_OF_LINE_OFFSET) as usize;
        if k < self.named_len {
            Some(self.named_len - 1 - k)
        } else {
            None
        }
    }

    /// Read the property at out-of-line `offset` (C++ `getDirect(offset)` OOL arm via
    /// `locationForOffset`, JSObject.h:711-723). `None` for an offset the shape never
    /// grew (a never-grown valid offset reads as `JSValue()` to the caller).
    pub fn prop_get(&self, offset: i32) -> Option<RuntimeValue> {
        let index = self.named_buf_index(offset)?;
        // SAFETY: `named_buf_index` returned Some => `named_len > 0` => the buffer was
        // grown and `data_addr != 0`, and `index < named_len <= buf.len()`.
        Some(unsafe { Self::load_at(self.data_addr, index) })
    }

    /// Write `value` into the property at out-of-line `offset`, growing the named
    /// side (REALLOCATING the buffer) if the slot does not yet exist. Returns `true`
    /// if the buffer REALLOCATED (the base address moved — the caller must rewrite
    /// cell+8 under the barrier). C++ `putDirectOffset` OOL arm + the
    /// `createOrGrowPropertyStorage` realloc (JSObject.cpp:3899 / Butterfly.h:238).
    pub fn prop_put(&mut self, offset: i32, value: RuntimeValue) -> bool {
        debug_assert!(offset >= FIRST_OUT_OF_LINE_OFFSET);
        let k = (offset - FIRST_OUT_OF_LINE_OFFSET) as usize;
        let moved = if k >= self.named_len {
            // Grow named storage to cover OOL property number `k`. OOL offsets are
            // assigned contiguously from 64 by the Structure (offsetForPropertyNumber),
            // so a growth extends by exactly one in the common path; `grow_named`
            // tolerates a larger jump (intermediate fresh slots read `undefined`).
            self.grow_named(k + 1);
            true
        } else {
            false
        };
        let index = self.named_len - 1 - k;
        // SAFETY: post-grow `named_len > k`, so `index < named_len <= buf.len()` and
        // `data_addr != 0`.
        unsafe { Self::store_at(self.data_addr, index, value) };
        moved
    }

    /// Clear the property at out-of-line `offset` back to `undefined` (deletion /
    /// data->accessor). No-op for a slot the shape never grew.
    pub fn prop_clear(&mut self, offset: i32) {
        if let Some(index) = self.named_buf_index(offset) {
            // SAFETY: `named_buf_index` Some => `index < named_len <= buf.len()`,
            // `data_addr != 0`.
            unsafe { Self::store_at(self.data_addr, index, RuntimeValue::undefined()) };
        }
    }

    /// Grow the named side so `named_len == new_named_len`, REALLOCATING the buffer.
    /// The live OOL props keep their base-relative position (offset 64 stays at
    /// `base[-1]`); the freshly exposed lower slots read `undefined` until written.
    /// The element side is copied verbatim (its `vector_cap` is unchanged). C++
    /// `Butterfly::createOrGrowPropertyStorage` (Butterfly.h:238).
    fn grow_named(&mut self, new_named_len: usize) {
        debug_assert!(new_named_len > self.named_len);
        let old_named_len = self.named_len;
        let old_data_addr = self.data_addr;
        let new_total = new_named_len + self.vector_cap;
        let mut new_buf = vec![hole(); new_total].into_boxed_slice();
        // The new lower slots (`[0 .. new_named_len - old_named_len)`) are fresh OOL
        // props; a grown-but-unwritten named slot reads `undefined` (getDirect on a
        // just-grown slot), matching the retired props-Vec `undefined` fill.
        for slot in new_buf.iter_mut().take(new_named_len - old_named_len) {
            *slot = RuntimeValue::undefined();
        }
        if old_data_addr != 0 {
            // Live OOL props occupy the TOP of the named region (nearest `base`): OOL `k`
            // is at `buf[named_len - 1 - k]`. Preserve that base-relative position.
            for k in 0..old_named_len {
                // SAFETY: copying from the still-live OLD buffer (exposed at `old_data_addr`);
                // `old_named_len - 1 - k < old_named_len <= old buf.len()`.
                new_buf[new_named_len - 1 - k] =
                    unsafe { Self::load_at(old_data_addr, old_named_len - 1 - k) };
            }
            // Element side: `buf[old_named_len + i]` -> `buf[new_named_len + i]`.
            for i in 0..self.vector_cap {
                // SAFETY: `old_named_len + i < old_named_len + vector_cap == old buf.len()`.
                new_buf[new_named_len + i] =
                    unsafe { Self::load_at(old_data_addr, old_named_len + i) };
            }
        }
        self.buf = new_buf;
        self.named_len = new_named_len;
        self.recompute_data_addr();
    }

    // ---- indexed element side (non-negative indices at/above `base`) ------------

    /// Read indexed element `index`; `None` for a hole or out-of-range
    /// (C++ `Butterfly::contiguous()` indexed load, Butterfly.h:196).
    pub fn elem_get(&self, index: usize) -> Option<RuntimeValue> {
        if index < self.public_len {
            // SAFETY: `index < public_len <= vector_cap` so `named_len + index <
            // buf.len()`, and `public_len > 0` => the buffer was grown (`data_addr != 0`).
            let value = unsafe { Self::load_at(self.data_addr, self.named_len + index) };
            if is_hole(value) {
                None
            } else {
                Some(value)
            }
        } else {
            None
        }
    }

    /// Write `value` into indexed element `index`, hole-filling growth and growing
    /// the vector capacity (REALLOCATING) when `index >= vector_cap`. Returns `true`
    /// if the buffer REALLOCATED (base moved). C++ `Butterfly::contiguous()` store.
    pub fn elem_put(&mut self, index: usize, value: RuntimeValue) -> bool {
        let moved = if index >= self.vector_cap {
            self.grow_vector(index + 1);
            true
        } else {
            false
        };
        // Slots between the old length and `index` become holes (already the empty
        // sentinel: spare capacity is hole-filled, in-place gaps are cleared on
        // shrink/clear), matching the contiguous "hole between old and new length".
        if index >= self.public_len {
            self.public_len = index + 1;
        }
        // SAFETY: post-grow `index < vector_cap`, so `named_len + index < buf.len()`,
        // `data_addr != 0`.
        unsafe { Self::store_at(self.data_addr, self.named_len + index, value) };
        moved
    }

    /// Resize the indexed element side to `len` slots (C++ `setVectorLength` /
    /// `setLength` clearing). Growth hole-fills (REALLOCATING the vector capacity if
    /// needed); shrink clears the truncated slots to holes and keeps the capacity.
    /// Returns `true` if the buffer REALLOCATED (base moved).
    pub fn elem_resize(&mut self, len: usize) -> bool {
        let moved = if len > self.vector_cap {
            self.grow_vector(len);
            true
        } else {
            false
        };
        if len < self.public_len {
            // Shrink: clear the dropped slots so a later regrow re-reads them as holes.
            for i in len..self.public_len {
                // SAFETY: `i < public_len <= vector_cap`, so `named_len + i < buf.len()`,
                // `data_addr != 0` (public_len > 0).
                unsafe { Self::store_at(self.data_addr, self.named_len + i, hole()) };
            }
        }
        self.public_len = len;
        moved
    }

    /// Live indexed element length (C++ `Butterfly::publicLength`, Butterfly.h:186).
    pub fn elem_len(&self) -> usize {
        self.public_len
    }

    /// Append a value to the indexed element side (C++ contiguous append; the
    /// `JSArray::push` fast path). Returns `true` if the buffer REALLOCATED.
    pub fn elem_push(&mut self, value: RuntimeValue) -> bool {
        self.elem_put(self.public_len, value)
    }

    /// Clear indexed element `index` to a hole (`delete arr[i]`); no-op out of range.
    pub fn elem_clear(&mut self, index: usize) {
        if index < self.public_len {
            // SAFETY: `index < public_len <= vector_cap` => in-bounds, `data_addr != 0`.
            unsafe { Self::store_at(self.data_addr, self.named_len + index, hole()) };
        }
    }

    /// Pop the last indexed element (`Array.prototype.pop` fast path); flattens a
    /// trailing hole to `None`. Shrinks `public_len` WITHOUT reallocating (keeps the
    /// vector capacity, like C++), so the base address does NOT move.
    pub fn elem_pop(&mut self) -> Option<RuntimeValue> {
        if self.public_len == 0 {
            return None;
        }
        self.public_len -= 1;
        let slot = self.named_len + self.public_len;
        // SAFETY: `slot = named_len + (public_len) < named_len + vector_cap == buf.len()`
        // (the value just below the old length), `data_addr != 0`.
        let value = unsafe { Self::load_at(self.data_addr, slot) };
        unsafe { Self::store_at(self.data_addr, slot, hole()) };
        if is_hole(value) {
            None
        } else {
            Some(value)
        }
    }

    /// Materialize the indexed element side as `Vec<Option<RuntimeValue>>` (hole ->
    /// `None`) for enumeration / snapshot reads. C++ `Butterfly::contiguous()` span.
    pub fn elements_vec(&self) -> Vec<Option<RuntimeValue>> {
        let data_addr = self.data_addr;
        (0..self.public_len)
            .map(|i| {
                // SAFETY: `i < public_len <= vector_cap` => in-bounds, `data_addr != 0`.
                let value = unsafe { Self::load_at(data_addr, self.named_len + i) };
                if is_hole(value) {
                    None
                } else {
                    Some(value)
                }
            })
            .collect()
    }

    /// Grow the indexed vector capacity to at least `min_vector_cap`, REALLOCATING.
    /// The named side is copied verbatim; live elements `[0..public_len]` are copied
    /// and the new spare `[public_len..vector_cap]` stays hole-filled. Capacity grows
    /// geometrically — the `Butterfly::ensureLengthSlow` / `nextCapacityAfter` analog
    /// (the growth FACTOR is not bit-identical to JSC; only amortized growth matters).
    fn grow_vector(&mut self, min_vector_cap: usize) {
        let new_vector_cap = self.next_vector_capacity(min_vector_cap);
        let old_data_addr = self.data_addr;
        let new_total = self.named_len + new_vector_cap;
        let mut new_buf = vec![hole(); new_total].into_boxed_slice();
        if old_data_addr != 0 {
            // Named side unchanged (same `named_len`): copy as-is.
            for j in 0..self.named_len {
                // SAFETY: `j < named_len <= old buf.len()`, exposed old buffer.
                new_buf[j] = unsafe { Self::load_at(old_data_addr, j) };
            }
            // Live elements copied; spare stays hole (the empty sentinel from `vec![hole()]`).
            for i in 0..self.public_len {
                // SAFETY: `named_len + i < named_len + public_len <= old buf.len()`.
                new_buf[self.named_len + i] =
                    unsafe { Self::load_at(old_data_addr, self.named_len + i) };
            }
        }
        self.buf = new_buf;
        self.vector_cap = new_vector_cap;
        self.recompute_data_addr();
    }

    /// Geometric vector-capacity growth (C++ `nextCapacityAfter`/`ensureLengthSlow`).
    fn next_vector_capacity(&self, min_vector_cap: usize) -> usize {
        const INITIAL_VECTOR_CAP: usize = 4;
        let geometric = if self.vector_cap == 0 {
            INITIAL_VECTOR_CAP
        } else {
            self.vector_cap + self.vector_cap / 2 + 1
        };
        geometric.max(min_vector_cap)
    }

    /// Visit every GC-edge value in the butterfly (each OOL property slot + each live
    /// indexed element, holes included). C++ `markAuxiliaryAndVisitOutOfLineProperties`
    /// value-append + `visitElements` (JSObject.cpp:108-111). The caller filters
    /// non-cell immediates (the `undefined`/empty fillers are not edges).
    pub fn for_each_value(&self, mut f: impl FnMut(RuntimeValue)) {
        let data_addr = self.data_addr;
        if data_addr == 0 {
            return; // empty butterfly: no slots
        }
        for j in 0..self.named_len {
            // SAFETY: `j < named_len <= buf.len()`, exposed buffer.
            f(unsafe { Self::load_at(data_addr, j) });
        }
        for i in 0..self.public_len {
            // SAFETY: `named_len + i < named_len + public_len <= buf.len()`.
            f(unsafe { Self::load_at(data_addr, self.named_len + i) });
        }
    }
}
