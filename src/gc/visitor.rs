//! Marking worklists, slot visitors, and conservative root scan descriptors.

// Salvage: collector-effects / conservative-root-marking / verifier slot-visitor
// proofs, consumed only by the gated ARM64 admission-proof cluster + their own
// tests. Gated off by default; the live SlotVisitor path (below) does not use
// them. (Map: heap/SlotVisitor.cpp / VerifierSlotVisitor.cpp — gated, never
// deleted.)
#[cfg(feature = "arm64_native_entry_proof")]
mod collector_effects;
#[cfg(feature = "arm64_native_entry_proof")]
mod conservative_marking;
#[cfg(feature = "arm64_native_entry_proof")]
mod verifier;

#[cfg(feature = "arm64_native_entry_proof")]
pub(crate) use collector_effects::{
    SlotVisitorAppendToMarkStackRecord, SlotVisitorCollectorEffectAction,
    SlotVisitorCollectorEffectRecord, SlotVisitorCollectorEffectsError,
    SlotVisitorCollectorEffectsPlan, SlotVisitorContainerNoteMarkedRecord,
    SlotVisitorNoteLiveAuxiliaryCellRecord,
};
#[cfg(feature = "arm64_native_entry_proof")]
pub(crate) use conservative_marking::{
    SlotVisitorConservativeRootMarkingAction, SlotVisitorConservativeRootMarkingError,
    SlotVisitorConservativeRootMarkingPlan, SlotVisitorConservativeRootMarkingRecord,
};
#[cfg(feature = "arm64_native_entry_proof")]
pub(crate) use verifier::{
    VerifierSlotVisitorCollectorStackAppendRecord, VerifierSlotVisitorConservativeRootAppendAction,
    VerifierSlotVisitorConservativeRootAppendError, VerifierSlotVisitorConservativeRootAppendPlan,
    VerifierSlotVisitorConservativeRootAppendProof,
    VerifierSlotVisitorConservativeRootAppendRecord, VerifierSlotVisitorDescriptor,
    VerifierSlotVisitorTestAndSetMarkRecord,
};

use crate::gc::{
    CellId, ConservativeRootCell, ConservativeRootSpan, ConservativeRoots, GcRef, HeapEpoch,
    HeapId, JsCell, MarkReason, RootRecord, TargetedRootRecord,
};

/// C++ `RootMarkReason` values that annotate a visitor's current root context.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RootMarkReason {
    #[default]
    None,
    ConservativeScan,
    ExecutableToCodeBlockEdges,
    ExternalRememberedSet,
    StrongReferences,
    ProtectedValues,
    MarkListSet,
    VMExceptions,
    StrongHandles,
    Debugger,
    JitStubRoutines,
    WeakMapSpace,
    WeakSets,
    Output,
    JitWorkList,
    CodeBlocks,
    DomGcOutput,
    PinballCompletionConservativeRoots,
}

/// Referrer token carried while visiting children.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReferrerTokenKind {
    #[default]
    None,
    HeapCell,
    OpaqueRoot,
    RootMarkReason,
}

/// Descriptor for the current referrer context.
///
/// The token is intentionally not a pointer-tagged representation. Pointer
/// tagging is a layout decision for a future C++ interop boundary. Referrers
/// describe traversal context and must not be used as cell identity authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReferrerToken {
    pub kind: ReferrerTokenKind,
    pub address: usize,
    pub root_mark_reason: RootMarkReason,
}

/// Opaque marking worklist identity.
///
/// Worklist IDs name queues owned by the collector. They do not identify the
/// cells stored as queued work items.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct MarkWorklistId(pub u64);

/// Worklist role in the collector.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkWorklistKind {
    #[default]
    Mutator,
    Collector,
    Shared,
    Auxiliary,
    Verifier,
}

/// Movement of mark-stack cells between local and shared stacks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkStackTransferKind {
    #[default]
    Transfer,
    Donate,
    Steal,
}

