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
use std::cell::{Cell as StdCell, OnceCell};

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
    // gc-r4 leak-fix B — rope-resolve extra-memory cost awaiting the collection trigger.
    // C++ reports INLINE at resolution (`vm.heap.reportExtraMemoryAllocated(this,
    // sizeToReport)`, runtime/JSString.cpp:252-257), but the port's resolveRope analog runs
    // under a SHARED borrow (the `OnceCell` fill inside `text()`, the Rust spelling of C++'s
    // `const resolveRope` + `mutable` fields), so the cost is accumulated here and drained
    // into `MarkedSpace::report_extra_memory_allocated` by `drain_pending_resolved_bytes`
    // at every allocation chokepoint + the GC safepoint poll. Equivalent trigger timing:
    // a collection only ever starts at a safepoint (see `MarkedSpace::allocate_blob`).
    pub(crate) pending_resolved_bytes: StdCell<usize>,
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
/// plus the rope FIBER word.
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
    // NOTE: C++ gives JSRopeString the SAME StringType — rope-ness is encoded in the fiber
    // word, not the type byte (`isRopeInPointer`, runtime/JSString.h:348).
    //
    // LAYOUT CONTRACT (leak-fix B evidence): cell byte 7 is MARKER-OWNED — the SlotVisitor
    // writes the JSCell `m_cellState` byte there on every white→grey / grey→black
    // transition (`set_cell_state`, gc/heap/slot_visitor.rs:144-153, pinned by
    // `marked_block::JsCellHeader` @7 = C++ JSCell::m_cellState). String-cell payload
    // therefore starts at offset 8; bytes 5..7 stay reserved header room (C++ m_type /
    // m_flags live there).
    pub(crate) js_type: JsType,
    // ROPE FIBER word — C++ `JSString::m_fiber` (runtime/JSString.h: the one pointer-wide
    // field every JSString carries; JSC packs rope FLAGS into its low bits — "We use lower
    // 3bits of fiber0 for flags. These bits are usable due to alignment", JSString.h:
    // 343-349). The port mirrors that exactly: arena cells are ATOM_SIZE=16-aligned
    // (MarkedBlock::atomSize, gc/heap/marked_block.rs:99), so the low 4 bits of a fiber
    // address are free for flags.
    //  - flat/empty string:  0 (no fiber, no flags)
    //  - shared substring:   the base cell's arena address (untagged — one fiber edge,
    //    like JSC's isSubstring rope visiting only fiber1, JSString.cpp:112-114)
    //  - 2-fiber concat:     fiber0's address | `ROPE_CELL_HAS_FIBER1_IN_POINTER`; the
    //    cell is then the LARGER `CoreRopeStringCell` and fiber1 lives at offset 16
    // `JSRopeString::visitChildrenImpl` (runtime/JSString.cpp:104-140) visits EVERY fiber;
    // the marker reads this inline word directly (`string_cell_fibers`) so a live rope
    // keeps its fiber strings marked (the #1 UAF landmine). For text RESOLUTION the SAME
    // addresses also live in the slab's `Substring{base}` / `Rope{fiber0,fiber1}` (the
    // GC-edge-on-cell vs resolution-map split — a consequence of the off-cell payload).
    pub(crate) fiber0: u64,
}

