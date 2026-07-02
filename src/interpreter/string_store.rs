//! `CoreStringStore` — the live JSString/StringImpl-backed string-cell store.
//!
//! Phase E B1: extracted from `interpreter/mod.rs`. gc-r4-completion U1 (string-cell GC):
//! the string CELL is now a POD `CoreObjectStore::space` arena cell (identity = arena
//! address, R4a — faithful to JSC GC'ing JSString), so it is marked + swept + reclaimed
//! like an object cell. The variable StringImpl payload (bytes / substring coords / atom /
//! heap-binding id) is relocated OUT of the cell into this store's `string_records` slab
//! (gc-r4 SD-4 off-cell relocation; a String has no inline value slots). The former leaking
//! `Vec<Pin<Box<CoreStringCell>>>` is GONE; the arena IS the string-cell home.
//!
//! Faithful TARGET on the C++ side: Source/JavaScriptCore/runtime/JSString.{h,cpp} +
//! WTF strings/StringImpl.h. ONE Heap, multiple subspaces (HeapUtil.h): the string cell
//! shares `CoreObjectStore::space` with object cells, distinguished by `js_type` (StringType)
//! — the collector type-dispatches by header (U0) and the object deref islands reject leaf
//! cells (U0b `JSCell::isObject()` gate).

use super::object_store::CoreObjectStore;
use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct CoreStringStore {
    // gc-r4-completion U1 (SD-4) — the store-owned slab of out-of-line StringImpl payloads,
    // the home of each string cell's variable bytes (the analog of `CoreObjectStore::butterflies`
    // for objects; C++ JSString's out-of-line `StringImpl`). A `string_records` SLOT is reached
    // from a cell's arena address through `indices_by_payload`. `string_record_free_list`
    // recycles a DEAD string's slot index (the Auxiliary-subspace sweep analog), filled by
    // `reconcile_dead_string`.
    pub(crate) string_records: Vec<StringRecord>,
    pub(crate) string_record_free_list: Vec<usize>,
    // text -> the canonical interned string CELL's ARENA ADDRESS. The AtomStringTable analog,
    // and it is WEAK: remove-on-sweep BY IDENTITY (a dying StringImpl evicts itself,
    // `~StringImpl -> AtomStringImpl::remove`, WTF/wtf/text/StringImpl.cpp:118-129) — done in
    // `reconcile_dead_string`. A MARKED (live) interned string is never reconciled (only
    // unmarked cells are), so it is retained — correct by construction, never strong-rooted
    // (a strong root over the whole map would defeat the GC). Only FLAT strings are interned.
    pub(crate) by_text: HashMap<String, usize>,
    // cell ARENA ADDRESS -> `string_records` slot index. The string-cell RESOLUTION index: it
    // lets `text(value)` / `index_for_value` resolve a string value to its payload with a
    // store-local map lookup and NO arena deref (so the ~60 `strings.text(value)` callers keep
    // their `&self` signatures). It is the store-side analog of JSString's inline StringImpl
    // pointer (kept store-side like `CoreObjectStore::object_addr_by_cell_id`); the reconcile
    // drops a dead cell's entry. (Field name retained from the pre-arena `payload -> index`.)
    pub(crate) indices_by_payload: HashMap<usize, usize>,
}

/// One string cell's out-of-line StringImpl payload (gc-r4-completion U1 SD-4), held in the
/// store's `string_records` slab. Carries the variable bytes (or substring coords) + the atom
/// id + the lazily-bound heap `CellId` + the cell's own arena address (slot -> addr, for
/// `value_for_index` + the by-identity interning removal). All POD-ish; freed by
/// `reconcile_dead_string` when the cell is swept (the `~StringImpl` analog).
#[derive(Clone, Debug, Default)]
pub(crate) struct StringRecord {
    /// The owning string cell's arena address (= identity). `string_value_for_addr` rebuilds
    /// the `RuntimeValue` from it; the weak interning removal matches `by_text` by this address.
    pub(crate) addr: usize,
    pub(crate) text: CoreStringCellText,
    pub(crate) atom: Option<Identifier>,
    /// The lazily-bound heap `CellId` (the `payload<->cell` bridge id; default == unbound).
    /// Mirrors `CoreObjectCell::cell_id`; kept in the slab so binding mutates store-local state
    /// (no arena cell mutation island needed).
    pub(crate) cell_id: CellId,
}

