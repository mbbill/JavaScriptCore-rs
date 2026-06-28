//! SlotVisitor: the marking visitor that drives a stop-the-world collection's
//! transitive mark phase (heap/SlotVisitor.h, heap/SlotVisitor.cpp, the inline
//! fast paths in heap/SlotVisitorInlines.h, and the mark-stack member in
//! heap/AbstractSlotVisitor.h:224). Collector Batch 2 of the S4 arena port
//! (Batch 1 = FreeList). Faithful port of the single-thread, stop-the-world
//! marking core: the collector mark stack (append / drain to fixpoint), the
//! `visitChildren`-driven edge walk, `testAndSetMarked` via the arena per-block
//! mark bitmap (gc/heap/marked_block.rs), and the mark-from-roots entry that
//! consumes the safepoint conservative-root gather.
//!
//! WHAT JSC DOES (the model this ports):
//!   - The collector seeds a worklist ("mark stack") from the roots, then drains
//!     it to a fixpoint: pop a grey cell, paint it black, enumerate its outgoing
//!     edges, and for each edge `testAndSetMarked` the target — pushing it grey
//!     onto the stack the first time it transitions white->grey. When the stack
//!     empties, every cell reachable from the roots is marked; everything still
//!     unmarked is garbage the sweeper reclaims (heap/SlotVisitor.cpp:488-518
//!     `drain`; :350-414 `visitChildren`; :255-296 the mark/append path).
//!   - A root is appended via `appendJSCellOrAuxiliary` (SlotVisitor.cpp:142):
//!     `Heap::testAndSetMarked` then, on the first mark, paint grey + push. A
//!     write-barrier/field edge is appended via `appendUnbarriered`
//!     (SlotVisitorInlines.h:43): an `isMarked` fast-path early-out, then the
//!     `appendSlow` -> `setMarkedAndAppendToMarkStack` test-and-set + grey + push.
//!   - Each cell's edges are enumerated by `cell->methodTable()->visitChildren`,
//!     dispatched per cell type (SlotVisitor.cpp:374-413). Every `visitChildren`
//!     body calls `visitor.append*` on its `WriteBarrier` slots.
//!
//! WHERE THE STATE LIVES (the S4 unsafe contract — see marked_block.rs C1-C6):
//! mark bits are the per-block atomic `m_marks` bitmap reached by `blockFor`
//! masking; `testAndSetMarked`/`isMarked` are the free functions in
//! marked_block.rs (MarkedBlock.h:613-637). The cell-state byte is JSCell header
//! offset 7, written through the interior-mutable cell slot (contract C4). This
//! visitor forms NO `&MarkedBlock` and never moves a cell; it walks raw cell
//! machine addresses (`CellPtr`, the carried `JSCell*` identity).
//!
//! NOT WIRED (collector Batch 2, behind `dead_code`): the collector cannot RUN
//! until R3/R4 put real typed cells in the arena (the `VisitChildren` edge
//! enumeration is supplied by the caller / by per-type method tables that do not
//! exist yet) and wire the safepoint root gather. The mark-from-roots entry here
//! takes a pre-gathered, already-liveness-validated root cell list — the output
//! of the interpreter safepoint gather (`gather_vm_register_roots` /
//! `gather_vm_frame_header_roots`, interpreter/mod.rs, NOT edited by this batch).
//!
//! DIVERGENCE — single-thread, stop-the-world reduction (faithful to the
//! marked_block.rs C5/C6 STW horizon). This batch ports the marking ALGORITHM,
//! not the concurrent/parallel machinery. Omitted with intent, each deferred to a
//! later concurrent-collector batch:
//!   - `m_mutatorStack` (the second of `forEachMarkStack`,
//!     AbstractSlotVisitor.h:225) and the segmented `MarkStackArray`
//!     (GCSegmentedArray) shape: only the collector stack exists here, modeled as
//!     a `Vec<CellPtr>`. Segmentation exists purely to donate work between
//!     parallel markers.
//!   - parallel donation / `donateKnownParallel` / `drainFromShared` /
//!     `m_rightToRun` / `MonotonicTime` timeout / the `minimumNumberOfScans
//!     BetweenRebalance` countdown (SlotVisitor.cpp:426-518): no second marker
//!     thread exists, so `drain` is an unconditional drain to the fixpoint.
//!   - `m_markingVersion`, `aboutToMark`, and the `Dependency` consume-load
//!     (SlotVisitorInlines.h:43-67): the STW horizon treats the marking version
//!     as never stale (same deferral as marked_block.rs `is_live_cell`), so a
//!     plain `is_marked`/`test_and_set_marked` stands in for the versioned read.
//!   - `HeapAnalyzer` edge/node analysis, `SetCurrentCellScope` (`m_currentCell`),
//!     `Integrity` audits, `reportExtraMemoryVisited`, the `Auxiliary` cell kind
//!     (`noteLiveAuxiliaryCell`), and `WTF::storeLoadFence` (a concurrent-marking
//!     ordering fence): none affect the mark result under STW single-thread.

