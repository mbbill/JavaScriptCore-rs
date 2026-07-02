//! MarkedSpace: the Heap-owned cell arena (heap/MarkedSpace.h). This is the proven
//! prototype `Arena` (tools/s4_arena_proto/src/lib.rs:383-579) RENAMED to its JSC
//! concept, with the synthetic membership `HashMap` swapped for the PRODUCTION
//! O(1) validity gate — a faithful port of `HeapUtil::isPointerGCObjectJSCell`
//! (heap/HeapUtil.h:51-89). Truth lives in (a) a registry of owned page bases
//! (`MarkedBlockSet`) + the per-block alloc/mark bitmaps, and (b) the live precise
//! cell set — exactly JSC, no `CellId`, no side table.
//!
//! NOT WIRED: this module is pure additive dead code in R1. The engine never
//! instantiates `MarkedSpace`; the JsValue keeps carrying `CellId`; CoreObjectStore
//! and the live deref path are untouched. R2/R3/R4 wire it in.

#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicU8, Ordering};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::gc::FxIntBuildHasher;

use super::block_directory::BlockDirectory;
use super::free_list::SweepResult;
use super::marked_block::{
    block_atoms_per_cell, block_for, block_start_atom, cell_ptr, clear_marks_block, is_atom,
    is_atom_aligned, is_live_cell, is_marked as marked_block_is_marked, round_up,
    test_and_set_marked, Cell, CellPtr, ATOMS_PER_BLOCK, ATOM_SIZE, CELL_BYTES, END_ATOM,
    HALF_ALIGNMENT, MARK_WORDS, PAYLOAD_BYTES, PRECISE_CUTOFF, SIZE_STEP,
};
use super::precise_allocation::PreciseSpace;

// ===================== FAITHFUL-MATCH layout invariants =====================
// Any drift from the proven prototype core fails the build here (the prototype is
// the miri regression; the main-crate core must match it).
const _: () = assert!(MARK_WORDS == 16);
const _: () = assert!(ATOMS_PER_BLOCK == 1024);
const _: () = assert!(HALF_ALIGNMENT == 8);

// ===================== Size classes (heap/MarkedSpace.h:52-69; MarkedSpace.cpp:40-160) =====================

/// `Options::sizeClassProgression()` default (runtime/OptionsList.h:244): the
/// geometric growth factor (~1.4) for size classes above preciseCutoff.
const SIZE_CLASS_PROGRESSION: f64 = 1.4;
/// `Options::preciseAllocationCutoff()` default (runtime/OptionsList.h:245).
const PRECISE_ALLOCATION_CUTOFF: usize = 100_000;

/// blockPayload (heap/MarkedSpace.h:59 = MarkedBlock::payloadSize, MarkedBlock.h:325).
const BLOCK_PAYLOAD: usize = PAYLOAD_BYTES;
/// largeCutoff (heap/MarkedSpace.h:65): the largest cell we put in a MarkedBlock —
/// half the payload rounded down to a step, so >=2 cells fit per block.
const LARGE_CUTOFF: usize = (BLOCK_PAYLOAD / 2) & !(SIZE_STEP - 1);
/// numSizeClasses (heap/MarkedSpace.h:69), incl. the size-zero class.
const NUM_SIZE_CLASSES: usize = LARGE_CUTOFF / SIZE_STEP + 1;

/// sizeClassToIndex (heap/MarkedSpace.h:82-85).
#[inline]
fn size_class_to_index(size: usize) -> usize {
    (size + SIZE_STEP - 1) / SIZE_STEP
}

/// indexToSizeClass (heap/MarkedSpace.h:87-92).
#[inline]
fn index_to_size_class(index: usize) -> usize {
    index * SIZE_STEP
}

/// The built size-class table: the actual classes plus the O(1) step->class lookup
/// (`s_sizeClassForSizeStep`, heap/MarkedSpace.h:183).
struct SizeClassTable {
    size_classes: Vec<usize>,
    size_class_for_size_step: Vec<u32>,
}

static SIZE_CLASS_TABLE: OnceLock<SizeClassTable> = OnceLock::new();

fn size_class_table() -> &'static SizeClassTable {
    SIZE_CLASS_TABLE.get_or_init(|| {
        let size_classes = build_size_classes();
        let size_class_for_size_step = build_size_class_for_size_step(&size_classes);
        SizeClassTable {
            size_classes,
            size_class_for_size_step,
        }
    })
}

/// `add` validation helper for the size-class builder (MarkedSpace.cpp:49-57).
fn size_class_add(result: &mut Vec<usize>, size_class: usize) {
    let size_class = round_up(size_class, ATOM_SIZE);
    debug_assert!(size_class % SIZE_STEP == 0);
    if result.is_empty() {
        debug_assert_eq!(size_class, SIZE_STEP);
    }
    result.push(size_class);
}

/// `sizeClasses()` (heap/MarkedSpace.cpp:40-143): exact per-step classes up to
/// preciseCutoff, then a geometric progression snapped to minimize per-block tail
/// wastage, manually injecting 256, sorted+deduped.
fn build_size_classes() -> Vec<usize> {
    let mut result: Vec<usize> = Vec::new();

    // Precise per-step classes: 16, 32, 48, 64 (< preciseCutoff==80).
    let mut size = SIZE_STEP;
    while size < PRECISE_CUTOFF {
        size_class_add(&mut result, size);
        size += SIZE_STEP;
    }

    // Geometric classes in (preciseCutoff, largeCutoff], snapped to reduce wastage.
    let mut i: i32 = 0;
    loop {
        let approximate_size = PRECISE_CUTOFF as f64 * SIZE_CLASS_PROGRESSION.powi(i);
        i += 1; // mirrors the C++ for-loop `++i` (runs every iteration incl. `continue`).
        let approximate_size_in_bytes = approximate_size as usize;
        assert!(approximate_size_in_bytes >= PRECISE_CUTOFF);
        if approximate_size_in_bytes > LARGE_CUTOFF {
            break;
        }
        let size_class = round_up(approximate_size_in_bytes, SIZE_STEP);
        // Snap so there is no slop at the tail of the block's payload.
        let cells_per_block = BLOCK_PAYLOAD / size_class;
        let possibly_better = (BLOCK_PAYLOAD / cells_per_block) & !(SIZE_STEP - 1);
        let original_wastage = BLOCK_PAYLOAD - cells_per_block * size_class;
        let new_wastage = (possibly_better - size_class) * cells_per_block;
        let better = if new_wastage > original_wastage {
            size_class
        } else {
            possibly_better
        };
        if better == *result.last().unwrap() {
            continue; // defense for when expStep is small
        }
        if better > LARGE_CUTOFF || better > PRECISE_ALLOCATION_CUTOFF {
            break;
        }
        size_class_add(&mut result, better);
    }

    // Manually inject high-volume class (MarkedSpace.cpp:126).
    size_class_add(&mut result, 256);

    result.sort_unstable();
    result.dedup();

    // optimalSizeFor's assumption: the first classes are exactly the per-step set
    // (MarkedSpace.cpp:139-140).
    let mut expect = SIZE_STEP;
    let mut idx = 0;
    while expect <= PRECISE_CUTOFF {
        debug_assert_eq!(result[idx], expect);
        expect += SIZE_STEP;
        idx += 1;
    }
    result
}

/// buildSizeClassTable (heap/MarkedSpace.cpp:145-159): fill `s_sizeClassForSizeStep`
/// so any byte size in (preciseCutoff, largeCutoff] maps to its class in O(1).
fn build_size_class_for_size_step(size_classes: &[usize]) -> Vec<u32> {
    let mut table = vec![0u32; NUM_SIZE_CLASSES];
    let mut next_index = 0usize;
    for &size_class in size_classes {
        let index = size_class_to_index(size_class);
        for slot in table.iter_mut().take(index + 1).skip(next_index) {
            *slot = size_class as u32;
        }
        next_index = index + 1;
    }
    for (i, slot) in table
        .iter_mut()
        .enumerate()
        .take(NUM_SIZE_CLASSES)
        .skip(next_index)
    {
        *slot = index_to_size_class(i) as u32;
    }
    table
}