/// Dependency attached to a mark operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkDependency {
    #[default]
    Strong,
    Hidden,
    Weak,
    Conservative,
    Auxiliary,
}

/// One item that would be appended to a mark stack.
///
/// The item borrows a cell for tracing under collector authority. It records
/// why the cell is visited, not ownership of the cell.
#[derive(Clone, Copy, Debug)]
pub struct MarkWorkItem {
    pub cell: GcRef<JsCell>,
    pub reason: MarkReason,
    pub dependency: MarkDependency,
    pub referrer: Option<ReferrerToken>,
}

/// Descriptor for C++ `SlotVisitor::append(const ConservativeRoots&)`.
///
/// Records are validated cell identities, not borrowed JSCell storage. The
/// visitor metadata names the collector context that would own the mark-stack
/// mutation in C++.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotVisitorConservativeRootAppendPlan {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub dependency: MarkDependency,
    pub referrer: Option<ReferrerToken>,
    pub records: Vec<SlotVisitorConservativeRootAppendRecord>,
}

/// One ordered conservative root append descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotVisitorConservativeRootAppendRecord {
    pub order: usize,
    pub root: ConservativeRootCell,
    pub cell: CellId,
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub dependency: MarkDependency,
    pub referrer: Option<ReferrerToken>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotVisitorConservativeRootAppendError {
    HeapMismatch {
        visitor: HeapId,
        roots: HeapId,
    },
    MarkingEpochMismatch {
        visitor: HeapEpoch,
        roots: HeapEpoch,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    InvalidConservativeRootCell(ConservativeRootCell),
}

/// Descriptor for stack balancing between mutator, collector, and helper worklists.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MarkStackTransfer {
    pub from: MarkWorklistId,
    pub to: MarkWorklistId,
    pub kind: MarkStackTransferKind,
    pub limit: Option<usize>,
    pub idle_thread_count: usize,
}

/// Observable mark-stack counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MarkWorklistStats {
    pub pending_cells: usize,
    pub pending_bytes: usize,
    pub donated_cells: usize,
}

/// Descriptor for one mark worklist.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkWorklistDescriptor {
    pub id: MarkWorklistId,
    pub kind: MarkWorklistKind,
    pub stats: MarkWorklistStats,
}

/// Drain mode used by a slot visitor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DrainMode {
    #[default]
    Main,
    Helper,
    PassiveParallel,
    Incremental,
}

/// Outcome of a drain increment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DrainResult {
    #[default]
    Done,
    TimedOut,
    NeedsMoreWork,
}

/// Conservative scan source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConservativeRootSource {
    #[default]
    MachineStack,
    VMRegisters,
    JitStubRoutines,
    Host,
}

/// Opaque root tracked outside ordinary cell fields.
///
/// Opaque root IDs belong to VM/host registries and must not be converted into
/// `CellId`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct OpaqueRootId(pub u64);

/// Opaque roots are owned by VM/host subsystems and discovered by the visitor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpaqueRootRecord {
    pub id: OpaqueRootId,
    pub address: usize,
    pub reason: RootMarkReason,
}

/// Slot visitor state visible to tracing and diagnostics.
///
/// The visitor borrows heap state for a marking epoch. Appending work items is
/// collector mutation authority over work queues, not object mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotVisitorDescriptor {
    pub heap: HeapId,
    pub code_name: &'static str,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub bytes_visited: usize,
    pub non_cell_visit_count: usize,
    pub mutator_is_stopped: bool,
    pub mutator_state_is_current: bool,
    pub is_parallel: bool,
    pub can_optimize_for_stopped_mutator: bool,
    pub current_referrer: Option<ReferrerToken>,
    pub root_mark_reason: RootMarkReason,
}