#![allow(dead_code)]

use core::ptr;

use super::marked_block::{
    block_for, cell_ptr, is_marked, test_and_set_marked, CellPtr, MarkedBlock, ATOM_SIZE,
};
use crate::gc::CellState;

// ===================== VisitChildren (the methodTable boundary) =====================

/// The `cell->methodTable()->visitChildren(cell, visitor)` boundary
/// (heap/SlotVisitor.cpp:374-413): given a cell, enumerate its outgoing strong
/// edges by calling `visitor.append_unbarriered` on each child. This is the
/// arena-side analog of the descriptor-world `gc::trace::Trace`/`Tracer` pair —
/// distinct because it speaks raw `CellPtr` machine addresses (the JIT/IC
/// `JSCell*` identity the S4 arena exposes once at allocation) rather than
/// `GcRef<JsCell>` descriptors; R3/R4 reconcile the two when typed cells land.
///
/// In C++ the dispatch is a per-`ClassInfo` virtual method-table call, so a
/// `&dyn VisitChildren` faithfully mirrors that vtable indirection: one
/// implementor stands in for the whole per-type method table until R3/R4 install
/// real typed cells with their own `visitChildren` bodies.
pub(crate) trait VisitChildren {
    /// `JSXxx::visitChildren(cell, visitor)`: append each child edge of `cell`.
    /// Implementors must NOT mark or mutate cells themselves; they only hand
    /// borrowed child identities to `visitor.append_unbarriered`.
    fn visit_children(&self, cell: CellPtr, visitor: &mut SlotVisitor);
}

// ===================== MarkStack (AbstractSlotVisitor::m_collectorStack) =====================

/// `MarkStackArray m_collectorStack` (heap/AbstractSlotVisitor.h:224). DIVERGENCE
/// (STW single-thread reduction): JSC uses a `GCSegmentedArray<const JSCell*>`
/// whose segmentation exists to donate work between parallel markers; with one
/// marker thread the faithful reduction is a `Vec<CellPtr>` used LIFO (DFS),
/// matching `MarkStackArray::append`/`removeLast`.
#[derive(Default)]
pub(crate) struct MarkStack {
    cells: Vec<CellPtr>,
}

impl MarkStack {
    pub(crate) fn new() -> Self {
        MarkStack { cells: Vec::new() }
    }

    /// `MarkStackArray::append`.
    #[inline]
    fn append(&mut self, cell: CellPtr) {
        self.cells.push(cell);
    }

    /// `MarkStackArray::removeLast` guarded by `canRemoveLast` (the drain pops the
    /// most-recently greyed cell — depth-first).
    #[inline]
    fn remove_last(&mut self) -> Option<CellPtr> {
        self.cells.pop()
    }

    /// `MarkStackArray::isEmpty`.
    #[inline]
    fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// `MarkStackArray::clear` (SlotVisitor::clearMarkStacks, SlotVisitor.cpp:127).
    fn clear(&mut self) {
        self.cells.clear();
    }
}