/// The POD arena STRING CELL — the JSString JSCell header. The variable StringImpl payload
/// lives off-cell in `CoreStringStore::string_records` (SD-4), so the cell is a pure header
/// plus the rope FIBER edge.
///
/// `#[repr(C)]` pins the header layout so `js_type` sits at the kind-consistent offset 4 (the
/// same fixed `JSCell::m_type` offset every arena cell kind carries, so the collector's
/// type-dispatch reads any cell's kind from a raw address — see `arena_cell_kind_at`).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct CoreStringCell {
    // C++ JSC JSCell::m_structureID (runtime/JSCell.h, offset 0). JSString uses
    // `vm.stringStructure`; the port does not model a string Structure, so this is INVALID —
    // the cell is a pure header whose payload lives in the `string_records` slab.
    pub(crate) structure_id: StructureId,
    // C++ JSC JSCell::m_type == StringType (runtime/JSCell.h:298 / runtime/JSType.h:37) for
    // every JSString cell; isString() == (type == StringType) (runtime/JSCell.h:127). Read at
    // the fixed common offset 4 by the collector's type-dispatch + U0b's isObject gate.
    pub(crate) js_type: JsType,
    // ROPE FIBER edge — the base string cell's ARENA ADDRESS for a substring/rope cell, or the
    // 0 sentinel for a flat/empty string. C++ JSC `JSRopeString::m_fibers` (runtime/JSString.h):
    // a rope holds its fiber(s) inline on the cell, and `JSRopeString::visitChildrenImpl`
    // (runtime/JSString.cpp:104) visits them. The marker reads this inline edge directly
    // (`string_cell_rope_base`) so a live rope keeps its base string marked (the #1 UAF
    // landmine). For text RESOLUTION the SAME base also lives in the slab's `Substring{base}`
    // (the GC-edge-on-cell vs resolution-map split — a consequence of the off-cell payload).
    pub(crate) base: u64,
}