impl SlotVisitorDescriptor {
    pub fn new(heap: HeapId, code_name: &'static str, marking_epoch: HeapEpoch) -> Self {
        Self {
            heap,
            code_name,
            marking_epoch,
            worklist: MarkWorklistId::default(),
            bytes_visited: 0,
            non_cell_visit_count: 0,
            mutator_is_stopped: false,
            mutator_state_is_current: false,
            is_parallel: false,
            can_optimize_for_stopped_mutator: false,
            current_referrer: None,
            root_mark_reason: RootMarkReason::None,
        }
    }

    pub fn append_cell(&self, cell: GcRef<JsCell>, reason: MarkReason) -> MarkWorkItem {
        MarkWorkItem {
            cell,
            reason,
            dependency: MarkDependency::Strong,
            referrer: self.current_referrer,
        }
    }

    pub fn append_hidden_cell(&self, cell: GcRef<JsCell>, reason: MarkReason) -> MarkWorkItem {
        MarkWorkItem {
            cell,
            reason,
            dependency: MarkDependency::Hidden,
            referrer: self.current_referrer,
        }
    }

    pub fn append_weak_cell(&self, cell: GcRef<JsCell>) -> MarkWorkItem {
        MarkWorkItem {
            cell,
            reason: MarkReason::WeakValidation,
            dependency: MarkDependency::Weak,
            referrer: self.current_referrer,
        }
    }

    pub fn append_conservative_roots_descriptor(
        &self,
        roots: &ConservativeRoots,
        roots_heap: HeapId,
        roots_marking_epoch: HeapEpoch,
    ) -> Result<SlotVisitorConservativeRootAppendPlan, SlotVisitorConservativeRootAppendError> {
        if roots_heap != self.heap {
            return Err(SlotVisitorConservativeRootAppendError::HeapMismatch {
                visitor: self.heap,
                roots: roots_heap,
            });
        }

        if roots_marking_epoch != self.marking_epoch {
            return Err(
                SlotVisitorConservativeRootAppendError::MarkingEpochMismatch {
                    visitor: self.marking_epoch,
                    roots: roots_marking_epoch,
                },
            );
        }

        if self.root_mark_reason != RootMarkReason::ConservativeScan {
            return Err(
                SlotVisitorConservativeRootAppendError::InvalidRootMarkReason {
                    actual: self.root_mark_reason,
                },
            );
        }

        let mut records = Vec::with_capacity(roots.size());
        for (order, root) in roots.roots().iter().copied().enumerate() {
            if root.candidate_address == 0 || root.cell == CellId::default() {
                return Err(
                    SlotVisitorConservativeRootAppendError::InvalidConservativeRootCell(root),
                );
            }

            // C++ `appendJSCellOrAuxiliary` validates JSCell structure, runs
            // `testAndSetMarked`, greys JSCells, appends them to the mark
            // stack, and notes Auxiliary cells. Rust only emits descriptor
            // records until real heap cell storage, mark bits, JSCell versus
            // Auxiliary classification, and mark-stack mutation are ported.
            records.push(SlotVisitorConservativeRootAppendRecord {
                order,
                root,
                cell: root.cell,
                heap: self.heap,
                marking_epoch: self.marking_epoch,
                worklist: self.worklist,
                root_mark_reason: self.root_mark_reason,
                dependency: MarkDependency::Conservative,
                referrer: self.current_referrer,
            });
        }

        Ok(SlotVisitorConservativeRootAppendPlan {
            heap: self.heap,
            marking_epoch: self.marking_epoch,
            worklist: self.worklist,
            root_mark_reason: self.root_mark_reason,
            dependency: MarkDependency::Conservative,
            referrer: self.current_referrer,
            records,
        })
    }
}

/// Root visitation plan for a collection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RootMarkingPlan {
    pub precise_roots: Vec<RootRecord>,
    pub targeted_roots: Vec<TargetedRootRecord>,
    pub conservative_spans: Vec<ConservativeRootSpan>,
    pub conservative_cells: Vec<crate::gc::ConservativeRootCell>,
    pub source: ConservativeRootSource,
}