// ===================== cell-state / cell-size helpers =====================

/// `JSCell::setCellState` (runtime/JSCellInlines.h): store the `CellState` byte at
/// JSCell header offset 7 (the `m_cellState` field pinned by
/// `marked_block::JsCellHeader`). Written through the interior-mutable cell slot.
#[inline]
fn set_cell_state(cell: CellPtr, state: CellState) {
    let cp = cell_ptr(cell.addr());
    // SAFETY (contract C3/C4): `cell.addr()` is a live, registered, once-exposed
    // arena cell; `cell_ptr` recovers the interior-mutable slot from the page's
    // exposed provenance. The `addr_of_mut!` write of a single header byte forms
    // no reference and aliases no sibling field. STW single mutator/collector
    // horizon (C5/C6): no concurrent access to this byte.
    unsafe {
        ptr::addr_of_mut!((*cp).header.cell_state).write(state as u8);
    }
}

/// `container.cellSize()` (CellContainer::cellSize): the cell's size class in
/// bytes, read from the block header (`atoms_per_cell`) reached by `blockFor`
/// masking. Drives `m_bytesVisited` (SlotVisitor.cpp:294).
#[inline]
fn container_cell_size(cell_addr: usize) -> usize {
    let base = block_for(cell_addr); // MarkedBlock::blockFor (MarkedBlock.h:489-492)
    let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(base).cast();
    // SAFETY (contract C3): `base` is a registered, once-exposed block (the caller
    // only ever passes addresses of cells it allocated into the arena); the
    // `addr_of!` read of the header's `atoms_per_cell` forms no reference.
    let atoms = unsafe { ptr::addr_of!((*bp).header.atoms_per_cell).read() } as usize;
    atoms * ATOM_SIZE
}

// ===================== SlotVisitor =====================

/// `SlotVisitor` (heap/SlotVisitor.h) reduced to its STW single-thread marking
/// core. Owns the collector mark stack and the visit/byte counters; the mark bits
/// and cell-state bytes it flips live in the Heap-owned arena (marked_block.rs),
/// not in the visitor.
#[derive(Default)]
pub(crate) struct SlotVisitor {
    /// `m_collectorStack` (AbstractSlotVisitor.h:224).
    collector_stack: MarkStack,
    /// `m_visitCount` (AbstractSlotVisitor.h:227): cells appended to the stack.
    visit_count: usize,
    /// `m_bytesVisited` (SlotVisitor.h:213): sum of marked cells' size classes.
    bytes_visited: usize,
}

impl SlotVisitor {
    pub(crate) fn new() -> Self {
        SlotVisitor {
            collector_stack: MarkStack::new(),
            visit_count: 0,
            bytes_visited: 0,
        }
    }

    /// `AbstractSlotVisitor::isEmpty` (AbstractSlotVisitor.h:139), reduced to the
    /// sole collector stack.
    pub(crate) fn is_empty(&self) -> bool {
        self.collector_stack.is_empty()
    }

    /// `AbstractSlotVisitor::visitCount` (AbstractSlotVisitor.h:185).
    pub(crate) fn visit_count(&self) -> usize {
        self.visit_count
    }

    /// `SlotVisitor::bytesVisited` (SlotVisitor.h:120).
    pub(crate) fn bytes_visited(&self) -> usize {
        self.bytes_visited
    }

    /// `SlotVisitor::clearMarkStacks` (SlotVisitor.cpp:127) + counter reset
    /// (`SlotVisitor::reset`, SlotVisitor.cpp:118-123). Lets one visitor drive
    /// successive collections.
    pub(crate) fn reset(&mut self) {
        self.collector_stack.clear();
        self.visit_count = 0;
        self.bytes_visited = 0;
    }

    // ---- root append: SlotVisitor::append(const ConservativeRoots&) ----