/// optimalSizeFor (heap/MarkedSpace.h:262-270): round up to the cell size for this
/// many bytes; returns `bytes` unchanged when above largeCutoff (the precise path).
fn optimal_size_for(bytes: usize) -> usize {
    assert!(bytes != 0);
    if bytes <= PRECISE_CUTOFF {
        return round_up(bytes, SIZE_STEP);
    }
    if bytes <= LARGE_CUTOFF {
        return size_class_table().size_class_for_size_step[size_class_to_index(bytes)] as usize;
    }
    bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SizeRoute {
    Marked(usize),
    Precise(usize),
}

/// Route a request: a MarkedBlock size class if it fits (<= largeCutoff), else a
/// PreciseAllocation (heap/MarkedSpace.h:262-270; CompleteSubspace allocation).
fn size_route(bytes: usize) -> SizeRoute {
    let sized = optimal_size_for(bytes);
    if bytes <= LARGE_CUTOFF {
        SizeRoute::Marked(sized)
    } else {
        SizeRoute::Precise(sized)
    }
}

// ===================== MarkedBlockSet (heap/MarkedBlockSet.h:36-49) =====================

/// TinyBloomFilter<uintptr_t> (wtf/TinyBloomFilter.h:32-86 / heap copy): a
/// false-positive-only fast negative over block base addresses. `ruleOut` NEVER
/// false-negatives (it only ORs bits in on `add`), so a live block is never
/// rejected — exactly the property HeapUtil relies on (HeapUtil.h:71-74).
#[derive(Clone, Copy, Default)]
struct TinyBloomFilter {
    bits: usize,
}

impl TinyBloomFilter {
    fn add(&mut self, bits: usize) {
        self.bits |= bits; // TinyBloomFilter::add (:57-60)
    }

    /// TinyBloomFilter::ruleOut (:68-78). True (rule out) for 0.
    fn rule_out(&self, bits: usize) -> bool {
        if bits == 0 {
            return true;
        }
        if (bits & self.bits) != bits {
            return true;
        }
        false
    }

    fn reset(&mut self) {
        self.bits = 0; // TinyBloomFilter::reset (:80-84)
    }
}

/// MarkedBlockSet (heap/MarkedBlockSet.h:36-49): the set of owned block base
/// addresses + the bloom filter, over `usize` page bases instead of `MarkedBlock*`.
struct MarkedBlockSet {
    set: HashSet<usize, FxIntBuildHasher>,
    filter: TinyBloomFilter,
}

impl MarkedBlockSet {
    fn new() -> Self {
        MarkedBlockSet {
            set: HashSet::default(),
            filter: TinyBloomFilter::default(),
        }
    }

    /// MarkedBlockSet::add (:51-55): register a new block base.
    fn add(&mut self, block: usize) {
        self.filter.add(block);
        self.set.insert(block);
    }

    /// MarkedBlockSet::remove (:57-63). JSC recomputes the filter only when set
    /// capacity shrinks a lot; we recompute on every removal. The filter is
    /// false-positive-only, so recompute can only tighten it — it can never reject
    /// a live block. Used by `MarkedSpace::shrink` (gc-r4 leak-fix C3): a decommitted
    /// block must drop out of every `self.blocks`-driven walk (`find`/`is_arena_cell`/
    /// `for_each_object_cell`/`clear_all_marks`) until it is reactivated and
    /// re-registered, or its header/payload could be read after its physical pages
    /// were returned to the OS.
    fn remove(&mut self, block: usize) {
        self.set.remove(&block);
        self.recompute_filter();
    }

    /// MarkedBlockSet::recomputeFilter (:65-71).
    fn recompute_filter(&mut self) {
        self.filter.reset();
        for &b in &self.set {
            self.filter.add(b);
        }
    }

    fn rule_out(&self, candidate: usize) -> bool {
        self.filter.rule_out(candidate)
    }

    fn contains(&self, candidate: usize) -> bool {
        self.set.contains(&candidate)
    }
}

// ===================== MarkedSpace (the cutover target) =====================

const BORROW_FREE: u8 = 0;
const BORROW_MUT: u8 = 1;

/// MarkedSpace (heap/MarkedSpace.h:49): per-size-class MarkedBlock directories + a
/// PreciseSpace, plus the production membership/validity gate. The proven cutover
/// target for interpreter/mod.rs CoreObjectStore + gc/heap.rs payload<->CellId maps.
pub(crate) struct MarkedSpace {
    /// One directory per size class actually used (keyed by cell_size_atoms).
    /// Mirrors MarkedSpace::m_directories (one BlockDirectory per class).
    directories: HashMap<usize, BlockDirectory>,
    precise: PreciseSpace,
    /// MarkedBlockSet of owned block bases (m_blocks, heap/MarkedSpace.h:224) — the
    /// PATH A membership half of the validity gate (HeapUtil.h:68-79).
    blocks: MarkedBlockSet,
    /// Live precise cell addresses (m_preciseAllocationSet, heap/MarkedSpace.h:163,
    /// 207) — the PATH B membership half of the gate (HeapUtil.h:54-65).
    precise_set: HashSet<usize, FxIntBuildHasher>,
    /// DEBUG-ONLY overlap detector (NOT the membership oracle). A per-cell borrow
    /// flag in a SEPARATE allocation (Box<AtomicU8>) so it can never retag the
    /// cell's UnsafeCell. Kept from the prototype but demoted: `find()` is the
    /// memory-safety gate; this only catches a careless overlapping `&mut`.
    debug_borrow_flags: HashMap<usize, Box<AtomicU8>, FxIntBuildHasher>,
    /// gc-r4 R3 (reversible shadow oracle): count of cells handed out via
    /// `allocate_blob`. The R3 shadow space only ever calls `allocate_blob` and R3
    /// never sweeps, so this is exactly its LIVE twin count — the population the
    /// suite-end cross-check compares against `CoreObjectStore::objects.len()`.
    allocated_blob_cells: usize,
    /// gc-r4 R4b live trigger — bytes handed out via `allocate_blob` SINCE the last
    /// collection (JSC `Heap::m_bytesAllocatedThisCycle`, heap/Heap.h:1006). The
    /// cooperative safepoint driver resets it to 0 at the END of each collection (a
    /// fresh cycle). NOT a leak counter (`allocated_blob_cells` is monotone); this
    /// drains every GC.
    bytes_allocated_this_cycle: usize,
    /// gc-r4 R4b live trigger — set true once `bytes_allocated_this_cycle` crosses
    /// `BYTES_ALLOCATED_GC_THRESHOLD`. The cooperative back-edge / VM-entry safepoint
    /// driver (`CoreObjectStore::poll_collection_at_safepoint`) checks it and runs ONE
    /// collection when armed. Decision 5: NEVER collect inline in `allocate_blob` —
    /// that would collect re-entrantly while a cell `&mut`/`&` is live.
    collection_request_armed: bool,
    /// gc-r4 leak-fix C3 GUARD (debug-only) — true from MARK-PHASE END
    /// (`note_mark_phase_end_for_shrink_guard`, called by `CoreObjectStore::
    /// force_collect` right after marking) until `shrink()` completes. THE INVARIANT
    /// IT PINS: no allocation occurs in that window within the single-STW
    /// collection, because `block_has_no_marks` — the emptiness test both
    /// `BlockDirectory::sweep_all_blocks`'s FreeList-exclusion pass and
    /// `BlockDirectory::shrink`'s decommit pass rest on — reads ONLY mark bits. An
    /// allocation in the window would set `newlyAllocated` but no mark, so its
    /// block could still classify "empty" and be decommitted under a LIVE cell
    /// (exactly the newlyAllocated race JSC's `isLive` handles via
    /// `m_newlyAllocatedVersion`, heap/MarkedBlockInlines.h:101-190 — machinery
    /// this single-STW port intentionally does not carry). Today the window's only
    /// occupants (`finalize_unconditional_finalizers`,
    /// `reconcile_dead_cells_before_sweep`) allocate nothing; a future
    /// finalizer-callback feature that allocates MUST trip the `allocate`/
    /// `allocate_blob` assertion, not silently reintroduce the race.
    #[cfg(debug_assertions)]
    no_alloc_until_shrink_completes: bool,
    _not_send_sync: PhantomData<*const ()>, // contract C6
}

/// gc-r4 R4b live trigger — bytes-allocated-since-last-GC threshold that arms a
/// deferred collection request (the JSC `m_bytesAllocatedThisCycle` vs eden-size
/// trigger, heap/Heap.cpp `Heap::collectIfNecessaryOrDefer`). A DELIBERATELY simple
/// single threshold (no eden-growth heuristic yet); a few MB mirrors JSC's small
/// initial eden. Under `cfg(test)` it is small so `cargo test --lib` — the live-driver
/// stress test — actually trips the trigger on modest allocation and exercises a real
/// collection at the back-edge safepoint, giving root-completeness coverage across the
/// suite's JS-running tests instead of never arming.
#[cfg(not(test))]
const BYTES_ALLOCATED_GC_THRESHOLD: usize = 4 * 1024 * 1024;
#[cfg(test)]
const BYTES_ALLOCATED_GC_THRESHOLD: usize = 16 * 1024;

/// gc-r4 leak-fix C1 — the `reportExtraMemoryAllocated`/`deprecatedReportExtraMemory` inline
/// gate: C++ JSC only bumps its counter and re-checks `collectIfNecessaryOrDefer` for a
/// report LARGER than `minExtraMemory` (heap/Heap.h:434-457, threshold at heap/Heap.h:681
/// `static constexpr size_t minExtraMemory = 256;`), so a flood of tiny reports (a short
/// string, a 4-element array) doesn't pay the bump/threshold-check cost every time.
const MIN_EXTRA_MEMORY_TO_REPORT: usize = 256;

impl Default for MarkedSpace {
    fn default() -> Self {
        Self::new()
    }
}

// gc-r4 R4a: `MarkedSpace` is the RELEASE object-cell arena owned by the interpreter's
// `CoreObjectStore` (which `#[derive(Debug)]`s, transitively required by the dispatch
// host's Debug). The space holds raw exposed pages + bitmaps with no meaningful Debug;
// surface only the population summary (mirrors the retired R3 ShadowOracle Debug).
impl std::fmt::Debug for MarkedSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarkedSpace")
            .field("directories", &self.directories.len())
            .field("blocks", &self.blocks.set.len())
            .field("precise_cells", &self.precise_set.len())
            .field("allocated_blob_cells", &self.allocated_blob_cells)
            .finish_non_exhaustive()
    }
}