impl RootMarkingPlan {
    pub fn validate(&self) -> Result<(), RootPlanningError> {
        let mut planned_roots =
            Vec::with_capacity(self.precise_roots.len() + self.targeted_roots.len());
        for (index, root) in self.precise_roots.iter().enumerate() {
            if self.precise_roots[..index]
                .iter()
                .any(|previous| previous.id == root.id)
            {
                return Err(RootPlanningError::DuplicateRoot(root.id));
            }
            planned_roots.push(root.id);
        }

        for targeted_root in &self.targeted_roots {
            if planned_roots.contains(&targeted_root.root.id) {
                return Err(RootPlanningError::DuplicateRoot(targeted_root.root.id));
            }
            if targeted_root.target == CellId::default() {
                return Err(RootPlanningError::InvalidRootTarget {
                    root: targeted_root.root.id,
                    target: targeted_root.target,
                });
            }
            planned_roots.push(targeted_root.root.id);
        }

        for span in &self.conservative_spans {
            if span.begin >= span.end {
                return Err(RootPlanningError::InvalidConservativeSpan(*span));
            }
        }

        for root in &self.conservative_cells {
            if root.candidate_address == 0 || root.cell == CellId::default() {
                return Err(RootPlanningError::InvalidConservativeRootCell(*root));
            }
        }

        Ok(())
    }