// Fixed, kind-consistent JSCell header offsets (mirrors CoreObjectCell's). js_type MUST sit at
// offset 4 so the collector reads any cell kind's `JSType` from a raw address.
const _: () = assert!(
    std::mem::offset_of!(CoreStringCell, structure_id) == 0,
    "CoreStringCell::structure_id must be at offset 0 (JSCell m_structureID)"
);
const _: () = assert!(
    std::mem::offset_of!(CoreStringCell, js_type) == 4,
    "CoreStringCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);
// POD: the MarkedBlock sweep runs NO destructor; a Drop field would leak (and break the blob
// copy in `admit_leaf_cell_blob`). The variable `String` bytes live in the slab, not here.
const _: () = assert!(
    !std::mem::needs_drop::<CoreStringCell>(),
    "CoreStringCell must be POD (no Drop) for the R4 MarkedBlock sweep + the blob copy"
);

/// The rope FIBER offset on the POD string cell — the base string cell's arena address slot.
pub(crate) const CORE_STRING_CELL_BASE_OFFSET: usize = std::mem::offset_of!(CoreStringCell, base);

/// gc-r4-completion U1/U4 — read a string cell's ROPE FIBER base edge from its arena bytes
/// (the `JSRopeString::m_fibers` read in `visitChildrenImpl`, runtime/JSString.cpp:104).
/// Returns the base cell's arena address for a ROPE/substring, or `None` for a flat/empty
/// string (the 0 sentinel). Called by the collector's `trace_leaf_cell`.
///
/// SAFETY: `addr` MUST be a byte-intact arena String cell (membership-gated + Leaf-classified
/// by the caller). `base` sits at `CORE_STRING_CELL_BASE_OFFSET` (const-asserted in-bounds);
/// the read copies a `u64` and forms no lasting reference.
pub(crate) unsafe fn string_cell_rope_base(addr: usize) -> Option<usize> {
    // SAFETY: see the contract above.
    let base = unsafe {
        core::ptr::with_exposed_provenance::<u64>(addr + CORE_STRING_CELL_BASE_OFFSET).read()
    };
    if base == 0 {
        None
    } else {
        Some(base as usize)
    }
}

/// The relocated string text payload (was an inline cell field pre-U1; now a `string_records`
/// slab variant). `Substring{base}` is the base cell's ARENA ADDRESS (was a Vec index pre-U1).
#[derive(Clone, Debug, Default)]
pub(crate) enum CoreStringCellText {
    #[default]
    Empty,
    Flat(String),
    Substring {
        /// The base string cell's ARENA ADDRESS (resolution mirror of the cell's `base` fiber).
        base: usize,
        start_byte: usize,
        end_byte: usize,
    },
}

const SHARED_SUBSTRING_MIN_CODE_UNITS: usize = 32;

/// Build + admit a POD `CoreStringCell` into the SHARED arena (`CoreObjectStore::space`) via
/// the leaf-cell admission chokepoint, returning its arena address (= identity). `base` is the
/// rope fiber (the base cell's arena address, or 0 for a flat/empty string).
fn admit_string_cell(objects: &mut CoreObjectStore, base: u64) -> usize {
    let cell = CoreStringCell {
        structure_id: StructureId::INVALID,
        js_type: JsType::String,
        base,
    };
    let len = core::mem::size_of::<CoreStringCell>();
    let src = core::ptr::from_ref(&cell).cast::<u8>();
    // SAFETY: `CoreStringCell` is POD (`needs_drop == false` asserted above) and `js_type` sits
    // at the const-asserted common offset; the interpreter store is single-threaded.
    // `admit_leaf_cell_blob` copies the bytes into a fresh arena slot + registers it live,
    // returning the arena address.
    unsafe { objects.admit_leaf_cell_blob(src, len) }
}

/// Rebuild the string `RuntimeValue` (identity) from a string cell's arena address — the leaf
/// analog of `CoreObjectStore::allocate_cell`'s `from_cell` tail.
fn string_value_for_addr(addr: usize) -> RuntimeValue {
    let ptr = core::ptr::with_exposed_provenance_mut::<CoreStringCell>(addr);
    let ptr = NonNull::new(ptr).expect("string cell arena address is non-null");
    // SAFETY: `addr` is a live arena string cell this store published; `from_cell` reads only
    // the pointer's integer bits (it never dereferences here); no GC moves a cell pre-R4b.
    RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
}

impl CoreStringStore {
    /// Allocate a slab record, REUSING a freed slot if one exists (the Auxiliary-subspace sweep
    /// reuse). Returns the slot index.
    fn push_record(&mut self, record: StringRecord) -> usize {
        if let Some(slot) = self.string_record_free_list.pop() {
            self.string_records[slot] = record; // drops the empty placeholder
            slot
        } else {
            let slot = self.string_records.len();
            self.string_records.push(record);
            slot
        }
    }

    pub(crate) fn allocate_untracked(
        &mut self,
        objects: &mut CoreObjectStore,
        text: &str,
    ) -> RuntimeValue {
        if let Some(&addr) = self.by_text.get(text) {
            return self.value_for_index(self.indices_by_payload[&addr]);
        }
        let addr = admit_string_cell(objects, 0);
        // gc-r4 leak-fix C1: report the StringImpl payload's byte length — a DIRECT match to
        // C++ `JSString::finishCreation(vm, length, cost)` (runtime/JSString.h:181,
        // `vm.heap.reportExtraMemoryAllocated(this, cost)`; `cost` = `StringImpl::cost()`,
        // the buffer's byte size). Unlike the butterfly/bigint sites, a WTF `StringImpl` IS a
        // genuine off-heap allocation in C++ too — see
        // `MarkedSpace::report_extra_memory_allocated`'s doc.
        objects.space.report_extra_memory_allocated(text.len());
        let slot = self.push_record(StringRecord {
            addr,
            text: CoreStringCellText::Flat(text.to_owned()),
            atom: None,
            cell_id: CellId::default(),
        });
        self.indices_by_payload.insert(addr, slot);
        self.by_text.insert(text.to_owned(), addr);
        self.value_for_index(slot)
    }

    pub(crate) fn allocate_with_heap(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        text: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(&addr) = self.by_text.get(text) {
            let slot = self.indices_by_payload[&addr];
            return self.bind_index_to_heap(heap, slot);
        }
        let addr = admit_string_cell(objects, 0);
        // gc-r4 leak-fix C1: see `allocate_untracked` — same direct match to C++
        // `JSString::finishCreation`'s `reportExtraMemoryAllocated(this, cost)`.
        objects.space.report_extra_memory_allocated(text.len());
        let slot = self.push_record(StringRecord {
            addr,
            text: CoreStringCellText::Flat(text.to_owned()),
            atom: None,
            cell_id: CellId::default(),
        });
        self.indices_by_payload.insert(addr, slot);
        self.by_text.insert(text.to_owned(), addr);
        self.bind_index_to_heap(heap, slot)
    }

    pub(crate) fn allocate_substring_with_heap(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        base_value: RuntimeValue,
        start: usize,
        end: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(base_slot) = self.index_for_value(base_value) else {
            return self.allocate_with_heap(objects, heap, "");
        };
        let base_addr = self.string_records[base_slot].addr;
        let substring = {
            let Some(text) = self.text_for_index(base_slot) else {
                return self.allocate_with_heap(objects, heap, "");
            };
            let length = string_code_unit_len(text);
            let start = start.min(length);
            let end = end.min(length);
            if start >= end {
                return self.allocate_with_heap(objects, heap, "");
            }
            if start == 0 && end == length {
                return self.bind_index_to_heap(heap, base_slot);
            }
            let substring_len = end.saturating_sub(start);
            if substring_len < SHARED_SUBSTRING_MIN_CODE_UNITS {
                let slice = string_slice_code_units(text, start, end);
                return self.allocate_with_heap(objects, heap, &slice);
            }
            let Some(start_byte) = string_byte_index_for_code_unit(text, start) else {
                let slice = string_slice_code_units(text, start, end);
                return self.allocate_with_heap(objects, heap, &slice);
            };
            let Some(end_byte) = string_byte_index_for_code_unit(text, end) else {
                let slice = string_slice_code_units(text, start, end);
                return self.allocate_with_heap(objects, heap, &slice);
            };
            if !text.is_ascii() {
                let slice = string_slice_code_units(text, start, end);
                return self.allocate_with_heap(objects, heap, &slice);
            }
            // Collapse a nested substring onto the ULTIMATE flat base CELL ADDRESS, so the rope
            // stays depth-1 (one fiber edge to the flat base — faithful to JSRopeString keeping
            // a single resolved base, not a chain).
            match &self.string_records[base_slot].text {
                CoreStringCellText::Substring {
                    base,
                    start_byte: base_start,
                    ..
                } => CoreStringCellText::Substring {
                    base: *base,
                    start_byte: base_start.saturating_add(start_byte),
                    end_byte: base_start.saturating_add(end_byte),
                },
                _ => CoreStringCellText::Substring {
                    base: base_addr,
                    start_byte,
                    end_byte,
                },
            }
        };
        // The rope's base cell ARENA ADDRESS = the cell's inline fiber edge (for the marker) +
        // the slab's `Substring{base}` (for resolution).
        let base_fiber = match &substring {
            CoreStringCellText::Substring { base, .. } => *base as u64,
            _ => 0,
        };
        // gc-r4 leak-fix C1: NO `report_extra_memory_allocated` call for this shared-substring
        // (rope) case — it stores a `{base, start_byte, end_byte}` fiber over the EXISTING
        // base cell's text, allocating no new text bytes (faithful to C++
        // `StringImpl::createSubstringSharingImpl`, which shares the base's buffer). The other
        // three arms above (`start >= end`, `substring_len < SHARED_SUBSTRING_MIN_CODE_UNITS`,
        // non-ASCII) fall through to `allocate_with_heap`, which DOES report (see its doc).
        let addr = admit_string_cell(objects, base_fiber);
        let slot = self.push_record(StringRecord {
            addr,
            text: substring,
            atom: None,
            cell_id: CellId::default(),
        });
        self.indices_by_payload.insert(addr, slot);
        // Substrings are NOT interned (no `by_text`) — matching the pre-U1 behavior.
        self.bind_index_to_heap(heap, slot)
    }

    pub(crate) fn allocate_atom_with_heap(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        identifier: Identifier,
        text: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        let value = self.allocate_with_heap(objects, heap, text)?;
        if let Some(slot) = self.index_for_value(value) {
            if self.string_records[slot].atom.is_none() {
                self.string_records[slot].atom = Some(identifier);
            }
        }
        Ok(value)
    }

    /// Lazily bind (or rebind) a string cell to the heap `payload<->cell` bridge, mirroring
    /// `CoreObjectStore::bind_object_to_heap`: bind the heap `CellId` to the cell's ARENA
    /// ADDRESS (not a Box pointer) and stamp it into the slab record. Returns the string value.
    pub(crate) fn bind_index_to_heap(
        &mut self,
        heap: &mut Heap,
        slot: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let addr = self.string_records[slot].addr;
        let cell_id = if let Some(cell_id) = heap.cell_for_payload(addr) {
            heap.publish_cell(cell_id)?;
            cell_id
        } else {
            let cell_id = allocate_primitive_interpreter_cell_id(
                heap,
                CellType::String,
                std::mem::size_of::<CoreStringCell>().max(1),
            )?;
            heap.bind_cell_payload(cell_id, addr)?;
            heap.publish_cell(cell_id)?;
            cell_id
        };
        self.string_records[slot].cell_id = cell_id;
        Ok(string_value_for_addr(addr))
    }

    /// gc-r4-completion U1 — the LEAF reconcile for ONE dead (unmarked) string cell, driven by
    /// the host from `CoreObjectStore::take_reclaimed_leaf_addrs` after a collection. Frees the
    /// cell's `string_records` slot (the `~StringImpl` payload free) and WEAK-removes its
    /// interning entry BY IDENTITY (`~StringImpl -> AtomStringImpl::remove`, WTF/wtf/text/
    /// StringImpl.cpp:118-129): the `by_text` entry is evicted ONLY if it still names THIS dead
    /// address. A no-op if `addr` is not one of this store's cells.
    pub(crate) fn reconcile_dead_string(&mut self, addr: usize) {
        let Some(slot) = self.indices_by_payload.remove(&addr) else {
            return;
        };
        // The interning key (only FLAT/empty strings are interned; substrings carry None).
        let intern_key: Option<String> = match &self.string_records[slot].text {
            CoreStringCellText::Flat(text) => Some(text.clone()),
            CoreStringCellText::Empty => Some(String::new()),
            CoreStringCellText::Substring { .. } => None,
        };
        if let Some(key) = intern_key {
            // BY IDENTITY: evict only if `by_text[key]` still resolves to THIS dead cell. (A
            // live re-intern cannot have replaced it, since a MARKED interned string is never
            // reconciled — only unmarked cells reach here — so this is exact, not racy.)
            if self.by_text.get(&key).copied() == Some(addr) {
                self.by_text.remove(&key);
            }
        }
        // Free the slab slot (drop the `String` payload + recycle the index).
        let _ = std::mem::take(&mut self.string_records[slot]);
        self.string_record_free_list.push(slot);
    }

    pub(crate) fn strict_equals(&self, left: RuntimeValue, right: RuntimeValue) -> Option<bool> {
        match (self.text(left), self.text(right)) {
            (Some(left), Some(right)) => Some(left == right),
            (Some(_), None) | (None, Some(_)) => Some(false),
            (None, None) => None,
        }
    }

    pub(crate) fn primitive_to_string(&self, value: RuntimeValue) -> Option<String> {
        if let Some(text) = self.text(value) {
            return Some(text.to_owned());
        }
        match value.kind() {
            ValueKind::Undefined => Some("undefined".to_owned()),
            ValueKind::Null => Some("null".to_owned()),
            ValueKind::Boolean => Some(if value.as_bool().unwrap_or(false) {
                "true".to_owned()
            } else {
                "false".to_owned()
            }),
            ValueKind::Int32 | ValueKind::Double => value.as_number().map(number_to_string),
            ValueKind::Cell | ValueKind::Unknown => None,
        }
    }

    pub(crate) fn text(&self, value: RuntimeValue) -> Option<&str> {
        let addr = value.as_cell()?.pointer_payload_bits();
        self.text_for_addr(addr)
    }

    /// Resolve a string cell's arena address to its text via the store-local resolution map +
    /// slab (NO arena deref). The rope/substring case recurses through the base cell's address.
    pub(crate) fn text_for_addr(&self, addr: usize) -> Option<&str> {
        let slot = *self.indices_by_payload.get(&addr)?;
        self.text_for_index(slot)
    }

    pub(crate) fn text_for_index(&self, slot: usize) -> Option<&str> {
        match &self.string_records.get(slot)?.text {
            CoreStringCellText::Empty => Some(""),
            CoreStringCellText::Flat(text) => Some(text.as_str()),
            CoreStringCellText::Substring {
                base,
                start_byte,
                end_byte,
            } => self.text_for_addr(*base)?.get(*start_byte..*end_byte),
        }
    }

    pub(crate) fn atom_identifier(&self, value: RuntimeValue) -> Option<Identifier> {
        let slot = self.index_for_value(value)?;
        self.string_records[slot].atom
    }

    pub(crate) fn index_for_value(&self, value: RuntimeValue) -> Option<usize> {
        let addr = value.as_cell()?.pointer_payload_bits();
        self.indices_by_payload.get(&addr).copied()
    }

    pub(crate) fn value_for_index(&self, slot: usize) -> RuntimeValue {
        let addr = self.string_records[slot].addr;
        string_value_for_addr(addr)
    }
}