/// leak-fix B — the POD arena 2-FIBER CONCAT ROPE cell: C++ `JSRopeString`. UNRATIFIED
/// LAYOUT DECISION (returned to the orchestrator as the leak-fix B architecture question):
/// the ratified "fit 2 fibers into the existing 16-byte cell" is IMPOSSIBLE — the marker
/// owns cell byte 7 (`m_cellState`, see `CoreStringCell`), leaving 8 payload bytes = 64
/// bits, while two fibers need >= 88 bits even with JSC's own 48-bit + atom-alignment
/// packing (`CompactFibers`, JSString.h:350-407). The faithful resolution implemented here
/// mirrors C++ itself: JSRopeString IS a separate, LARGER cell — sizeof(JSRopeString) == 32
/// vs sizeof(JSString) == 16 (the 1bbd6bf9 shrink note in the rope-string design node) —
/// allocated from its own subspace (`vm.ropeStringSpace()`, JSString.h:337-341). The port
/// gives the concat rope a 32-byte cell in the shared arena's 32-byte size class
/// (`allocate_blob` routes by size, gc/heap/marked_space.rs). Flat/substring strings keep
/// the 16-byte `CoreStringCell` unchanged.
///
/// The header prefix (structure_id/js_type/fiber0) is IDENTICAL to `CoreStringCell`, so
/// every header-offset reader (type dispatch, fiber0 trace) works on both kinds; the
/// `ROPE_CELL_HAS_FIBER1_IN_POINTER` low bit of `fiber0` is what licenses reading the
/// wider layout (the port's `isRopeInPointer` analog, JSString.h:348).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct CoreRopeStringCell {
    /// See `CoreStringCell::structure_id` (INVALID; JSC ropes use the same stringStructure).
    pub(crate) structure_id: StructureId,
    /// StringType, like every JSString (rope-ness lives in `fiber0`'s low bit).
    pub(crate) js_type: JsType,
    /// Fiber 0's arena address, tagged with `ROPE_CELL_HAS_FIBER1_IN_POINTER` (JSC packs
    /// its rope flags into the same low alignment bits, JSString.h:343-349).
    pub(crate) fiber0: u64,
    /// Fiber 1's arena address (C++ CompactFibers fiber1, JSString.h:352-406 — stored
    /// there as split 48-bit halves; a plain word here, the split is a cell-size
    /// micro-optimization the 32-byte budget does not require).
    pub(crate) fiber1: u64,
    /// Reserved to keep sizeof == 32 == C++ sizeof(JSRopeString) (C++ uses this room for
    /// CompactFibers' m_length + fiber2 halves; the port keeps length in the slab record
    /// and has no 3-fiber form — see `CoreStringCellText::Rope`).
    pub(crate) reserved: u64,
}

/// The port's `isRopeInPointer`-style flag (runtime/JSString.h:343-349: "We use lower
/// 3bits of fiber0 for flags. These bits are usable due to alignment"): set on a concat
/// rope cell's `fiber0` word to license reading `fiber1` at offset 16 (i.e. "this is the
/// 32-byte `CoreRopeStringCell`"). Substring cells carry an UNTAGGED base address.
const ROPE_CELL_HAS_FIBER1_IN_POINTER: u64 = 0b1;
const ROPE_CELL_FIBER_FLAG_MASK: u64 = 0b1111; // the 4 alignment bits reserved for flags