    /// `SlotVisitor::append(const ConservativeRoots&)` (SlotVisitor.cpp:134-140):
    /// append every gathered root. The driver passes the cell list produced by the
    /// safepoint conservative-root gather (the validated `HeapCell**` of
    /// `ConservativeRoots::roots()`); here that is the already-liveness-checked
    /// output of the interpreter safepoint gather, supplied as `CellPtr`s.
    pub(crate) fn append_conservative_roots(&mut self, roots: &[CellPtr]) {
        for &root in roots {
            self.append_js_cell_or_auxiliary(root);
        }
    }

    /// `SlotVisitor::appendJSCellOrAuxiliary` (SlotVisitor.cpp:142-224), JSCell
    /// branch. `Heap::testAndSetMarked` then, on the first white->grey transition,
    /// paint grey and push. (Validation/integrity audits and the `Auxiliary`
    /// branch are deferred — see the module DIVERGENCE note.)
    fn append_js_cell_or_auxiliary(&mut self, cell: CellPtr) {
        // Heap::testAndSetMarked: returns true the first time it sets the bit.
        if !test_and_set_marked(cell.addr()) {
            return; // already marked this cycle
        }
        // setCellState(PossiblyGrey): queued for scanning (SlotVisitor.cpp:214).
        set_cell_state(cell, CellState::PossiblyGrey);
        self.append_to_mark_stack(cell);
    }

    // ---- field/edge append: SlotVisitor::appendUnbarriered(JSCell*) ----

    /// `SlotVisitor::appendUnbarriered(JSCell*)` (heap/SlotVisitorInlines.h:43-67).
    /// The `isMarked` fast path early-outs on already-marked targets; otherwise
    /// `appendSlow`. (Null targets are screened by the `VisitChildren` callers,
    /// matching the `if (!cell) return;` guard.)
    pub(crate) fn append_unbarriered(&mut self, cell: CellPtr) {
        // block.isMarked(cell) fast path: most edges point at already-marked cells.
        if is_marked(cell.addr()) {
            return;
        }
        self.append_slow(cell);
    }

    /// `SlotVisitor::appendSlow` -> `appendHiddenSlowImpl`
    /// (SlotVisitor.cpp:228-253). The heap-analyzer edge hook is deferred, so this
    /// reduces to `setMarkedAndAppendToMarkStack` directly (single MarkedBlock
    /// container path; the PreciseAllocation branch lands with large cells in R2).
    fn append_slow(&mut self, cell: CellPtr) {
        self.set_marked_and_append_to_mark_stack(cell);
    }

    /// `SlotVisitor::setMarkedAndAppendToMarkStack` (SlotVisitor.cpp:256-269):
    /// `container.testAndSetMarked` (returns if another append already set it),
    /// then paint grey and push.
    fn set_marked_and_append_to_mark_stack(&mut self, cell: CellPtr) {
        if !test_and_set_marked(cell.addr()) {
            return; // was already set (the isMarked fast path raced/lost — benign)
        }
        // setCellState(PossiblyGrey) (SlotVisitor.cpp:266).
        set_cell_state(cell, CellState::PossiblyGrey);
        self.append_to_mark_stack(cell);
    }

    /// `SlotVisitor::appendToMarkStack(ContainerType&, JSCell*)`
    /// (SlotVisitor.cpp:280-296): account the visit (`noteMarked` block bookkeeping
    /// is deferred to R2) and push the now-grey cell onto the collector stack.
    fn append_to_mark_stack(&mut self, cell: CellPtr) {
        self.visit_count += 1;
        self.bytes_visited += container_cell_size(cell.addr());
        self.collector_stack.append(cell);
    }

    // ---- the edge walk: SlotVisitor::visitChildren ----