impl MarkedSpace {
    pub(crate) fn new() -> Self {
        MarkedSpace {
            directories: HashMap::new(),
            precise: PreciseSpace::new(),
            blocks: MarkedBlockSet::new(),
            precise_set: HashSet::default(),
            debug_borrow_flags: HashMap::default(),
            allocated_blob_cells: 0,
            bytes_allocated_this_cycle: 0,
            collection_request_armed: false,
            #[cfg(debug_assertions)]
            no_alloc_until_shrink_completes: false,
            _not_send_sync: PhantomData,
        }
    }

    /// gc-r4 leak-fix C3 GUARD — record that the mark phase has ended and the mark
    /// bits are now the AUTHORITATIVE liveness `block_has_no_marks` classifies
    /// against, opening the no-allocation window that closes when `shrink()`
    /// completes (see the `no_alloc_until_shrink_completes` field doc for the race
    /// this pins). Called by `CoreObjectStore::force_collect` immediately after
    /// marking; debug-only effect (release compiles to nothing).
    pub(crate) fn note_mark_phase_end_for_shrink_guard(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.no_alloc_until_shrink_completes = true;
        }
    }

    /// Route a size to a directory/precise space (optimalSizeFor, MarkedSpace.h
    /// :262-270) and allocate one cell. For a MarkedBlock size class the
    /// BlockDirectory hands the cell out through the LocalAllocator FreeList
    /// interval fast path (heap/LocalAllocatorInlines.h:33-43; heap/FreeList.h
    /// :82-123) — no longer a raw atom bump. The address is exposed ONCE and
    /// membership registered. Returns the `CellPtr` the JsValue carries.
    pub(crate) fn allocate(&mut self, js_type: u8, payload: u64) -> CellPtr {
        // gc-r4 leak-fix C3 GUARD: no allocation between mark-phase end and
        // shrink() completing (see the `no_alloc_until_shrink_completes` field doc).
        #[cfg(debug_assertions)]
        assert!(
            !self.no_alloc_until_shrink_completes,
            "allocation between mark-phase end and shrink() — a fresh cell's block \
             could classify empty (marks-only) and be decommitted under it"
        );
        match size_route(CELL_BYTES) {
            SizeRoute::Marked(sz) => {
                let atoms = sz / ATOM_SIZE;
                let dir = self
                    .directories
                    .entry(atoms)
                    .or_insert_with(|| BlockDirectory::new(atoms));
                let (cp, new_base) = dir.allocate(Cell::new(js_type, payload));
                if let Some(base) = new_base {
                    // MarkedSpace::didAddBlock -> m_blocks.add (MarkedBlockSet.h:51-55).
                    self.blocks.add(base);
                }
                self.record_debug_flag(cp);
                cp
            }
            SizeRoute::Precise(sz) => {
                let cp = self.precise.allocate(sz, Cell::new(js_type, payload));
                // MarkedSpace::registerPreciseAllocation -> m_preciseAllocationSet
                // (heap/MarkedSpace.cpp:239,293).
                self.precise_set.insert(cp.addr());
                self.record_debug_flag(cp);
                cp
            }
        }
    }

    // ===================== gc-r4 R4a: POD CELL BLOB ALLOCATION =====================
    // `allocate_blob` accepts a real POD CELL BLOB (`CoreObjectCell`) into the arena via
    // the SAME routing / BlockDirectory / FreeList path `allocate` uses. gc-r4 R4a (the
    // flip) PROMOTED this from R3's debug-only byte-twin to THE object-cell allocation:
    // `CoreObjectStore::allocate_cell` calls it and the returned address IS the cell's
    // identity (the former `Vec<Pin<Box<CoreObjectCell>>>` box path is DELETED). The arena
    // still does NOT sweep (R4b / GAP B), so cells accumulate. `allocated_blob_cell_count`
    // is the monotone live-cell count (the post-flip analog of the old `objects.len()`).
    // NOTE: `shadow_write`/`shadow_bytes_eq` below are R3 twin-resync VESTIGES with no
    // remaining caller (there is no twin post-flip) — safe to prune in a follow-up.

    /// Route a POD cell blob by its byte size and store it via the production allocate
    /// path (a twin of the authoritative box cell). Returns the carried `CellPtr`.
    ///
    /// SAFETY: `src..src+len` is `len` readable bytes of an initialized POD value
    /// (`needs_drop == false`); single mutator thread (contract C5/C6).
    pub(crate) unsafe fn allocate_blob(&mut self, src: *const u8, len: usize) -> CellPtr {
        // gc-r4 leak-fix C3 GUARD: no allocation between mark-phase end and
        // shrink() completing (see the `no_alloc_until_shrink_completes` field doc).
        // A future finalizer-callback feature that allocates during the end phase
        // must trip this assertion, not silently reintroduce the newlyAllocated race.
        #[cfg(debug_assertions)]
        assert!(
            !self.no_alloc_until_shrink_completes,
            "allocation between mark-phase end and shrink() — a fresh cell's block \
             could classify empty (marks-only) and be decommitted under it"
        );
        self.allocated_blob_cells += 1;
        // gc-r4 R4b live trigger (decision 5): ACCUMULATE bytes-this-cycle and ARM a
        // DEFERRED collection request when it crosses the threshold. Do NOT collect
        // here — collection runs ONLY at the cooperative back-edge / VM-entry
        // safepoint, so no collection can fire while a cell `&mut`/`&` is live in an
        // `allocate_*` / `with_cell_mut` window (forecloses re-entrancy by
        // construction). Mirrors JSC bumping `m_bytesAllocatedThisCycle` in the
        // allocation slow path and deferring the actual collect to a safepoint.
        self.bump_bytes_allocated_this_cycle(len);
        match size_route(len) {
            SizeRoute::Marked(sz) => {
                let atoms = sz / ATOM_SIZE;
                let dir = self
                    .directories
                    .entry(atoms)
                    .or_insert_with(|| BlockDirectory::new(atoms));
                // SAFETY: forwarded — see the fn contract.
                let (cp, new_base) = unsafe { dir.allocate_blob(src, len) };
                if let Some(base) = new_base {
                    self.blocks.add(base); // didAddBlock -> m_blocks.add
                }
                cp
            }
            SizeRoute::Precise(sz) => {
                // SAFETY: forwarded — see the fn contract.
                let cp = unsafe { self.precise.allocate_blob(sz, src, len) };
                self.precise_set.insert(cp.addr());
                cp
            }
        }
    }

    /// Re-sync the arena twin at `cp` from the authoritative box cell (`src..src+len`)
    /// in lockstep (gc-r4 R3: the box stays authoritative; the twin tracks it). `&self`
    /// because the cell slot is interior-mutable once-exposed page memory — no `&mut
    /// MarkedSpace`, no `&MarkedBlock`, only a raw place copy (contract C4/C5).
    ///
    /// SAFETY: `cp` is a live cell this space handed out via `allocate_blob`;
    /// `src..src+len` is `len` readable POD bytes; single mutator thread.
    pub(crate) unsafe fn shadow_write(&self, cp: CellPtr, src: *const u8, len: usize) {
        let dst = ptr::with_exposed_provenance_mut::<u8>(cp.addr());
        // SAFETY (C2,C3,C4): `cp.addr()` is a live cell inside a once-exposed page; the
        // raw byte copy forms no reference; the box cell and the twin are distinct
        // allocations (non-overlapping).
        unsafe { ptr::copy_nonoverlapping(src, dst, len) };
    }

    /// Prove the arena twin at `cp` is BYTE-EQUAL to the box cell (`src..src+len`) — the
    /// R4-readiness assert that the arena holds the cell byte-identically. Reads the twin
    /// back through a FRESH provenance recovery (so a slot-overlap / block-corruption bug
    /// surfaces) and compares every byte.
    ///
    /// The comparison spans the FULL struct width including any `#[repr(C)]` padding.
    /// That is sound under the R3 gate (native `cargo test`, no miri): the twin is a
    /// verbatim `copy_nonoverlapping` of the box, so padding bytes match bit-for-bit;
    /// only a real arena corruption makes them differ. (The R4 gate separately runs miri
    /// on the live raw-arena deref — gc-r4.md R4 technical gate (b).)
    ///
    /// SAFETY: `cp` is a live cell handed out via `allocate_blob`; `src..src+len` is
    /// `len` readable bytes; single mutator thread; raw place reads form no reference.
    pub(crate) unsafe fn shadow_bytes_eq(&self, cp: CellPtr, src: *const u8, len: usize) -> bool {
        let twin = ptr::with_exposed_provenance::<u8>(cp.addr());
        for i in 0..len {
            // SAFETY: both `twin+i` and `src+i` are in-bounds readable bytes (twin is a
            // live `len`-byte arena cell, src a `len`-byte POD value); raw reads only.
            let (a, b) = unsafe { (ptr::read(twin.add(i)), ptr::read(src.add(i))) };
            if a != b {
                return false;
            }
        }
        true
    }

    /// gc-r4 R3 population cross-check input: how many cells this space handed out via
    /// `allocate_blob` (== live twins, since R3 never sweeps).
    pub(crate) fn allocated_blob_cell_count(&self) -> usize {
        self.allocated_blob_cells
    }

    /// gc-r4 R4b live trigger — is a deferred collection ARMED (the byte counter
    /// crossed `BYTES_ALLOCATED_GC_THRESHOLD` since the last GC)? The cooperative
    /// safepoint driver polls this; a disarmed back-edge pays one bool load.
    pub(crate) fn collection_request_armed(&self) -> bool {
        self.collection_request_armed
    }

    /// gc-r4 R4b live trigger — bytes allocated since the last collection (test probe
    /// + JSC `m_bytesAllocatedThisCycle` mirror).
    pub(crate) fn bytes_allocated_this_cycle(&self) -> usize {
        self.bytes_allocated_this_cycle
    }

    /// gc-r4 R4b live trigger — start a fresh allocation cycle: clear the byte counter
    /// and DISARM the request. The driver calls this at the END of a collection (JSC
    /// resets `m_bytesAllocatedThisCycle` after a collection).
    pub(crate) fn reset_bytes_allocated_this_cycle(&mut self) {
        self.bytes_allocated_this_cycle = 0;
        self.collection_request_armed = false;
    }

    /// Shared bump-and-arm step for `bytes_allocated_this_cycle`, the JSC `Heap::didAllocate`
    /// analog (heap/Heap.cpp:2639-2648): the ONE funnel a cell's own arena bytes
    /// (`allocate_blob`) and an off-arena extra-memory report (`report_extra_memory_allocated`
    /// below) both bump before `collectIfNecessaryOrDefer` reads the total
    /// (`totalBytesAllocatedThisCycle()`, heap/Heap.h:697, read at heap/Heap.cpp:2887/2901).
    /// MONOTONIC within a cycle by construction (`saturating_add`, never subtracted) — see
    /// `report_extra_memory_allocated`'s doc for the C++ citation proving this is faithful.
    fn bump_bytes_allocated_this_cycle(&mut self, bytes: usize) {
        self.bytes_allocated_this_cycle = self.bytes_allocated_this_cycle.saturating_add(bytes);
        if self.bytes_allocated_this_cycle >= BYTES_ALLOCATED_GC_THRESHOLD {
            self.collection_request_armed = true;
        }
    }

    /// gc-r4 leak-fix C1 — the accounting entry point for OFF-ARENA payload bytes an arena
    /// cell owns: a butterfly (out-of-line property/element storage), an ArrayBuffer/
    /// BoundArgs/Captures backing, a String's StringImpl text, or a BigInt's digit limbs.
    /// Named for, and funneling into the SAME `bytes_allocated_this_cycle` trigger as, C++
    /// JSC `Heap::reportExtraMemoryAllocated`/`Heap::deprecatedReportExtraMemory`
    /// (heap/Heap.h:434-457), which both bottom out in `Heap::didAllocate`
    /// (heap/Heap.cpp:689-696 `reportExtraMemoryAllocatedSlowCase`, :699-709
    /// `deprecatedReportExtraMemorySlowCase`, :2639-2648 `didAllocate`) and then call
    /// `collectIfNecessaryOrDefer` (heap/Heap.cpp:2852-2918), exactly like `allocate_blob`
    /// does here. Mirrors the `size > minExtraMemory` gate every C++ call site inlines
    /// (heap/Heap.h:434-457) via `MIN_EXTRA_MEMORY_TO_REPORT`.
    ///
    /// MONOTONIC / no decrement on free: C++ `m_extraMemorySize`/`m_deprecatedExtraMemorySize`
    /// (heap/Heap.h:879-880) are only ever `+=`'d (`reportExtraMemoryVisited`,
    /// heap/Heap.cpp:2821-2833; `deprecatedReportExtraMemorySlowCase`, :699-709) and are reset
    /// to 0 ONLY at a collection boundary (heap/Heap.cpp:2419-2420, inside `updateAllocationLimits`
    /// after a full collection) — never decremented when the owning object dies mid-cycle. This
    /// port matches that: `reset_bytes_allocated_this_cycle` (the collection-boundary reset) is
    /// the ONLY place `bytes_allocated_this_cycle` goes down; the sweep-time slab frees
    /// (`CoreObjectStore::free_dead_cell_slabs`) do NOT call back into this counter.
    ///
    /// DIVERGENCE: in C++ most of these payload KINDS are NOT "extra memory" at all — a
    /// Butterfly is allocated from `Heap::auxiliarySpace` (heap/Heap.h:1109, a first-class GC
    /// `CompleteSubspace`) and a JSBigInt's digits are allocated AS PART of the cell's own
    /// variable-sized `tryAllocateCell` (runtime/JSBigInt.cpp:109, `JSBigInt::allocationSize`,
    /// runtime/JSBigInt.h:70-72) — both are counted for free by the SAME `didAllocate` the
    /// block/large allocator already calls for any GC cell (heap/Heap.cpp:2508-2513), never
    /// through `reportExtraMemoryAllocated`. This port's arena admits only FIXED-SIZE POD cell
    /// blobs (`allocate_blob`) and relocates every variable payload OFF-CELL into a store-owned
    /// slab (gc-r4 SD-4: `object_store.rs` butterflies/bound_args_backings/captures_backings/
    /// array_buffer_backings, `bigint_store.rs` digit limbs), so those bytes are invisible to
    /// `allocate_blob`'s `len`. Funneling them through this API is the faithful-EFFECT
    /// substitute (same counter, same `collectIfNecessaryOrDefer`-style trigger) for a payload
    /// shape C++ never treats as "extra" — commented at each call site. Only a `String`'s
    /// StringImpl text is a LITERAL 1:1 match: WTF `StringImpl` is a genuine separate
    /// (fastMalloc) heap allocation in C++ too (`JSString::finishCreation`, runtime/
    /// JSString.h:181, `vm.heap.reportExtraMemoryAllocated(this, cost)`).
    pub(crate) fn report_extra_memory_allocated(&mut self, bytes: usize) {
        if bytes <= MIN_EXTRA_MEMORY_TO_REPORT {
            return;
        }
        self.bump_bytes_allocated_this_cycle(bytes);
    }

    /// Force-route through PreciseAllocation regardless of size (to exercise the
    /// +8 dispatch path in tests).
    pub(crate) fn allocate_precise(&mut self, js_type: u8, payload: u64) -> CellPtr {
        let cp = self
            .precise
            .allocate(CELL_BYTES, Cell::new(js_type, payload));
        self.precise_set.insert(cp.addr());
        self.record_debug_flag(cp);
        cp
    }

    fn record_debug_flag(&mut self, cp: CellPtr) {
        self.debug_borrow_flags
            .insert(cp.addr(), Box::new(AtomicU8::new(BORROW_FREE)));
    }

    /// Dispatch helper: which space owns this carried address
    /// (PreciseAllocation::isPreciseAllocation, heap/PreciseAllocation.h:68-71).
    /// MarkedBlock cells are 16-aligned (bit 3 clear); precise cells carry the +8
    /// bit. Deref MUST dispatch on this BEFORE any field load.
    #[inline]
    pub(crate) fn is_precise(addr: usize) -> bool {
        addr & HALF_ALIGNMENT != 0
    }

    /// PRODUCTION VALIDITY GATE — faithful port of
    /// `HeapUtil::isPointerGCObjectJSCell` (heap/HeapUtil.h:51-89). Called with an
    /// ARBITRARY carried address; returns a `CellPtr` only if it is a live arena
    /// cell, so deref never touches a non-arena / dangling / interior pointer. This
    /// REPLACES the prototype's synthetic `HashMap` membership set with the JSC
    /// registry + bitmap truth (no `CellId`, no side table).
    pub(crate) fn find(&self, addr: usize) -> Option<CellPtr> {
        // Dispatch on the halfAlignment bit FIRST (HeapUtil.h:54; PreciseAllocation.h:68-71).
        if Self::is_precise(addr) {
            // PATH B — PreciseAllocation cell: live precise set membership
            // (HeapUtil.h:54-65; m_preciseAllocationSet).
            if self.precise_set.contains(&addr) {
                return Some(CellPtr::from_addr(addr));
            }
            return None;
        }

        // PATH A — MarkedBlock cell.
        let candidate = block_for(addr); // MarkedBlock::blockFor (MarkedBlock.h:489-492)
        if self.blocks.rule_out(candidate) {
            // TinyBloomFilter fast negative (HeapUtil.h:71). Ruled out -> not a cell.
            return None;
        }
        if !is_atom_aligned(addr) {
            // MarkedBlock::isAtomAligned (HeapUtil.h:76; MarkedBlock.h:474-476).
            return None;
        }
        if !self.blocks.contains(candidate) {
            // Registered-block membership (HeapUtil.h:79; MarkedBlockSet set).
            return None;
        }
        // cellKind(candidate) == HeapCell::JSCell (HeapUtil.h:82): R1 only allocates
        // JSCell-kind blocks (Auxiliary subspaces are deferred to R2), so this is
        // trivially satisfied for every registered block. R2 records the kind.
        if !is_live_cell(addr) {
            // MarkedBlock::Handle::isLiveCell (HeapUtil.h:85): isAtom + alloc/mark
            // bit. Only reached AFTER set.contains proved `candidate` is a real,
            // once-exposed page, so recovering its header is sound (contract C3).
            return None;
        }
        Some(CellPtr::from_addr(addr))
    }

    /// MEMBERSHIP-ONLY admission gate for the COLLECTOR (gc-r4 R4b-mark). This is
    /// `find` MINUS its final `is_live_cell` check — block bloom `rule_out` →
    /// `is_atom_aligned` → `blocks.contains` (PATH A), or live `precise_set`
    /// membership (PATH B). It admits an address iff it lies on an arena object
    /// cell, REGARDLESS of that cell's mark / newlyAllocated liveness bits.
    ///
    /// THE #1 R4b UAF LANDMINE — why the marker MUST gate through THIS, never the
    /// liveness-bearing `find`: `find` ends in `is_live_cell` = `newlyAllocated OR
    /// marked`. `clear_all_marks` zeroes every mark at begin-marking, and a prior
    /// collection's SURVIVOR already had its `newlyAllocated` bit cleared by that
    /// collection's sweep (R4b-sweep, `DoesNotHave` mode). So at the start of the
    /// SECOND+ collection an old-gen survivor has `newlyAllocated == 0 AND marks ==
    /// 0` → `is_live_cell == false` → `find` returns `None`. A marker that gated its
    /// roots/edges through `find` would REJECT every still-live old-gen cell, leave
    /// it unmarked, and the sweep would reclaim the WHOLE live old generation → mass
    /// use-after-free. The membership test does not consult liveness, so the
    /// survivor is admitted, marked, and retained. (It detonates only on collection
    /// #2 — survivors keep `newlyAllocated` through #1 — so a single-collection test
    /// cannot catch it; the reclaim test runs ≥2 collections.)
    ///
    /// A foreign LEAF cell (String/Symbol/BigInt `Box`) lies in no arena block, so
    /// PATH A's `rule_out`/`contains` reject it (and it never carries the +8 precise
    /// bit / sits in `precise_set`) → `None`, exactly as `find` rejects it: the
    /// marker never dereferences a non-object cell as a `CoreObjectCell`.
    ///
    /// (DIVERGENCE from `find`: this drops the `is_atom` cell-START check that lived
    /// inside `is_live_cell`. Sound under the PRECISE cooperative root model — every
    /// admitted root/edge is a `RuntimeValue::from_cell` address, i.e. the exact
    /// cell-start `allocate_blob` returned, never an interior pointer. A future
    /// CONSERVATIVE stack scan (gc-r4 GAP C) that admits interior pointers MUST
    /// re-add an `is_atom` cell-start check here before dereferencing.)
    pub(crate) fn is_arena_cell(&self, addr: usize) -> Option<CellPtr> {
        // Dispatch on the halfAlignment bit FIRST (PreciseAllocation.h:68-71). The
        // precise PATH B is membership-only already (`find` uses no `is_live_cell`
        // there), so it is identical for the collector.
        if Self::is_precise(addr) {
            if self.precise_set.contains(&addr) {
                return Some(CellPtr::from_addr(addr));
            }
            return None;
        }
        let candidate = block_for(addr);
        if self.blocks.rule_out(candidate) {
            return None;
        }
        if !is_atom_aligned(addr) {
            return None;
        }
        if !self.blocks.contains(candidate) {
            return None;
        }
        // NO is_live_cell — see the UAF landmine note above.
        Some(CellPtr::from_addr(addr))
    }

    /// MEMBERSHIP + CELL-START admission gate for the CONSERVATIVE native-stack
    /// scan (gc-r4 GAP C — the A1.5 baseline-JIT-frame scan). This is
    /// [`Self::is_arena_cell`] (membership, NOT liveness) PLUS the
    /// `MarkedBlock::isAtom` cell-START check (`marked_block::is_atom`,
    /// heap/MarkedBlock.h:664-676) that `is_arena_cell` deliberately DROPS for the
    /// precise root path.
    ///
    /// WHY the extra check (the [`Self::is_arena_cell`] note demands it for a
    /// conservative scanner): an object cell spans `ATOMS_PER_CELL` (=5) atoms, so
    /// an atom-aligned address can land on an INTERIOR atom of a multi-atom cell.
    /// `is_arena_cell` admits any atom-aligned in-block address as a cell start; a
    /// scan that feeds a raw stack word through it could admit an interior atom and
    /// the marker would deref non-cell-start bytes as a header. `isAtom` rejects
    /// cell-middle pointers (`(atom - startAtom) % atomsPerCell == 0`), so only a
    /// true cell start is admitted before any deref. Faithful to
    /// `ConservativeRoots::genericAddPointer`, which resolves through
    /// `cellAlign`/`isLiveCell` (cell-start aware) before marking
    /// (heap/ConservativeRoots.cpp:167-177).
    ///
    /// MEMBERSHIP-only, never liveness — same #1 UAF landmine as `is_arena_cell`.
    /// Conservative over-approximation (a non-cell word that happens to hit a live
    /// cell start) is SAFE: it can only RETAIN, never free a live cell.
    pub(crate) fn is_arena_cell_start(&self, addr: usize) -> Option<CellPtr> {
        let cp = self.is_arena_cell(addr)?;
        // A precise allocation is a single cell; precise_set membership already
        // implies `addr` is its exact start (no interior atoms to reject).
        if Self::is_precise(addr) {
            return Some(cp);
        }
        // Block path: re-add the dropped cell-START check (`is_arena_cell` proved
        // the block is registered, so the `isAtom` header deref is sound) before
        // the marker may read this address as a `CoreObjectCell` header.
        if !is_atom(addr) {
            return None;
        }
        Some(cp)
    }

    /// COLLECTOR begin-marking: clear EVERY cell's mark bit so the upcoming mark
    /// phase computes a fresh live set (gc-r4 R4b-mark; the MarkedSpace half of
    /// `Heap::beginMarking`, heap/MarkedSpace::beginMarking). Walks every registered
    /// MarkedBlock base (`m_blocks`) zeroing its `m_marks` words, and every live
    /// precise allocation clearing its single mark byte. `&self` (not `&mut`): the
    /// mark words are atomic interior-mutable state reached through each page's
    /// once-exposed provenance (contract C3/C4), exactly like `collector_mark`; the
    /// block/precise registries are only read. JSC's `markingVersion` bump is
    /// deferred (same HeapVersion DIVERGENCE as `is_live_cell`); the direct zero has
    /// the identical observable result.
    pub(crate) fn clear_all_marks(&self) {
        for &base in &self.blocks.set {
            clear_marks_block(base);
        }
        for &addr in &self.precise_set {
            PreciseSpace::clear_mark(addr);
        }
    }

    /// TEST-ONLY: simulate a prior FULL-collection sweep's `newlyAllocated` reset
    /// (heap/MarkedBlock::resetAllocated; R4b-sweep `DoesNotHave` mode) so every surviving
    /// cell enters the NEXT begin-marking in the OLD-GEN state (`newlyAllocated == 0`).
    /// R4b-mark performs no real sweep yet (that is R4b-sweep), so a mark-only test uses
    /// this to reproduce the 2nd-collection survivor state the MEMBERSHIP gate must
    /// admit — the #1 UAF landmine, which a single-collection run cannot exhibit.
    #[cfg(test)]
    pub(crate) fn simulate_post_sweep_clear_newly_allocated(&self) {
        for &base in &self.blocks.set {
            super::marked_block::clear_newly_allocated_block(base);
        }
    }

    // ---- MUTATOR (contract C5: &mut only between safepoints, minimal scope) ----

    /// MUTATOR write of field0. Forms `&mut Cell` in MINIMAL scope, between
    /// safepoints, dropped before any other access path runs.
    pub(crate) fn mutator_write_field0(&self, cp: CellPtr, v: u64) {
        let _g = self.borrow_mut_guard(cp);
        let p = cell_ptr(cp.addr());
        // SAFETY (C3,C4,C5): cp validated live; arena is the sole owner/access path
        // (UnsafeCell, no Box/Unique alias); &mut scope is minimal and exclusive;
        // collector is stopped; no two overlapping &mut to one cell.
        unsafe {
            let cell = &mut *p;
            cell.field0 = v;
        } // &mut dropped here
    }

    pub(crate) fn mutator_write_structure_id(&self, cp: CellPtr, sid: u32) {
        let _g = self.borrow_mut_guard(cp);
        let p = cell_ptr(cp.addr());
        // SAFETY: as mutator_write_field0.
        unsafe {
            let cell = &mut *p;
            cell.header.structure_id = sid;
        }
    }

    /// JIT-style field read via `addr_of!` -> read(): forms NO reference, the
    /// narrowest possible miri footprint (contract C5). This is the shape the
    /// optimizing JIT emits: `load [base + disp]`.
    pub(crate) fn raw_read_field0(&self, cp: CellPtr) -> u64 {
        let p = cell_ptr(cp.addr());
        // SAFETY (C3,C4): live cell; raw place read forms no reference.
        unsafe { ptr::addr_of!((*p).field0).read() }
    }

    /// Read the JSType byte (offset 5) the JIT/ICs gate on, via addr_of (no ref).
    pub(crate) fn raw_read_js_type(&self, cp: CellPtr) -> u8 {
        let p = cell_ptr(cp.addr());
        // SAFETY (C3,C4): live cell; raw place read forms no reference.
        unsafe { ptr::addr_of!((*p).header.js_type).read() }
    }

    pub(crate) fn raw_read_structure_id(&self, cp: CellPtr) -> u32 {
        let p = cell_ptr(cp.addr());
        // SAFETY: as above.
        unsafe { ptr::addr_of!((*p).header.structure_id).read() }
    }

    // ---- COLLECTOR (contract C5: & only at stop-the-world) ----

    /// COLLECTOR read at stop-the-world: forms `&Cell` (no concurrent mutator &mut).
    pub(crate) fn collector_read(&self, cp: CellPtr) -> (u32, u8, u64) {
        let p = cell_ptr(cp.addr());
        // SAFETY (C3,C4,C5): stop-the-world; no &mut alive; cp is a live cell.
        unsafe {
            let cell = &*p;
            (cell.header.structure_id, cell.header.js_type, cell.field0)
        }
    }

    /// COLLECTOR mark: dispatch precise vs marked on the +8 bit, then flip the
    /// per-block atomic mark word via `blockFor` masking
    /// (MarkedBlock::testAndSetMarked, MarkedBlock.h:489-492,586-592,633-637).
    /// Returns true if this call set the bit.
    pub(crate) fn collector_mark(&self, cp: CellPtr) -> bool {
        let addr = cp.addr();
        if Self::is_precise(addr) {
            return PreciseSpace::mark(addr);
        }
        test_and_set_marked(addr)
    }

    pub(crate) fn collector_is_marked(&self, cp: CellPtr) -> bool {
        let addr = cp.addr();
        if Self::is_precise(addr) {
            return PreciseSpace::is_marked(addr);
        }
        marked_block_is_marked(addr)
    }

    /// gc-r4 R4b-sweep — is `addr` a MARKED arena object cell? The collector's post-mark
    /// liveness query the store's pre-sweep reconciliation uses to retain live cells: the
    /// MEMBERSHIP gate (`is_arena_cell`, NOT the liveness `find`) then the mark bit. Returns
    /// `false` for a non-arena address or for an unmarked (dead-this-cycle) cell. The
    /// membership gate, not liveness, is load-bearing (see `is_arena_cell`'s UAF note).
    pub(crate) fn is_addr_marked(&self, addr: usize) -> bool {
        match self.is_arena_cell(addr) {
            Some(cp) => self.collector_is_marked(cp),
            None => false,
        }
    }

    /// gc-r4 R4b-sweep — visit EVERY object-cell slot of every registered MarkedBlock,
    /// yielding each cell-START address (the per-block stride the sweep walks:
    /// `[start_atom, END_ATOM)` step `atoms_per_cell`). This is R4b decision 3: the block
    /// walk the STORE drives its pre-sweep reconciliation over (gc/heap stays ignorant of
    /// `CoreObjectCell`).
    ///
    /// YIELDS EVERY SLOT — live, dead-this-cycle, AND never-allocated/free. A never-
    /// allocated slot is zeroed (fresh `alloc_zeroed` page); a swept slot holds a `FreeCell`
    /// link record (its bytes 8..16 clobbered). NEITHER is a valid `CoreObjectCell`, so the
    /// caller MUST decide which yielded slots are real, byte-intact cells (the store's
    /// authoritative live-address set) BEFORE dereferencing one. This walk only ENUMERATES
    /// addresses by arithmetic; it never dereferences a slot.
    ///
    /// Only MarkedBlocks are walked: object cells are < `largeCutoff` so none is a precise
    /// allocation (a precise object cell would need a separate walk; none is produced).
    pub(crate) fn for_each_object_cell(&self, mut f: impl FnMut(usize)) {
        for &base in &self.blocks.set {
            let start_atom = block_start_atom(base);
            let atoms_per_cell = block_atoms_per_cell(base);
            let mut atom = start_atom;
            while atom < END_ATOM {
                f(base + atom * ATOM_SIZE);
                atom += atoms_per_cell;
            }
        }
    }

    /// gc-r4 R4b-sweep — sweep EVERY object-cell directory: reclaim each unmarked cell's
    /// atoms into that directory's combined FreeList (DoesNotHave full-collection mode) so
    /// re-allocation reuses them. MUST run AFTER the store's pre-sweep reconciliation, which
    /// reads dead cells' out-of-line handles BEFORE this clobbers them with FreeCell records
    /// (the gc-r4 R4b ordering invariant). Returns the aggregate `SweepResult`.
    pub(crate) fn sweep_all_object_blocks(&mut self) -> SweepResult {
        let mut total = SweepResult::default();
        for dir in self.directories.values_mut() {
            let r = dir.sweep_all_blocks();
            total.freed_cells += r.freed_cells;
            total.freed_bytes += r.freed_bytes;
            total.retained_cells += r.retained_cells;
        }
        total
    }

    /// `MarkedSpace::shrink()` (heap/MarkedSpace.cpp:408-415): return every
    /// COMPLETELY EMPTY block's physical pages to the OS across every directory
    /// (gc-r4 leak-fix C3). CALLER CONTRACT: run immediately after
    /// `sweep_all_object_blocks()` in the SAME collection pass — mirrors
    /// `Heap::sweepSynchronously`'s `m_objectSpace.sweepBlocks();
    /// m_objectSpace.shrink();` adjacency (heap/Heap.cpp:1250-1251); see
    /// `BlockDirectory::shrink`'s doc for why this port's combined-FreeList
    /// DIVERGENCE makes that adjacency load-bearing, not merely conventional.
    pub(crate) fn shrink(&mut self) {
        for dir in self.directories.values_mut() {
            for base in dir.shrink() {
                // MarkedSpace::freeBlock's `m_blocks.remove` (heap/MarkedSpace.cpp
                // :401-406), adapted for decommit-not-free — see `MarkedBlockSet::
                // remove`'s doc.
                self.blocks.remove(base);
            }
        }
        // gc-r4 leak-fix C3 GUARD: shrink complete — the marks-only emptiness
        // classification is spent; allocation is safe again (see the
        // `no_alloc_until_shrink_completes` field doc).
        #[cfg(debug_assertions)]
        {
            self.no_alloc_until_shrink_completes = false;
        }
    }

    /// gc-r4 leak-fix C3 — bytes of arena block pages currently physically
    /// committed, summed across every directory. See `BlockDirectory::
    /// committed_block_bytes` for the JSC-analog note (no direct counterpart by this
    /// name; substitutes for reading OS `phys_footprint` in-process).
    ///
    /// TRUST BOUNDARY: this is BOOKKEEPING (the per-directory `decommitted` flags),
    /// not an OS probe. It matches OS reality only while
    /// [`Self::decommit_failure_count`] stays 0 — a persistently failing `madvise`
    /// leaves pages resident that this still counts as decommitted. Any
    /// leak/footprint claim built on this metric must check that counter first.
    pub(crate) fn committed_block_bytes(&self) -> usize {
        self.directories
            .values()
            .map(|d| d.committed_block_bytes())
            .sum()
    }

    /// gc-r4 leak-fix C3 — release-visible count of persistent (post-EAGAIN-retry)
    /// `madvise` decommit failures: the divergence bound on
    /// [`Self::committed_block_bytes`] (see its TRUST BOUNDARY note). Delegates to
    /// the module-private `page_decommit` counter (a process-wide static — every
    /// space shares it, exact today with one live `MarkedSpace` per store).
    pub(crate) fn decommit_failure_count(&self) -> usize {
        super::page_decommit::decommit_failure_count()
    }

    /// See `BlockDirectory::reactivated_block_count`'s doc — summed across
    /// directories, the leak-fix C3 test-visible proof that a `shrink()`-decommitted
    /// block was reactivated rather than the allocator paging in a fresh one.
    pub(crate) fn reactivated_block_count(&self) -> usize {
        self.directories
            .values()
            .map(|d| d.reactivated_block_count())
            .sum()
    }

    // ---- DEBUG borrow flag (contract C: #[cfg(debug_assertions)] overlap check) ----

    /// On `&mut` entry, set the per-cell DEBUG borrow flag (sidecar AtomicU8 in a
    /// SEPARATE allocation, NOT inside the cell's UnsafeCell). Panics (does NOT form
    /// a conflicting &mut) if the cell is already mutably borrowed -> catches a
    /// careless overlapping/nested &mut WITHOUT itself committing the UB. NOT the
    /// membership gate — `find()` is.
    fn borrow_mut_guard(&self, cp: CellPtr) -> MutGuard<'_> {
        let flag: &AtomicU8 = self
            .debug_borrow_flags
            .get(&cp.addr())
            .expect("mutator deref of unallocated cell (find() gate violated)");
        if cfg!(debug_assertions) {
            let prev = flag.swap(BORROW_MUT, Ordering::Relaxed);
            assert_eq!(
                prev, BORROW_FREE,
                "overlapping &mut to one cell (contract C5)"
            );
        }
        MutGuard { flag }
    }
}