    pub fn planned_steps(&self) -> Result<Vec<RootPlanStep>, RootPlanningError> {
        self.validate()?;
        let mut steps = Vec::with_capacity(
            self.precise_roots.len()
                + self.targeted_roots.len()
                + self.conservative_spans.len()
                + self.conservative_cells.len(),
        );

        for root in &self.precise_roots {
            steps.push(RootPlanStep::Precise {
                root: *root,
                reason: root_mark_reason_for_kind(root.kind),
            });
        }

        for targeted_root in &self.targeted_roots {
            steps.push(RootPlanStep::TargetedPrecise {
                root: targeted_root.root,
                target: targeted_root.target,
                reason: root_mark_reason_for_kind(targeted_root.root.kind),
            });
        }

        for span in &self.conservative_spans {
            steps.push(RootPlanStep::Conservative {
                span: *span,
                source: self.source,
            });
        }

        for root in &self.conservative_cells {
            steps.push(RootPlanStep::ConservativeCell {
                root: *root,
                source: self.source,
            });
        }

        Ok(steps)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootPlanStep {
    Precise {
        root: RootRecord,
        reason: RootMarkReason,
    },
    TargetedPrecise {
        root: RootRecord,
        target: CellId,
        reason: RootMarkReason,
    },
    Conservative {
        span: ConservativeRootSpan,
        source: ConservativeRootSource,
    },
    ConservativeCell {
        root: crate::gc::ConservativeRootCell,
        source: ConservativeRootSource,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootPlanningError {
    DuplicateRoot(crate::gc::RootId),
    InvalidRootTarget {
        root: crate::gc::RootId,
        target: CellId,
    },
    InvalidConservativeSpan(ConservativeRootSpan),
    InvalidConservativeRootCell(crate::gc::ConservativeRootCell),
}

pub fn root_mark_reason_for_kind(kind: crate::gc::RootKind) -> RootMarkReason {
    match kind {
        crate::gc::RootKind::Handle => RootMarkReason::StrongHandles,
        crate::gc::RootKind::ExplicitRoot => RootMarkReason::ProtectedValues,
        crate::gc::RootKind::VMRegister => RootMarkReason::StrongReferences,
        crate::gc::RootKind::Stack => RootMarkReason::ConservativeScan,
        crate::gc::RootKind::JitCode => RootMarkReason::JitStubRoutines,
        crate::gc::RootKind::Host => RootMarkReason::DomGcOutput,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{HeapId, RootId, RootKind};

    fn conservative_scan_visitor(heap: HeapId, epoch: HeapEpoch) -> SlotVisitorDescriptor {
        let mut visitor = SlotVisitorDescriptor::new(heap, "slot-visitor-test", epoch);
        visitor.worklist = MarkWorklistId(19);
        visitor.current_referrer = Some(ReferrerToken {
            kind: ReferrerTokenKind::RootMarkReason,
            address: 0,
            root_mark_reason: RootMarkReason::ConservativeScan,
        });
        visitor.root_mark_reason = RootMarkReason::ConservativeScan;
        visitor
    }

    fn conservative_roots(cells: &[(usize, CellId)]) -> ConservativeRoots {
        let mut roots = ConservativeRoots::new();
        for (candidate_address, cell) in cells {
            roots.add_validated_cell(ConservativeRootCell {
                candidate_address: *candidate_address,
                cell: *cell,
            });
        }
        roots
    }

    #[test]
    fn root_plan_orders_precise_roots_before_conservative_spans() {
        let plan = RootMarkingPlan {
            precise_roots: vec![RootRecord {
                id: RootId(1),
                kind: RootKind::Handle,
                heap: HeapId(7),
            }],
            targeted_roots: Vec::new(),
            conservative_spans: vec![ConservativeRootSpan {
                begin: 0x1000,
                end: 0x1010,
            }],
            conservative_cells: Vec::new(),
            source: ConservativeRootSource::MachineStack,
        };

        assert_eq!(
            plan.planned_steps(),
            Ok(vec![
                RootPlanStep::Precise {
                    root: RootRecord {
                        id: RootId(1),
                        kind: RootKind::Handle,
                        heap: HeapId(7)
                    },
                    reason: RootMarkReason::StrongHandles
                },
                RootPlanStep::Conservative {
                    span: ConservativeRootSpan {
                        begin: 0x1000,
                        end: 0x1010
                    },
                    source: ConservativeRootSource::MachineStack
                }
            ])
        );
    }

    #[test]
    fn root_plan_rejects_empty_conservative_span() {
        let plan = RootMarkingPlan {
            precise_roots: Vec::new(),
            targeted_roots: Vec::new(),
            conservative_spans: vec![ConservativeRootSpan {
                begin: 0x1000,
                end: 0x1000,
            }],
            conservative_cells: Vec::new(),
            source: ConservativeRootSource::Host,
        };

        assert_eq!(
            plan.validate(),
            Err(RootPlanningError::InvalidConservativeSpan(
                ConservativeRootSpan {
                    begin: 0x1000,
                    end: 0x1000
                }
            ))
        );
    }

    #[test]
    fn slot_visitor_append_conservative_roots_preserves_order_and_reason() {
        let heap = HeapId(7);
        let epoch = HeapEpoch(11);
        let visitor = conservative_scan_visitor(heap, epoch);
        let roots = conservative_roots(&[(0x1000, CellId(1)), (0x2000, CellId(2))]);

        let plan = visitor
            .append_conservative_roots_descriptor(&roots, heap, epoch)
            .expect("append descriptor");

        assert_eq!(plan.heap, heap);
        assert_eq!(plan.marking_epoch, epoch);
        assert_eq!(plan.worklist, MarkWorklistId(19));
        assert_eq!(plan.root_mark_reason, RootMarkReason::ConservativeScan);
        assert_eq!(plan.dependency, MarkDependency::Conservative);
        assert_eq!(plan.referrer, visitor.current_referrer);
        assert_eq!(
            plan.records,
            vec![
                SlotVisitorConservativeRootAppendRecord {
                    order: 0,
                    root: ConservativeRootCell {
                        candidate_address: 0x1000,
                        cell: CellId(1)
                    },
                    cell: CellId(1),
                    heap,
                    marking_epoch: epoch,
                    worklist: MarkWorklistId(19),
                    root_mark_reason: RootMarkReason::ConservativeScan,
                    dependency: MarkDependency::Conservative,
                    referrer: visitor.current_referrer
                },
                SlotVisitorConservativeRootAppendRecord {
                    order: 1,
                    root: ConservativeRootCell {
                        candidate_address: 0x2000,
                        cell: CellId(2)
                    },
                    cell: CellId(2),
                    heap,
                    marking_epoch: epoch,
                    worklist: MarkWorklistId(19),
                    root_mark_reason: RootMarkReason::ConservativeScan,
                    dependency: MarkDependency::Conservative,
                    referrer: visitor.current_referrer
                }
            ]
        );
    }

    #[test]
    fn slot_visitor_append_conservative_roots_accepts_empty_roots() {
        let heap = HeapId(7);
        let epoch = HeapEpoch(11);
        let visitor = conservative_scan_visitor(heap, epoch);

        let plan = visitor
            .append_conservative_roots_descriptor(&ConservativeRoots::new(), heap, epoch)
            .expect("append descriptor");

        assert!(plan.records.is_empty());
        assert_eq!(plan.root_mark_reason, RootMarkReason::ConservativeScan);
        assert_eq!(plan.dependency, MarkDependency::Conservative);
    }

    #[test]
    fn slot_visitor_append_conservative_roots_rejects_mismatch_or_wrong_reason() {
        let heap = HeapId(7);
        let epoch = HeapEpoch(11);
        let visitor = conservative_scan_visitor(heap, epoch);
        let roots = conservative_roots(&[(0x1000, CellId(1))]);

        assert_eq!(
            visitor.append_conservative_roots_descriptor(&roots, HeapId(8), epoch),
            Err(SlotVisitorConservativeRootAppendError::HeapMismatch {
                visitor: heap,
                roots: HeapId(8)
            })
        );
        assert_eq!(
            visitor.append_conservative_roots_descriptor(&roots, heap, HeapEpoch(12)),
            Err(
                SlotVisitorConservativeRootAppendError::MarkingEpochMismatch {
                    visitor: epoch,
                    roots: HeapEpoch(12)
                }
            )
        );

        let wrong_reason = SlotVisitorDescriptor::new(heap, "slot-visitor-test", epoch);
        assert_eq!(
            wrong_reason.append_conservative_roots_descriptor(&roots, heap, epoch),
            Err(
                SlotVisitorConservativeRootAppendError::InvalidRootMarkReason {
                    actual: RootMarkReason::None
                }
            )
        );
    }

    #[test]
    fn slot_visitor_conservative_cells_stay_distinct_from_precise_roots() {
        let heap = HeapId(7);
        let epoch = HeapEpoch(11);
        let visitor = conservative_scan_visitor(heap, epoch);
        let conservative_root = ConservativeRootCell {
            candidate_address: 0x3000,
            cell: CellId(9),
        };
        let roots =
            conservative_roots(&[(conservative_root.candidate_address, conservative_root.cell)]);

        let append_plan = visitor
            .append_conservative_roots_descriptor(&roots, heap, epoch)
            .expect("append descriptor");
        assert_eq!(append_plan.records[0].root, conservative_root);
        assert_eq!(append_plan.records[0].cell, CellId(9));

        let precise_root = RootRecord {
            id: RootId(1),
            kind: RootKind::Handle,
            heap,
        };
        let plan = RootMarkingPlan {
            precise_roots: vec![precise_root],
            targeted_roots: Vec::new(),
            conservative_spans: Vec::new(),
            conservative_cells: vec![conservative_root],
            source: ConservativeRootSource::MachineStack,
        };

        assert_eq!(
            plan.planned_steps(),
            Ok(vec![
                RootPlanStep::Precise {
                    root: precise_root,
                    reason: RootMarkReason::StrongHandles
                },
                RootPlanStep::ConservativeCell {
                    root: conservative_root,
                    source: ConservativeRootSource::MachineStack
                }
            ])
        );
    }
}