    /// `SlotVisitor::visitChildren` (SlotVisitor.cpp:350-414): paint the popped
    /// cell black, then dispatch to its method table's `visitChildren` to append
    /// its children. `storeLoadFence` (a concurrent-marking ordering fence) is
    /// omitted under the STW single-thread horizon.
    fn visit_children(&mut self, cell: CellPtr, classes: &dyn VisitChildren) {
        debug_assert!(is_marked(cell.addr()), "visitChildren on an unmarked cell");
        // cell->setCellState(PossiblyBlack): scanned (SlotVisitor.cpp:373).
        set_cell_state(cell, CellState::PossiblyBlack);
        // cell->methodTable()->visitChildren(cell, *this) (SlotVisitor.cpp:413).
        // Disjoint borrows: `classes` (&dyn) is external to `self` (&mut), exactly
        // as the C++ method table is reached through the cell, not the visitor.
        classes.visit_children(cell, self);
    }

    // ---- drain to fixpoint: SlotVisitor::drain ----

    /// `SlotVisitor::drain` (SlotVisitor.cpp:488-518), STW single-thread reduction:
    /// pop and `visitChildren` until the collector stack is empty. Each
    /// `visitChildren` may push newly greyed children, so the loop runs to the
    /// transitive-closure fixpoint. No timeout, no mutator stack, no parallel
    /// donation/rebalance countdown (see module DIVERGENCE note).
    pub(crate) fn drain(&mut self, classes: &dyn VisitChildren) {
        while let Some(cell) = self.collector_stack.remove_last() {
            self.visit_children(cell, classes);
        }
    }

    /// The mark-from-roots entry: seed the stack from the gathered roots and drain
    /// to the fixpoint. This is the SlotVisitor side of the Heap's
    /// mark-roots-then-drain loop (the conservative-root append of
    /// SlotVisitor.cpp:134 followed by `drain`). After it returns, every cell
    /// reachable from `roots` is marked; every still-unmarked arena cell is
    /// garbage the sweeper reclaims.
    pub(crate) fn mark_from_roots(&mut self, roots: &[CellPtr], classes: &dyn VisitChildren) {
        self.append_conservative_roots(roots);
        self.drain(classes);
    }
}

