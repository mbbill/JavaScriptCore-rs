//! Bytecode integration summaries for interpreter, tiering, GC, and tooling.
//!
//! These records compose existing code-block, source-map, root-map, profiling,
//! and tier metadata. They do not dispatch bytecode, schedule compilation, or
//! mutate VM state.

use crate::bytecode::{
    BytecodeIndex, BytecodeRootMapValidationError, CodeBlock, CodeBlockLifecycleState,
    ExecutionTier, SourceOriginSemanticValidationFinding, UnlinkedToLinkedBytecodeRecord,
    ValueProfileRootValidationError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeIntegrationSummary {
    pub link: UnlinkedToLinkedBytecodeRecord,
    pub tier: BytecodeTierExecutionSummary,
    pub roots: BytecodeRootIntegrationSummary,
    pub tooling: BytecodeToolingSourceMapSummary,
    pub diagnostics: Vec<BytecodeIntegrationDiagnostic>,
}

impl BytecodeIntegrationSummary {
    pub fn is_ready_for_interpreter(&self) -> bool {
        self.tier.has_interpreter_entry
            && self.link.linked_lifecycle == CodeBlockLifecycleState::LinkedInterpreter
            && !self
                .diagnostics
                .iter()
                .any(BytecodeIntegrationDiagnostic::blocks_interpreter_entry)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeTierExecutionSummary {
    pub current_tier: ExecutionTier,
    pub has_interpreter_entry: bool,
    pub has_baseline_entry: bool,
    pub has_optimizing_entry: bool,
    pub llint_threshold: Option<i32>,
    pub baseline_threshold: Option<i32>,
    pub optimizing_threshold: Option<i32>,
    pub llint_deferred: bool,
    pub baseline_deferred: bool,
    pub optimizing_deferred: bool,
    pub loop_osr_counter_count: u32,
    pub osr_exit_count: u32,
    pub did_fail_jit: bool,
    pub did_fail_ftl: bool,
}

impl BytecodeTierExecutionSummary {
    fn from_code_block(code_block: &CodeBlock) -> Self {
        let tier_state = code_block.tier_state();
        let entrypoints = code_block.entrypoints();
        Self {
            current_tier: tier_state.current_tier,
            has_interpreter_entry: entrypoints.interpreter.is_some(),
            has_baseline_entry: entrypoints.baseline_jit.is_some(),
            has_optimizing_entry: entrypoints.optimizing_jit.is_some(),
            llint_threshold: tier_state.llint_counter.threshold,
            baseline_threshold: tier_state.baseline_counter.threshold,
            optimizing_threshold: tier_state.optimizing_counter.threshold,
            llint_deferred: tier_state.llint_counter.deferred,
            baseline_deferred: tier_state.baseline_counter.deferred,
            optimizing_deferred: tier_state.optimizing_counter.deferred,
            loop_osr_counter_count: tier_state.profiling_counters.loop_osr.len() as u32,
            osr_exit_count: tier_state.osr_exit_count,
            did_fail_jit: tier_state.did_fail_jit,
            did_fail_ftl: tier_state.did_fail_ftl,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeRootIntegrationSummary {
    pub root_map_count: u32,
    pub complete_root_map_count: u32,
    pub precise_root_slot_count: u32,
    pub imprecise_root_slot_count: u32,
    pub value_profile_root_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeToolingSourceMapSummary {
    pub source_note_count: u32,
    pub code_origin_pc_mapping_count: u32,
    pub bytecode_source_mapping_count: u32,
    pub semantic_source_mapping_count: u32,
    pub debugger_hook_count: u32,
    pub profiler_hook_count: u32,
    pub type_profiler_range_count: u32,
    pub control_flow_profile_offset_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeIntegrationDiagnostic {
    pub kind: BytecodeIntegrationDiagnosticKind,
    pub bytecode_index: Option<BytecodeIndex>,
}

impl BytecodeIntegrationDiagnostic {
    pub const fn new(
        kind: BytecodeIntegrationDiagnosticKind,
        bytecode_index: Option<BytecodeIndex>,
    ) -> Self {
        Self {
            kind,
            bytecode_index,
        }
    }

    pub fn blocks_interpreter_entry(&self) -> bool {
        matches!(
            self.kind,
            BytecodeIntegrationDiagnosticKind::MissingInterpreterEntry
                | BytecodeIntegrationDiagnosticKind::RootMapInvalid(_)
                | BytecodeIntegrationDiagnosticKind::ValueProfileRootInvalid(_)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BytecodeIntegrationDiagnosticKind {
    MissingInterpreterEntry,
    LifecycleBeforeInterpreterLink(CodeBlockLifecycleState),
    TierEntrypointMissing { tier: ExecutionTier },
    RootMapInvalid(BytecodeRootMapValidationError),
    ValueProfileRootInvalid(ValueProfileRootValidationError),
    SourceSemanticMapInvalid(SourceOriginSemanticValidationFinding),
}

pub fn summarize_bytecode_integration(code_block: &CodeBlock) -> BytecodeIntegrationSummary {
    let link = code_block.link_record();
    let tier = BytecodeTierExecutionSummary::from_code_block(code_block);
    let roots = summarize_roots(code_block);
    let tooling = summarize_tooling(code_block);
    let mut diagnostics = Vec::new();

    if !tier.has_interpreter_entry {
        diagnostics.push(BytecodeIntegrationDiagnostic::new(
            BytecodeIntegrationDiagnosticKind::MissingInterpreterEntry,
            None,
        ));
    }
    if code_block.lifecycle() == CodeBlockLifecycleState::Linking {
        diagnostics.push(BytecodeIntegrationDiagnostic::new(
            BytecodeIntegrationDiagnosticKind::LifecycleBeforeInterpreterLink(
                code_block.lifecycle(),
            ),
            None,
        ));
    }
    if matches!(tier.current_tier, ExecutionTier::BaselineJit) && !tier.has_baseline_entry {
        diagnostics.push(BytecodeIntegrationDiagnostic::new(
            BytecodeIntegrationDiagnosticKind::TierEntrypointMissing {
                tier: ExecutionTier::BaselineJit,
            },
            None,
        ));
    }
    if matches!(
        tier.current_tier,
        ExecutionTier::DfgJit | ExecutionTier::FtlJit
    ) && !tier.has_optimizing_entry
    {
        diagnostics.push(BytecodeIntegrationDiagnostic::new(
            BytecodeIntegrationDiagnosticKind::TierEntrypointMissing {
                tier: tier.current_tier,
            },
            None,
        ));
    }

    for root_map in &code_block.side_tables().root_maps {
        if let Err(error) = root_map.validate() {
            let bytecode_index = match error {
                BytecodeRootMapValidationError::SlotOutsideRange(index) => Some(index),
                BytecodeRootMapValidationError::DuplicateSlot { bytecode_index, .. } => {
                    Some(bytecode_index)
                }
                _ => None,
            };
            diagnostics.push(BytecodeIntegrationDiagnostic::new(
                BytecodeIntegrationDiagnosticKind::RootMapInvalid(error),
                bytecode_index,
            ));
        }
    }

    if let Err(error) = code_block
        .side_tables()
        .value_profiles
        .validate_root_metadata()
    {
        let bytecode_index = match error {
            ValueProfileRootValidationError::ProfileRootMissingProfile {
                bytecode_index, ..
            } => Some(bytecode_index),
            _ => None,
        };
        diagnostics.push(BytecodeIntegrationDiagnostic::new(
            BytecodeIntegrationDiagnosticKind::ValueProfileRootInvalid(error),
            bytecode_index,
        ));
    }

    for finding in code_block
        .side_tables()
        .code_origins
        .semantic_mappings
        .validate()
        .findings
    {
        let bytecode_index = match finding {
            SourceOriginSemanticValidationFinding::DuplicateBytecodeIndex { bytecode_index } => {
                Some(bytecode_index)
            }
            _ => None,
        };
        diagnostics.push(BytecodeIntegrationDiagnostic::new(
            BytecodeIntegrationDiagnosticKind::SourceSemanticMapInvalid(finding),
            bytecode_index,
        ));
    }

    BytecodeIntegrationSummary {
        link,
        tier,
        roots,
        tooling,
        diagnostics,
    }
}

fn summarize_roots(code_block: &CodeBlock) -> BytecodeRootIntegrationSummary {
    let root_maps = &code_block.side_tables().root_maps;
    BytecodeRootIntegrationSummary {
        root_map_count: root_maps.len() as u32,
        complete_root_map_count: root_maps.iter().filter(|map| map.complete).count() as u32,
        precise_root_slot_count: root_maps
            .iter()
            .flat_map(|map| &map.slots)
            .filter(|slot| slot.precise)
            .count() as u32,
        imprecise_root_slot_count: root_maps
            .iter()
            .flat_map(|map| &map.slots)
            .filter(|slot| !slot.precise)
            .count() as u32,
        value_profile_root_count: code_block.side_tables().value_profiles.root_metadata.len()
            as u32,
    }
}

fn summarize_tooling(code_block: &CodeBlock) -> BytecodeToolingSourceMapSummary {
    let unlinked_side_tables = code_block.unlinked().side_tables();
    let linked_side_tables = code_block.side_tables();
    BytecodeToolingSourceMapSummary {
        source_note_count: unlinked_side_tables.source_notes.notes.len() as u32,
        code_origin_pc_mapping_count: linked_side_tables.code_origins.pc_mappings.len() as u32,
        bytecode_source_mapping_count: linked_side_tables.code_origins.source_mappings.len() as u32,
        semantic_source_mapping_count: linked_side_tables
            .code_origins
            .semantic_mappings
            .entries
            .len() as u32,
        debugger_hook_count: linked_side_tables.bytecode_hooks.debugger_hooks.len() as u32,
        profiler_hook_count: linked_side_tables.bytecode_hooks.profiler_hooks.len() as u32,
        type_profiler_range_count: unlinked_side_tables.type_profiler_ranges.len() as u32,
        control_flow_profile_offset_count: unlinked_side_tables.control_flow_profile_offsets.len()
            as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{
        BytecodeRootMap, BytecodeRootMapId, BytecodeRootSlotDescriptor, BytecodeRootSlotKind,
        CodeBlockEntrypoints, CodeBlockTierState, CodeKind, InstructionBuilder,
        InterpreterEntrySlot, LinkContext, LinkedSideTables, Opcode, OperandWidth, SourceNote,
        SourceNoteTable, TierCounterState, UnlinkedCodeBlock, UnlinkedCodeBlockPhase,
        UnlinkedSideTables, ValueProfile, ValueProfileBucket, ValueProfileBucketKind,
        ValueProfileRootMetadata, ValueProfileTable, VirtualRegister,
    };
    use crate::gc::CellId;
    use crate::runtime::CodeBlockId;

    #[test]
    fn bytecode_integration_summary_reports_tiers_roots_and_tooling() {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        let bytecode_index = BytecodeIndex::from_offset(0);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, builder.finalize())
            .with_side_tables(UnlinkedSideTables {
                source_notes: SourceNoteTable {
                    notes: vec![SourceNote {
                        bytecode_index,
                        divot: 5,
                        start_offset_from_divot: 1,
                        end_offset_from_divot: 2,
                        line: 1,
                        column: 5,
                    }],
                    ..SourceNoteTable::default()
                },
                ..UnlinkedSideTables::default()
            })
            .with_phase(UnlinkedCodeBlockPhase::Finalized);
        let root_slot = BytecodeRootSlotDescriptor::virtual_register(
            bytecode_index,
            VirtualRegister::from_raw(1),
            BytecodeRootSlotKind::VirtualRegister,
        );
        let profile_slot = crate::bytecode::code_block::RuntimeSlot(3);
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_side_tables(LinkedSideTables {
                root_maps: vec![BytecodeRootMap {
                    id: BytecodeRootMapId(1),
                    owner: Some(CodeBlockId(CellId(7))),
                    bytecode_range_start: bytecode_index,
                    bytecode_range_end: bytecode_index,
                    slots: vec![root_slot],
                    complete: true,
                }],
                value_profiles: ValueProfileTable {
                    profiles: vec![ValueProfile {
                        bytecode_index,
                        checkpoint: crate::bytecode::Checkpoint::NONE,
                        operand: None,
                        buckets: vec![ValueProfileBucket {
                            slot: profile_slot,
                            kind: ValueProfileBucketKind::Sample,
                        }],
                        prediction: crate::bytecode::SpeculatedTypeSet(0),
                        update_policy: crate::bytecode::ProfileUpdatePolicy::ConcurrentBuckets,
                    }],
                    root_metadata: vec![ValueProfileRootMetadata::for_profile_slot(
                        bytecode_index,
                        profile_slot,
                        Some(BytecodeRootMapId(1)),
                        crate::bytecode::ProfileUpdatePolicy::ConcurrentBuckets,
                    )],
                    ..ValueProfileTable::default()
                },
                ..LinkedSideTables::default()
            })
            .with_entrypoints(CodeBlockEntrypoints {
                interpreter: Some(InterpreterEntrySlot(0)),
                ..CodeBlockEntrypoints::default()
            })
            .with_tier_state(CodeBlockTierState {
                llint_counter: TierCounterState {
                    threshold: Some(10),
                    deferred: false,
                },
                ..CodeBlockTierState::default()
            })
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter);

        let summary = summarize_bytecode_integration(&code_block);

        assert!(summary.is_ready_for_interpreter());
        assert_eq!(summary.link.instruction_count, 1);
        assert_eq!(summary.roots.root_map_count, 1);
        assert_eq!(summary.roots.precise_root_slot_count, 1);
        assert_eq!(summary.roots.value_profile_root_count, 1);
        assert_eq!(summary.tooling.source_note_count, 1);
        assert_eq!(summary.tier.llint_threshold, Some(10));
        assert!(summary.diagnostics.is_empty());
    }

    #[test]
    fn bytecode_integration_diagnostics_report_missing_entry_and_invalid_roots() {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        let bytecode_index = BytecodeIndex::from_offset(0);
        let bad_slot = BytecodeRootSlotDescriptor::virtual_register(
            bytecode_index,
            VirtualRegister::from_raw(1),
            BytecodeRootSlotKind::VirtualRegister,
        );
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, builder.finalize())
            .with_phase(UnlinkedCodeBlockPhase::Finalized);
        let code_block = CodeBlock::from_unlinked(unlinked, LinkContext::default())
            .with_side_tables(LinkedSideTables {
                root_maps: vec![BytecodeRootMap {
                    id: BytecodeRootMapId(1),
                    owner: None,
                    bytecode_range_start: bytecode_index,
                    bytecode_range_end: bytecode_index,
                    slots: vec![bad_slot, bad_slot],
                    complete: true,
                }],
                ..LinkedSideTables::default()
            })
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter);

        let summary = summarize_bytecode_integration(&code_block);

        assert!(!summary.is_ready_for_interpreter());
        assert!(summary.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            BytecodeIntegrationDiagnosticKind::MissingInterpreterEntry
        )));
        assert!(summary.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            BytecodeIntegrationDiagnosticKind::RootMapInvalid(
                BytecodeRootMapValidationError::DuplicateSlot { .. }
            )
        )));
    }
}