// Fixed, kind-consistent JSCell header offsets (mirrors CoreObjectCell's). js_type MUST sit
// at offset 4 so the collector reads any cell kind's `JSType` from a raw address; fiber
// words MUST clear byte 7 (`m_cellState`, marker-owned — see `CoreStringCell::js_type` doc).
const _: () = assert!(
    std::mem::offset_of!(CoreStringCell, structure_id) == 0,
    "CoreStringCell::structure_id must be at offset 0 (JSCell m_structureID)"
);
const _: () = assert!(
    std::mem::offset_of!(CoreStringCell, js_type) == 4,
    "CoreStringCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);
const _: () = assert!(
    std::mem::offset_of!(CoreStringCell, fiber0) == 8,
    "CoreStringCell::fiber0 must sit after the 8-byte JSCell header (byte 7 = m_cellState)"
);
const _: () = assert!(
    std::mem::size_of::<CoreStringCell>() == 16,
    "CoreStringCell must stay exactly 16 bytes (one MarkedBlock atom; C++ sizeof(JSString))"
);
const _: () = assert!(
    std::mem::offset_of!(CoreRopeStringCell, structure_id) == 0
        && std::mem::offset_of!(CoreRopeStringCell, js_type) == 4
        && std::mem::offset_of!(CoreRopeStringCell, fiber0) == 8,
    "CoreRopeStringCell must share the CoreStringCell header prefix"
);
const _: () = assert!(
    std::mem::offset_of!(CoreRopeStringCell, fiber1) == 16,
    "CoreRopeStringCell::fiber1 must sit at offset 16 (read by string_cell_fibers)"
);
const _: () = assert!(
    std::mem::size_of::<CoreRopeStringCell>() == 32,
    "CoreRopeStringCell must be exactly 32 bytes (C++ sizeof(JSRopeString))"
);
// POD: the MarkedBlock sweep runs NO destructor; a Drop field would leak (and break the blob
// copy in `admit_leaf_cell_blob`). The variable `String` bytes live in the slab, not here.
const _: () = assert!(
    !std::mem::needs_drop::<CoreStringCell>() && !std::mem::needs_drop::<CoreRopeStringCell>(),
    "string cells must be POD (no Drop) for the R4 MarkedBlock sweep + the blob copy"
);

/// The rope FIBER-WORD offset shared by both string cell kinds.
pub(crate) const CORE_STRING_CELL_FIBER0_OFFSET: usize =
    std::mem::offset_of!(CoreStringCell, fiber0);
const CORE_ROPE_STRING_CELL_FIBER1_OFFSET: usize = std::mem::offset_of!(CoreRopeStringCell, fiber1);

/// Tag one fiber address for a fiber word. Hard asserts (not debug): a violated alignment
/// invariant would silently corrupt a GC edge — the same "usable due to alignment" contract
/// JSC states for its fiber flag bits (runtime/JSString.h:343-349).
fn tagged_fiber_word(addr: usize, flags: u64) -> u64 {
    assert!(
        addr as u64 & ROPE_CELL_FIBER_FLAG_MASK == 0,
        "string-cell fiber address must be ATOM_SIZE-aligned (low bits carry flags)"
    );
    addr as u64 | flags
}

/// gc-r4-completion U1/U4 + leak-fix B — read a string cell's ROPE FIBER edges from its
/// arena bytes (the fiber reads in `JSRopeString::visitChildrenImpl`, runtime/
/// JSString.cpp:104-140, which visits EVERY non-null fiber). Returns each fiber cell's
/// arena address, or `None` per empty slot (flat/empty strings carry a zero fiber word).
/// Called by the collector's `trace_leaf_cell` — BOTH returned fibers must be traced.
/// `fiber1` (offset 16) is read ONLY when fiber0's `ROPE_CELL_HAS_FIBER1_IN_POINTER` flag
/// proves the cell is the 32-byte `CoreRopeStringCell` (the `isRopeInPointer`-style
/// dispatch JSC performs on the same word, JSString.cpp:110-112).
///
/// SAFETY: `addr` MUST be a byte-intact arena String cell (membership-gated + Leaf-classified
/// by the caller). The fiber words sit at const-asserted in-bounds offsets; the reads copy
/// `u64`s and form no lasting reference.
pub(crate) unsafe fn string_cell_fibers(addr: usize) -> [Option<usize>; 2] {
    // SAFETY: see the contract above.
    let word = unsafe {
        core::ptr::with_exposed_provenance::<u64>(addr + CORE_STRING_CELL_FIBER0_OFFSET).read()
    };
    if word == 0 {
        return [None, None];
    }
    let fiber0 = Some((word & !ROPE_CELL_FIBER_FLAG_MASK) as usize);
    if word & ROPE_CELL_HAS_FIBER1_IN_POINTER == 0 {
        return [fiber0, None];
    }
    // SAFETY: the flag proved this is the 32-byte CoreRopeStringCell, so offset 16 is
    // in-bounds cell memory.
    let fiber1 = unsafe {
        core::ptr::with_exposed_provenance::<u64>(addr + CORE_ROPE_STRING_CELL_FIBER1_OFFSET).read()
    };
    [fiber0, Some(fiber1 as usize)]
}

/// The port's `convertToNonRope` fiber drop (declared runtime/JSString.h:532; the resolve
/// paths install the resolved StringImpl over `m_fiber`, runtime/JSString.cpp:240/255/271,
/// after which `visitChildrenImpl` no longer sees fiber edges): zero both fiber words so a
/// RESOLVED concat rope stops retaining its children (they become collectable exactly as
/// in C++).
///
/// SAFETY: `addr` MUST be a live arena `CoreRopeStringCell` this store published (resolve
/// only runs on `Rope` records). `&self`-safe raw write: the cell slot is interior-mutable
/// once-exposed page memory (the same contract as `MarkedSpace::shadow_write`) and there is
/// a single mutator thread; the collector only runs at safepoints, never concurrently.
unsafe fn clear_rope_string_cell_fibers(addr: usize) {
    let dst = core::ptr::with_exposed_provenance_mut::<u8>(addr + CORE_STRING_CELL_FIBER0_OFFSET);
    // SAFETY: see the contract above — zeroes fiber0+fiber1 (offsets 8..24) in-bounds of
    // the 32-byte rope cell; no reference formed.
    unsafe { core::ptr::write_bytes(dst, 0, 2 * core::mem::size_of::<u64>()) };
}

/// The relocated string text payload (was an inline cell field pre-U1; now a `string_records`
/// slab variant). `Substring{base}` / `Rope{fiber0,fiber1}` carry the fiber cells' ARENA
/// ADDRESSES (resolution mirrors of the cell's packed inline fibers).
#[derive(Clone, Debug, Default)]
pub(crate) enum CoreStringCellText {
    #[default]
    Empty,
    Flat(String),
    Substring {
        /// The base string cell's ARENA ADDRESS (resolution mirror of the cell's fiber0).
        base: usize,
        start_byte: usize,
        end_byte: usize,
    },
    /// leak-fix B — the LAZY 2-fiber CONCAT rope (C++ JSRopeString's 2-fiber form,
    /// runtime/JSString.h:567-578; created by the string `+` jsString overloads,
    /// runtime/OperationsInlines.h:98/131/161). String `+` allocates this O(1) node
    /// instead of eagerly copying both operands; `resolveRope` flattens ONCE on first
    /// flat-text use (runtime/JSString.cpp:228-283). JSC allows up to
    /// `s_maxInternalRopeLength = 3` fibers (JSString.h:623) — the ratified first step
    /// ports the 2-fiber node only (jsAdd only ever creates 2-fiber ropes; the 3-fiber
    /// form serves op_strcat's RopeBuilder, and a third fiber would not fit the packed
    /// 16-byte cell).
    Rope {
        /// Left operand's cell ARENA ADDRESS (JSRopeString fiber0). GC-edge mirror; stale
        /// (never read) once `resolved` is filled — the cell's inline fibers are zeroed at
        /// resolution (`convertToNonRope`) and the children may be swept afterwards.
        fiber0: usize,
        /// Right operand's cell ARENA ADDRESS (JSRopeString fiber1).
        fiber1: usize,
        /// The rope's UTF-16 code-unit length, fixed at creation — C++ keeps it inline
        /// (`CompactFibers::m_length`, JSString.h:386-390) so `JSString::length()` on a
        /// rope is O(1) and NEVER resolves (JSString.h:524-527).
        len_code_units: usize,
        /// The resolve-once cache (`convertToNonRope(String&&)` result, JSString.h:532).
        /// `OnceCell` is the Rust spelling of C++'s `const` `resolveRope` mutating
        /// `mutable` fields (JSString.h:627 — resolveRope IS a const member function):
        /// it lets the ~60 `&self` text() callers trigger the one-time flatten.
        resolved: OnceCell<String>,
    },
}

const SHARED_SUBSTRING_MIN_CODE_UNITS: usize = 32;

/// Build + admit a POD `CoreStringCell` into the SHARED arena (`CoreObjectStore::space`) via
/// the leaf-cell admission chokepoint, returning its arena address (= identity). `base` is
/// the single fiber word (the base cell's arena address for a shared substring, or 0 for a
/// flat/empty string) — a 2-fiber concat rope uses `admit_rope_string_cell` instead.
fn admit_string_cell(objects: &mut CoreObjectStore, base: usize) -> usize {
    let cell = CoreStringCell {
        structure_id: StructureId::INVALID,
        js_type: JsType::String,
        fiber0: tagged_fiber_word(base, 0),
    };
    let len = core::mem::size_of::<CoreStringCell>();
    let src = core::ptr::from_ref(&cell).cast::<u8>();
    // SAFETY: `CoreStringCell` is POD (`needs_drop == false` asserted above) and `js_type` sits
    // at the const-asserted common offset; the interpreter store is single-threaded.
    // `admit_leaf_cell_blob` copies the bytes into a fresh arena slot + registers it live,
    // returning the arena address.
    unsafe { objects.admit_leaf_cell_blob(src, len) }
}

/// leak-fix B — build + admit a POD 32-byte `CoreRopeStringCell` (C++ JSRopeString; see the
/// struct doc for the layout decision) through the SAME leaf-cell chokepoint; the blob's
/// size routes it into the arena's 32-byte size class (`allocate_blob` -> `size_route`,
/// the port's stand-in for JSC's dedicated `ropeStringSpace()` IsoSubspace,
/// JSString.h:337-341).
fn admit_rope_string_cell(objects: &mut CoreObjectStore, fiber0: usize, fiber1: usize) -> usize {
    let cell = CoreRopeStringCell {
        structure_id: StructureId::INVALID,
        js_type: JsType::String,
        fiber0: tagged_fiber_word(fiber0, ROPE_CELL_HAS_FIBER1_IN_POINTER),
        fiber1: tagged_fiber_word(fiber1, 0),
        reserved: 0,
    };
    let len = core::mem::size_of::<CoreRopeStringCell>();
    let src = core::ptr::from_ref(&cell).cast::<u8>();
    // SAFETY: `CoreRopeStringCell` is POD with `js_type` at the const-asserted common
    // offset; single mutator thread (same contract as `admit_string_cell`).
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
        self.drain_pending_resolved_bytes(objects);
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
        self.drain_pending_resolved_bytes(objects);
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
        self.drain_pending_resolved_bytes(objects);
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
            CoreStringCellText::Substring { base, .. } => *base,
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

    /// leak-fix B — `JSRopeString::create(vm, s1, s2)` (the 2-fiber ctor, runtime/
    /// JSString.h:567-578; creation sites: the jsString overloads, runtime/
    /// OperationsInlines.h:98/131/161): the LAZY string `+`. Allocates an O(1) rope cell
    /// holding both fiber edges + the summed length, deferring the byte copy to
    /// `resolve_rope_for_index` on first flat-text use.
    ///
    /// NO `report_extra_memory_allocated` here — a rope allocates ~no text bytes:
    /// JSRopeString's finishCreation is the plain `Base::finishCreation(vm)` (JSString.h:
    /// 607-610 and the rope ctors), unlike `JSString::finishCreation(vm, length, cost)`
    /// which reports the StringImpl cost (JSString.h:181). The cost is reported at
    /// RESOLUTION instead (JSString.cpp:252-257), which is what makes `s += chunk` report
    /// O(total) instead of O(n^2) bytes.
    ///
    /// Callers guarantee both operands are string cells and non-empty (the jsString
    /// empty-side shortcuts return the other operand BEFORE reaching rope creation,
    /// OperationsInlines.h:149-154); a non-string operand is a defensive error.
    pub(crate) fn allocate_rope_with_heap(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        left: RuntimeValue,
        right: RuntimeValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        self.drain_pending_resolved_bytes(objects);
        let (Some(left_slot), Some(right_slot)) =
            (self.index_for_value(left), self.index_for_value(right))
        else {
            return Err(ExecutionError::ExpectedObject);
        };
        // `initializeLength(s1->length() + s2->length())` (JSString.h:567-578): O(1) for a
        // rope operand (its own cached length), one scan for a flat operand.
        let len_code_units = self
            .code_unit_length_for_index(left_slot)
            .unwrap_or(0)
            .saturating_add(self.code_unit_length_for_index(right_slot).unwrap_or(0));
        let fiber0 = self.string_records[left_slot].addr;
        let fiber1 = self.string_records[right_slot].addr;
        let addr = admit_rope_string_cell(objects, fiber0, fiber1);
        let slot = self.push_record(StringRecord {
            addr,
            text: CoreStringCellText::Rope {
                fiber0,
                fiber1,
                len_code_units,
                resolved: OnceCell::new(),
            },
            atom: None,
            cell_id: CellId::default(),
        });
        self.indices_by_payload.insert(addr, slot);
        // Ropes are NOT interned (no `by_text`): C++ JSRopeString::create never touches the
        // atom table; only flat allocations intern.
        self.bind_index_to_heap(heap, slot)
    }

    /// leak-fix B — drain the rope-resolve extra-memory cost into the collection trigger
    /// (see the `pending_resolved_bytes` field doc: the C++ inline
    /// `reportExtraMemoryAllocated` at resolveRope, JSString.cpp:252-257, deferred to the
    /// next allocation chokepoint / safepoint poll because the port resolves under `&self`).
    pub(crate) fn drain_pending_resolved_bytes(&self, objects: &mut CoreObjectStore) {
        let bytes = self.pending_resolved_bytes.take();
        if bytes != 0 {
            objects.space.report_extra_memory_allocated(bytes);
        }
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
            // leak-fix B note: 32-byte `CoreRopeStringCell`s register here at
            // sizeof(CoreStringCell)=16 too — bridge-BOOKKEEPING-only (the legacy
            // payload<->cell id table, the known stale-id-table residual), never a
            // deref/layout input; rides that residual until the bridge retires.
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
        // The interning key (only FLAT/empty strings are interned; substrings and ropes
        // carry None — a rope's resolved cache is never interned either).
        let intern_key: Option<String> = match &self.string_records[slot].text {
            CoreStringCellText::Flat(text) => Some(text.clone()),
            CoreStringCellText::Empty => Some(String::new()),
            CoreStringCellText::Substring { .. } | CoreStringCellText::Rope { .. } => None,
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
            // Flat-text access on a rope RESOLVES it (once): every C++ character accessor
            // funnels through `resolveRope` the same way (`JSString::value/view`,
            // runtime/JSString.h — `if (isRope()) resolveRope(...)`), so comparisons,
            // hashing/interning, property-key conversion, regexp/JSON inputs and every
            // other `text()` caller flatten lazily and exactly once.
            CoreStringCellText::Rope { .. } => self.resolve_rope_for_index(slot),
        }
    }

    /// leak-fix B — `JSRopeString::resolveRope` (runtime/JSString.cpp:228-283,
    /// `resolveRopeWithFunction`): flatten a concat rope ONCE on first flat-text access,
    /// cache the result (`convertToNonRope`), queue the flattened byte cost for the
    /// collection trigger (JSString.cpp:252-257), and drop the cell's fiber edges so the
    /// children become collectable.
    ///
    /// ITERATIVE (explicit stack), not recursive: a `s += chunk` loop builds an O(n)-deep
    /// left-leaning rope, and C++ itself had to replace naive recursion (MUST_TAIL_CALL
    /// resolution, f941f5eb; bounded-loop substring descent, 93f2fd68) — a recursive port
    /// would overflow the Rust stack the same way. Every leaf's bytes are appended exactly
    /// once, so resolving a whole concat chain is O(total bytes)
    /// (`resolveRopeInternalNoSubstring` -> `resolveToBuffer`, JSString.cpp:144-148).
    fn resolve_rope_for_index(&self, slot: usize) -> Option<&str> {
        let record = self.string_records.get(slot)?;
        let CoreStringCellText::Rope {
            fiber0,
            fiber1,
            len_code_units,
            resolved,
        } = &record.text
        else {
            return None;
        };
        if let Some(text) = resolved.get() {
            return Some(text.as_str());
        }
        // In-order flatten: pop order fiber0 -> fiber1 (code units ~= bytes for ASCII, so
        // the length doubles as a capacity hint).
        let mut out = String::with_capacity(*len_code_units);
        let mut stack: Vec<usize> = vec![*fiber1, *fiber0];
        while let Some(fiber_addr) = stack.pop() {
            let Some(&fiber_slot) = self.indices_by_payload.get(&fiber_addr) else {
                // A live UNRESOLVED rope's fibers are traced (`trace_leaf_cell`), so its
                // children cannot have been reconciled dead while it still resolves.
                debug_assert!(false, "unresolved rope fiber has no live record");
                continue;
            };
            match &self.string_records[fiber_slot].text {
                CoreStringCellText::Empty => {}
                CoreStringCellText::Flat(text) => out.push_str(text),
                CoreStringCellText::Substring { .. } => {
                    // Substring fibers contribute their shared slice (depth-bounded: a
                    // substring's base is always flat or an already-resolved rope).
                    if let Some(text) = self.text_for_index(fiber_slot) {
                        out.push_str(text);
                    }
                }
                CoreStringCellText::Rope {
                    fiber0,
                    fiber1,
                    resolved,
                    ..
                } => {
                    if let Some(text) = resolved.get() {
                        // An already-resolved nested rope contributes its cached text; its
                        // fibers were dropped at ITS resolution and must not be walked.
                        out.push_str(text);
                    } else {
                        stack.push(*fiber1);
                        stack.push(*fiber0);
                    }
                }
            }
        }
        let resolved_byte_len = out.len();
        // First (and only) resolution: fill the cache (convertToNonRope) ...
        if resolved.set(out).is_err() {
            debug_assert!(false, "rope resolved reentrantly");
        }
        // ... queue the flattened cost (`sizeToReport = newImpl->cost()` +
        // `reportExtraMemoryAllocated`, JSString.cpp:252-257; drained store-side, see
        // `drain_pending_resolved_bytes`) ...
        self.pending_resolved_bytes.set(
            self.pending_resolved_bytes
                .get()
                .saturating_add(resolved_byte_len),
        );
        // ... and drop the cell's inline fiber edges so the children become collectable.
        // SAFETY: `record.addr` is this store's live arena string cell; single mutator
        // thread; the collector only runs at safepoints (see `clear_rope_string_cell_fibers`).
        unsafe { clear_rope_string_cell_fibers(record.addr) };
        resolved.get().map(String::as_str)
    }

    /// `JSString::length()` — O(1) for EVERY representation and it NEVER resolves a rope:
    /// C++ keeps the rope length inline (`m_compactFibers.length()`, runtime/JSString.h:
    /// 524-527), so `str.length` on a rope must not flatten it (a flatten-on-length would
    /// re-create the O(n^2) growth the rope exists to fix). `None` for a non-string value.
    pub(crate) fn code_unit_length(&self, value: RuntimeValue) -> Option<usize> {
        let slot = self.index_for_value(value)?;
        self.code_unit_length_for_index(slot)
    }

    /// `code_unit_length` clamped like `string_code_unit_len_i32` (JSString::MaxLength is
    /// int32 max, runtime/JSString.h:86).
    pub(crate) fn code_unit_length_i32(&self, value: RuntimeValue) -> Option<i32> {
        self.code_unit_length(value)
            .map(|length| i32::try_from(length).unwrap_or(i32::MAX))
    }

    fn code_unit_length_for_index(&self, slot: usize) -> Option<usize> {
        match &self.string_records.get(slot)?.text {
            CoreStringCellText::Empty => Some(0),
            CoreStringCellText::Flat(text) => Some(string_code_unit_len(text)),
            // Shared substrings are ASCII-only (see allocate_substring_with_heap), so
            // bytes == code units.
            CoreStringCellText::Substring {
                start_byte,
                end_byte,
                ..
            } => Some(end_byte.saturating_sub(*start_byte)),
            CoreStringCellText::Rope { len_code_units, .. } => Some(*len_code_units),
        }
    }

    /// `JSString::isRope()` for the jsString eager-vs-lazy check (runtime/
    /// OperationsInlines.h:97/130 — an eager concat needs `s->value()`, which would
    /// resolve, so any rope operand routes to rope creation): true for a shared substring
    /// (C++ substring ropes ARE ropes — `isSubstring` implies `isRope`) and for an
    /// UNRESOLVED concat rope; false once resolved (`convertToNonRope`). Divergence note:
    /// the port's substrings never convert to non-rope (they stay permanent lazy views,
    /// pre-existing behavior), so they remain "ropes" here indefinitely — harmless, it only
    /// routes tiny mixed concats onto the rope path C++ would also take.
    pub(crate) fn is_rope(&self, value: RuntimeValue) -> bool {
        let Some(slot) = self.index_for_value(value) else {
            return false;
        };
        match &self.string_records[slot].text {
            CoreStringCellText::Substring { .. } => true,
            CoreStringCellText::Rope { resolved, .. } => resolved.get().is_none(),
            CoreStringCellText::Empty | CoreStringCellText::Flat(_) => false,
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