// ================================== TESTS ==================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::heap::block_directory::BlockDirectory;
    use crate::gc::heap::marked_block::{Cell, ATOMS_PER_CELL};

    /// A null child slot (the JSCell `WriteBarrier` holding no pointer).
    fn none() -> CellPtr {
        CellPtr::from_addr(0)
    }

    /// A test object graph laid over the demo `Cell`'s two inline field words: each
    /// word holds a child cell's machine address (0 == no child). This stands in
    /// for a JSCell with two `WriteBarrier` slots whose `visitChildren` appends
    /// each non-null slot — exactly the `JSFinalObject::visitChildren` shape.
    struct InlineFieldGraph;

    impl VisitChildren for InlineFieldGraph {
        fn visit_children(&self, cell: CellPtr, visitor: &mut SlotVisitor) {
            let cp = cell_ptr(cell.addr());
            // SAFETY (contract C3/C5): `cell` is a live arena cell; the collector
            // forms only shared reads of cell fields at STW. `addr_of!` reads form
            // no reference.
            let (c0, c1) = unsafe {
                (
                    ptr::addr_of!((*cp).field0).read(),
                    ptr::addr_of!((*cp).field1).read(),
                )
            };
            for child in [c0, c1] {
                if child != 0 {
                    // appendUnbarriered on a non-null WriteBarrier slot.
                    visitor.append_unbarriered(CellPtr::from_addr(child as usize));
                }
            }
        }
    }

    /// Write the two child-edge slots of a cell (the mutator installing two
    /// `WriteBarrier` fields). Uses the same interior-mutable slot path as the
    /// allocator (contract C4).
    fn set_children(cell: CellPtr, c0: CellPtr, c1: CellPtr) {
        let cp = cell_ptr(cell.addr());
        // SAFETY (contract C3/C4): `cell` is a freshly allocated, sole-owned arena
        // cell; writing its inline field words forms no reference and aliases no
        // sibling field. Single mutator before any collector entry (C5).
        unsafe {
            ptr::addr_of_mut!((*cp).field0).write(c0.addr() as u64);
            ptr::addr_of_mut!((*cp).field1).write(c1.addr() as u64);
        }
    }

    /// Allocate `n` cells of the demo size class into a fresh arena directory.
    fn alloc_cells(dir: &mut BlockDirectory, n: usize) -> Vec<CellPtr> {
        (0..n)
            .map(|i| dir.allocate(Cell::new(0 /* type */, i as u64)).0)
            .collect()
    }

    /// Mark a small object graph from a single root, drain to fixpoint, and confirm
    /// the reachable set is marked, the unreachable set is left for sweep, and the
    /// visit/byte counters match the reachable cells exactly.
    #[test]
    fn marks_reachable_graph_and_leaves_garbage_for_sweep() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
        // root -> a -> b -> c (a chain); d, e isolated (unreachable garbage).
        let cells = alloc_cells(&mut dir, 6);
        let (root, a, b, c, d, e) = (cells[0], cells[1], cells[2], cells[3], cells[4], cells[5]);
        set_children(root, a, none());
        set_children(a, b, none());
        set_children(b, c, none());
        set_children(c, none(), none());
        // d, e keep their zeroed field words (no edges) and are unreferenced.

        let graph = InlineFieldGraph;
        let mut visitor = SlotVisitor::new();
        visitor.mark_from_roots(&[root], &graph);

        // Reachable closure {root,a,b,c} is marked; garbage {d,e} is not.
        for &live in &[root, a, b, c] {
            assert!(is_marked(live.addr()), "reachable cell must be marked");
        }
        for &garbage in &[d, e] {
            assert!(
                !is_marked(garbage.addr()),
                "unreachable cell stays unmarked"
            );
        }

        // The unmarked set is exactly the sweep candidate set.
        let sweep: Vec<CellPtr> = cells
            .iter()
            .copied()
            .filter(|cell| !is_marked(cell.addr()))
            .collect();
        assert_eq!(sweep, vec![d, e], "garbage identified for sweep");

        // visitCount / bytesVisited cover the four reachable cells only.
        assert_eq!(visitor.visit_count(), 4);
        assert_eq!(visitor.bytes_visited(), 4 * ATOMS_PER_CELL * ATOM_SIZE);
        assert!(visitor.is_empty(), "stack drained to the fixpoint");
    }

    /// A cycle in the object graph must terminate (each cell is appended at most
    /// once, since `testAndSetMarked` greys it exactly once) and mark the whole
    /// cycle.
    #[test]
    fn cyclic_graph_drains_to_fixpoint_once_per_cell() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
        let cells = alloc_cells(&mut dir, 3);
        let (root, a, b) = (cells[0], cells[1], cells[2]);
        // root -> a -> b -> root (a cycle) plus a back-edge a -> root.
        set_children(root, a, none());
        set_children(a, b, root);
        set_children(b, root, none());

        let graph = InlineFieldGraph;
        let mut visitor = SlotVisitor::new();
        visitor.mark_from_roots(&[root], &graph);

        for &live in &[root, a, b] {
            assert!(is_marked(live.addr()));
        }
        // Each cell greyed/visited exactly once despite the back-edges.
        assert_eq!(visitor.visit_count(), 3);
        assert!(visitor.is_empty());
    }

    /// Re-appending an already-marked root (or a root reached as a child) is a
    /// no-op: `append_conservative_roots` and `append_unbarriered` both early-out
    /// on the set mark bit, so the second collection seed adds no duplicate work.
    #[test]
    fn already_marked_roots_and_edges_are_idempotent() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
        let cells = alloc_cells(&mut dir, 2);
        let (root, child) = (cells[0], cells[1]);
        set_children(root, child, none());
        set_children(child, root, none()); // child also points back at root

        let graph = InlineFieldGraph;
        let mut visitor = SlotVisitor::new();
        // Seed BOTH cells as roots and let the graph re-discover them as edges.
        visitor.mark_from_roots(&[root, child, root], &graph);

        assert!(is_marked(root.addr()) && is_marked(child.addr()));
        // Two distinct cells, each marked/visited exactly once.
        assert_eq!(visitor.visit_count(), 2);
        assert!(visitor.is_empty());
    }
}