struct MutGuard<'a> {
    flag: &'a AtomicU8,
}

impl Drop for MutGuard<'_> {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            self.flag.store(BORROW_FREE, Ordering::Relaxed);
        }
    }
}

// ================================== TESTS ==================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::heap::marked_block::{
        clear_newly_allocated_block, ATOMS_PER_CELL, BLOCK_MASK, FIRST_PAYLOAD_ATOM,
    };

    /// Cells-per-block for the demo size class (after the header atoms).
    fn cells_per_block() -> usize {
        (ATOMS_PER_BLOCK - FIRST_PAYLOAD_ATOM) / ATOMS_PER_CELL
    }

    /// gc-r4 leak-fix C3 GUARD fires: allocating inside the mark-end -> shrink()
    /// window MUST panic (debug builds; tests run with debug_assertions). This is
    /// the tripwire pinning the `block_has_no_marks` soundness invariant — a future
    /// finalizer-callback feature that allocates during the collection end phase
    /// must hit this assertion, not silently reintroduce the newlyAllocated race
    /// (see the `no_alloc_until_shrink_completes` field doc).
    #[test]
    #[should_panic(expected = "allocation between mark-phase end and shrink()")]
    fn allocation_inside_mark_end_to_shrink_window_panics() {
        let mut space = MarkedSpace::new();
        space.note_mark_phase_end_for_shrink_guard();
        let _ = space.allocate(0x10, 0xDEAD);
    }

    /// gc-r4 leak-fix C3 GUARD clears: `shrink()` completing closes the window, so
    /// the same allocation succeeds afterwards — the guard gates exactly the
    /// window, not allocation in general.
    #[test]
    fn allocation_after_shrink_closes_the_guard_window() {
        let mut space = MarkedSpace::new();
        space.note_mark_phase_end_for_shrink_guard();
        space.shrink(); // closes the window (clears the debug flag)
        let cp = space.allocate(0x10, 0xBEEF);
        assert_eq!(space.find(cp.addr()), Some(cp), "post-shrink alloc is live");
    }

    /// gc-r4 R4b-mark THE #1 UAF LANDMINE in miniature: a live old-gen SURVIVOR has
    /// `newlyAllocated == 0` (cleared by the prior sweep) AND `marks == 0` (cleared at
    /// begin-marking). The liveness-bearing `find` then returns `None` and would (under
    /// the later sweep) reclaim it — but the collector's MEMBERSHIP gate `is_arena_cell`
    /// still admits it, so the marker can mark and retain it. This is the exact
    /// divergence the marker depends on.
    #[test]
    fn is_arena_cell_admits_survivor_that_liveness_find_rejects() {
        let mut space = MarkedSpace::new();
        let a = space.allocate(0x10, 0xA11E);
        let base = a.addr() & BLOCK_MASK;

        // Both liveness sources present right after allocation: BOTH gates admit.
        assert_eq!(space.find(a.addr()), Some(a));
        assert_eq!(space.is_arena_cell(a.addr()), Some(a));

        // Simulate the post-sweep + begin-marking survivor state: clear marks
        // (begin-marking) AND the alloc bitmap (the prior full-collection sweep).
        space.clear_all_marks();
        clear_newly_allocated_block(base);

        // `find` now REJECTS the survivor (is_live_cell == false) — gating the marker
        // through `find` here would orphan + sweep a LIVE cell (mass UAF).
        assert_eq!(
            space.find(a.addr()),
            None,
            "liveness find drops the survivor"
        );
        // The membership gate STILL admits it — the marker can mark + retain it.
        assert_eq!(
            space.is_arena_cell(a.addr()),
            Some(a),
            "membership gate retains the survivor"
        );

        // Foreign / non-arena addresses are rejected by BOTH gates (no type confusion).
        assert!(space.is_arena_cell(0xdead_beef).is_none());
        assert!(space.is_arena_cell(0).is_none());
    }

    /// `clear_all_marks` zeroes every block's mark bitmap (begin-marking), across
    /// multiple blocks, leaving newlyAllocated untouched.
    #[test]
    fn clear_all_marks_zeroes_marks_across_blocks() {
        let mut space = MarkedSpace::new();
        let n = cells_per_block() + 5; // force a second block
        let addrs: Vec<_> = (0..n).map(|i| space.allocate(0x10, i as u64)).collect();
        // Mark every cell, then clear.
        for &a in &addrs {
            assert!(space.collector_mark(a));
            assert!(space.collector_is_marked(a));
        }
        space.clear_all_marks();
        for &a in &addrs {
            assert!(
                !space.collector_is_marked(a),
                "mark cleared at begin-marking"
            );
            // newlyAllocated survives clear_all_marks, so the mutator gate still admits.
            assert_eq!(space.find(a.addr()), Some(a));
        }
    }

    /// A precise (large) cell's mark is also cleared by `clear_all_marks`.
    #[test]
    fn clear_all_marks_clears_precise_mark() {
        let mut space = MarkedSpace::new();
        let p = space.allocate_precise(0x20, 1);
        assert!(space.collector_mark(p));
        assert!(space.collector_is_marked(p));
        space.clear_all_marks();
        assert!(!space.collector_is_marked(p));
        // Precise membership is liveness-independent, so both gates still admit it.
        assert_eq!(space.is_arena_cell(p.addr()), Some(p));
    }

    /// THE CRUX TEST (the S2-UB interleaving). The exact sequence that made S2 UB:
    ///   1. allocate cell A and MUTATE a field through its carried address;
    ///   2. allocate MANY more cells, GROWING the block list (push new pages ->
    ///      realloc the Vec<*mut u8> of owning pointers);
    ///   3. re-deref A after EACH growth and read/write it again.
    /// In S2 step 2's owner-&mut popped A's carried tag -> deref in step 3 = UB.
    /// Here there is NO co-owning reference, so A's exposed provenance survives.
    /// Driving the SAME allocate->grow->re-deref->mark interleaving as the proven
    /// prototype crux and asserting identical results is the faithful-match guard.
    #[test]
    fn interleaved_mutate_grow_then_rederef_is_sound() {
        let mut space = MarkedSpace::new();

        // (1) Allocate A in the first block and mutate it.
        let a = space.allocate(0x10, 0xA000);
        space.mutator_write_field0(a, 0x1111);
        space.mutator_write_structure_id(a, 0xABCD);
        assert_eq!(space.raw_read_field0(a), 0x1111);

        // (2) Allocate enough to force at least TWO new blocks (two Vec growths),
        //     re-deref A after EACH growth to prove provenance stability.
        let per_block = cells_per_block();
        let mut others = Vec::new();
        let mut blocks_seen = std::collections::HashSet::new();
        blocks_seen.insert(a.addr() & BLOCK_MASK);
        for i in 0..(per_block * 2 + 3) {
            let b = space.allocate(0x11, 0xB000 + i as u64);
            space.mutator_write_field0(b, 0xB000 + i as u64);
            others.push(b);
            let nb = b.addr() & BLOCK_MASK;
            if blocks_seen.insert(nb) {
                // A NEW block was just created (the Vec<*mut u8> grew). Re-deref A
                // immediately: this is the precise S2 interleaving point.
                assert_eq!(space.raw_read_field0(a), 0x1111, "A field survives growth");
                assert_eq!(space.raw_read_structure_id(a), 0xABCD);
                space.mutator_write_field0(a, 0x1111); // write again through A
            }
        }
        assert!(
            blocks_seen.len() >= 3,
            "must have grown to >=3 blocks (got {})",
            blocks_seen.len()
        );

        // (3) Final re-deref of A after all growth: still valid + correct.
        assert_eq!(space.raw_read_field0(a), 0x1111);
        space.mutator_write_field0(a, 0x2222);
        assert_eq!(space.raw_read_field0(a), 0x2222);

        // Every other cell still independently readable (no cross-corruption).
        for (i, &b) in others.iter().enumerate() {
            assert_eq!(space.raw_read_field0(b), 0xB000 + i as u64);
        }
    }

    /// blockFor masking recovers the right block; collector mark/sweep round-trips
    /// across multiple blocks; mutator re-enters cleanly after a full GC cycle.
    #[test]
    fn marked_space_roundtrip_and_collector() {
        let mut space = MarkedSpace::new();
        let n = cells_per_block() + 7; // force a second block
        let mut addrs = Vec::new();
        for i in 0..n {
            addrs.push(space.allocate(0x10 + (i as u8 & 7), i as u64));
        }

        // blockFor(cell) must hit a registered block base; find() admits it.
        for &a in &addrs {
            assert!(!MarkedSpace::is_precise(a.addr()));
            assert_eq!(space.find(a.addr()), Some(a), "live cell admitted by gate");
        }

        // Mutator phase: write+read each cell via its carried address.
        for (i, &a) in addrs.iter().enumerate() {
            space.mutator_write_field0(a, 0xDEAD_0000 + i as u64);
        }
        for (i, &a) in addrs.iter().enumerate() {
            assert_eq!(space.raw_read_field0(a), 0xDEAD_0000 + i as u64);
            assert_eq!(space.raw_read_js_type(a), 0x10 + (i as u8 & 7));
        }

        // Collector phase (STW): mark via masking; idempotent; readable.
        for &a in &addrs {
            assert!(space.collector_mark(a), "first mark sets the bit");
            assert!(!space.collector_mark(a), "second mark finds it set");
            assert!(space.collector_is_marked(a));
            let (_sid, _ty, _f) = space.collector_read(a);
        }

        // Re-enter mutator after collection: fields still writable (provenance
        // survived a full cycle through the atomic mark words in the header).
        for (i, &a) in addrs.iter().enumerate() {
            space.mutator_write_field0(a, 0xBEEF_0000 + i as u64);
            assert_eq!(space.raw_read_field0(a), 0xBEEF_0000 + i as u64);
        }
    }

    /// PreciseAllocation +8 dispatch: a precise cell carries the halfAlignment bit;
    /// deref masks-then-recovers; mark lives in the precise header.
    #[test]
    fn precise_allocation_plus8_dispatch() {
        let mut space = MarkedSpace::new();
        let p = space.allocate_precise(0x20, 0xC0DE);
        assert!(
            MarkedSpace::is_precise(p.addr()),
            "precise cell carries +8 bit"
        );
        assert_eq!(p.addr() & HALF_ALIGNMENT, HALF_ALIGNMENT);
        assert_eq!(
            space.find(p.addr()),
            Some(p),
            "precise cell admitted by gate"
        );

        space.mutator_write_field0(p, 0x9999);
        assert_eq!(space.raw_read_field0(p), 0x9999);
        assert_eq!(space.raw_read_js_type(p), 0x20);

        assert!(space.collector_mark(p));
        assert!(!space.collector_mark(p));
        assert!(space.collector_is_marked(p));
        let (_sid, ty, f) = space.collector_read(p);
        assert_eq!(ty, 0x20);
        assert_eq!(f, 0x9999);

        // Mix marked + precise in one space; both dispatch correctly.
        let m = space.allocate(0x10, 1);
        assert!(!MarkedSpace::is_precise(m.addr()));
        assert!(space.collector_mark(m));
        assert!(space.collector_is_marked(m));
        assert!(space.collector_is_marked(p)); // precise mark independent
    }

    /// The production validity gate refuses non-arena / dangling / interior
    /// addresses without deref — exercising the REGISTRY gate, not a HashMap.
    #[test]
    fn find_gate_rejects_non_arena_addresses() {
        let mut space = MarkedSpace::new();
        let a = space.allocate(0x10, 7);
        assert_eq!(space.find(a.addr()), Some(a));
        // Not in any registered block (bloom filter / set membership reject).
        assert!(space.find(0xdead_beef).is_none());
        // Zero is ruled out by the TinyBloomFilter (ruleOut(0) == true).
        assert!(space.find(0).is_none());
        // Interior / misaligned pointer into a live cell: isAtomAligned rejects.
        assert!(space.find(a.addr() + 1).is_none());
        // Atom-aligned but cell-middle pointer (a + one atom): isAtom rejects it.
        assert!(space.find(a.addr() + ATOM_SIZE).is_none());
    }

    /// DEBUG borrow flag catches an overlapping &mut by PANICKING at the flag check
    /// BEFORE forming the second &mut -> no real UB, just a clean panic. (Only
    /// meaningful with debug_assertions; test/miri builds keep them on.)
    #[test]
    #[should_panic(expected = "overlapping &mut")]
    fn debug_borrow_flag_detects_overlap() {
        let mut space = MarkedSpace::new();
        let a = space.allocate(0x10, 1);
        let _g1 = space.borrow_mut_guard(a); // holds the flag (no live &mut Cell)
                                             // Second borrow of the SAME cell: the flag check panics before any second
                                             // &mut is formed. Catches the logical overlap without committing UB.
        let _g2 = space.borrow_mut_guard(a);
    }

    /// The size-class table is the FULL JSC set (MarkedSpace.cpp:40-160), not the
    /// schema seed in gc::space (which is missing the 48B class). Faithful checks:
    /// per-step classes 16/32/48/64/80, geometric classes above 80, the injected
    /// 256, and optimalSizeFor routing.
    #[test]
    fn size_class_table_matches_jsc_algorithm() {
        let classes = &size_class_table().size_classes;

        // Exact per-step classes up to preciseCutoff INCLUDING 48 (the class the
        // legacy gc::space::STATIC_MARKED_SIZE_CLASSES seed omits).
        for expected in [16usize, 32, 48, 64, 80] {
            assert!(
                classes.contains(&expected),
                "missing per-step class {expected}"
            );
        }

        // optimalSizeFor below preciseCutoff rounds up to the step.
        assert_eq!(optimal_size_for(1), 16);
        assert_eq!(optimal_size_for(16), 16);
        assert_eq!(optimal_size_for(17), 32);
        assert_eq!(optimal_size_for(48), 48);
        assert_eq!(optimal_size_for(80), 80);

        // The injected high-volume 256 class exists and routes exactly.
        assert!(classes.contains(&256), "missing injected 256 class");
        assert_eq!(optimal_size_for(256), 256);
        assert_eq!(optimal_size_for(255), 256);

        // Geometric region: there is at least one class strictly between 80 and the
        // injected 256, and every class is a multiple of the step, sorted, unique.
        assert!(
            classes.iter().any(|&c| c > PRECISE_CUTOFF && c < 256),
            "no geometric class in (80,256)"
        );
        for w in classes.windows(2) {
            assert!(w[0] < w[1], "classes must be sorted+deduped");
        }
        for &c in classes {
            assert_eq!(c % SIZE_STEP, 0, "class {c} not a multiple of sizeStep");
            assert!(
                c <= LARGE_CUTOFF || c == 256,
                "class {c} exceeds largeCutoff"
            );
        }

        // optimalSizeFor in (preciseCutoff, largeCutoff] returns a real class;
        // above largeCutoff it returns the byte size unchanged (precise path).
        let mid = LARGE_CUTOFF; // <= largeCutoff -> a size class
        assert!(optimal_size_for(mid) >= mid);
        assert_eq!(optimal_size_for(LARGE_CUTOFF + 1), LARGE_CUTOFF + 1);

        // Routing dispatch: a payload <= largeCutoff is Marked, above is Precise.
        assert!(matches!(size_route(64), SizeRoute::Marked(64)));
        assert!(matches!(
            size_route(LARGE_CUTOFF + 1),
            SizeRoute::Precise(_)
        ));
    }
}
