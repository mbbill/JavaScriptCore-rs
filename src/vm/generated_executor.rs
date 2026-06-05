//! VM-owned generated CodeBlock entry execution.
//!
//! C++ JSC prepares a `CodeBlock` through
//! `ScriptExecutable::prepareForExecutionImpl`, installs LLInt/Baseline
//! `JITCode`, and the interpreter enters through `JITCode::addressForCall`
//! (`Interpreter::executeProgram` / `executeCallImpl`). Rust's
//! baseline-generated code still routes through a bytecode executor shim while
//! the real register-allocated baseline and optimizing tiers are missing, so
//! this module owns that temporary CodeBlock/JITCode entry boundary: artifact
//! validation, sidecar dispatch, metrics replay, fallback completion, and the
//! no-GC/rooting handoff back into the VM.

use super::*;

impl Vm {
    pub(super) fn execute_baseline_generated_code_in_current_region<H: DispatchHost>(
        &mut self,
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        expected_frame: Option<crate::runtime::CallFrameId>,
        entry_kind: crate::interpreter::ExecutionEntryKind,
        current_tier: JitType,
        host: &mut H,
        config: DispatchConfig,
    ) -> ExecutionCompletion {
        self.execute_baseline_generated_code_in_current_region_with_entry_context(
            code_block_id,
            code_block,
            expected_frame,
            entry_kind,
            current_tier,
            code_block_id,
            entry_kind,
            current_tier,
            host,
            config,
            BaselineGeneratedExecutionValidation::Full,
        )
    }

    pub(super) fn execute_baseline_generated_code_in_current_region_with_entry_context<
        H: DispatchHost,
    >(
        &mut self,
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        expected_frame: Option<crate::runtime::CallFrameId>,
        entry_kind: crate::interpreter::ExecutionEntryKind,
        current_tier: JitType,
        resume_context_owner: CodeBlockId,
        resume_entry_kind: crate::interpreter::ExecutionEntryKind,
        resume_current_tier: JitType,
        host: &mut H,
        config: DispatchConfig,
        validation: BaselineGeneratedExecutionValidation,
    ) -> ExecutionCompletion {
        let control = self.execute_baseline_generated_code_once_in_current_region(
            code_block_id,
            code_block,
            expected_frame,
            entry_kind,
            current_tier,
            host,
            config,
            validation,
        );
        self.drive_generated_execution_control_with_entry_context(
            control,
            code_block,
            Some((resume_context_owner, resume_entry_kind, resume_current_tier)),
            host,
            config,
        )
    }

    pub(super) fn execute_baseline_generated_code_once_in_current_region<H: DispatchHost>(
        &mut self,
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        expected_frame: Option<crate::runtime::CallFrameId>,
        entry_kind: crate::interpreter::ExecutionEntryKind,
        current_tier: JitType,
        host: &mut H,
        config: DispatchConfig,
        validation: BaselineGeneratedExecutionValidation,
    ) -> GeneratedExecutionControl {
        self.drain_pending_property_load_runtime_invalidations(host);
        if !self.config.baseline_generated_execution_enabled() {
            // C++ JSC enters LLInt or real JITCode at this boundary. When the
            // diagnostic policy disables Rust's temporary bytecode shim, callers
            // with interpreter continuations must decline before reaching here;
            // direct/generated helper callers fail closed.
            return GeneratedExecutionControl::failed(
                ExecutionError::BaselineGeneratedExecutionRejected,
            );
        }
        let Some(artifact) = self
            .tiering
            .baseline_generated_code_artifact_for(code_block_id)
        else {
            return GeneratedExecutionControl::failed(
                ExecutionError::BaselineGeneratedCodeUnavailable,
            );
        };
        let Some(expected_frame) = expected_frame else {
            return GeneratedExecutionControl::failed(ExecutionError::NoActiveFrame);
        };

        if let Some(runtime_helper_plan) = artifact.runtime_helper_plan() {
            let bytecode_snapshot = artifact.eligibility_proof.bytecode_snapshot_fingerprint();
            let generated_call_link_candidate_table = match self
                .generated_call_link_candidate_table_for_owner_cached(
                    code_block_id,
                    bytecode_snapshot,
                ) {
                Ok(table) => table,
                Err(_) => {
                    return GeneratedExecutionControl::failed(
                        ExecutionError::BaselineGeneratedExecutionRejected,
                    )
                }
            };
            let GeneratedPropertySidecarProjection {
                property_load_plan_table,
                guarded_candidate_table,
                megamorphic_load_candidate_table,
                store_candidate_table,
                megamorphic_store_candidate_table,
                megamorphic_has_candidate_table,
            } = match self.generated_property_sidecar_projection_for_owner_cached(
                code_block_id,
                bytecode_snapshot,
            ) {
                Ok(projection) => projection,
                Err(_) => {
                    return GeneratedExecutionControl::failed(
                        ExecutionError::BaselineGeneratedExecutionRejected,
                    )
                }
            };
            let _call_link_attachment_plan_table = match self
                .tiering
                .call_link_attachment_plan_table_for_owner(code_block_id, bytecode_snapshot)
            {
                Ok(table) => table,
                Err(_) => {
                    return GeneratedExecutionControl::failed(
                        ExecutionError::BaselineGeneratedExecutionRejected,
                    )
                }
            };
            let generated_direct_call_hot_slots =
                self.generated_direct_call_hot_slot_projections_for_owner(code_block_id);
            if !property_load_plan_table.is_empty()
                || !guarded_candidate_table.is_empty()
                || !megamorphic_load_candidate_table.is_empty()
                || !store_candidate_table.is_empty()
                || !megamorphic_store_candidate_table.is_empty()
                || !megamorphic_has_candidate_table.is_empty()
            {
                let (
                    result,
                    destination_root_sync_requests,
                    probe_miss_records,
                    guarded_probe_miss_records,
                    store_probe_miss_records,
                    store_mutation_rejection_records,
                    call_link_probe_miss_records,
                    call_link_probe_blocked_records,
                    direct_call_hot_slot_hits,
                    metrics,
                ) = {
                    let mut sidecars = if generated_call_link_candidate_table.is_empty() {
                        BaselineGeneratedPropertyExecutionSidecars::new(
                            host,
                            Some((&property_load_plan_table, &guarded_candidate_table)),
                            Some(&store_candidate_table),
                        )
                    } else {
                        BaselineGeneratedPropertyExecutionSidecars::new_with_generated_call_link_and_direct_call_hot_slots(
                            host,
                            Some((&property_load_plan_table, &guarded_candidate_table)),
                            Some(&store_candidate_table),
                            &generated_call_link_candidate_table,
                            &generated_direct_call_hot_slots,
                        )
                    }
                    .with_property_load_megamorphic_candidate_table(Some(
                        &megamorphic_load_candidate_table,
                    ))
                    .with_property_store_megamorphic_candidate_table(Some(
                        &megamorphic_store_candidate_table,
                    ))
                    .with_property_has_megamorphic_candidate_table(Some(
                        &megamorphic_has_candidate_table,
                    ));
                    let mut metrics =
                        BaselineGeneratedExecutionMetrics::for_dispatch_config(config);
                    let mut local_dispatch_budget = DispatchBudget::from_config(config);
                    let dispatch_budget = self
                        .active_dispatch_budget
                        .as_mut()
                        .unwrap_or(&mut local_dispatch_budget);
                    let result =
                        execute_baseline_generated_code_with_runtime_helpers_and_sidecars_and_metrics_and_validation_and_dispatch_budget(
                            BaselineGeneratedExecutionRequest {
                                artifact: &artifact,
                                owner: code_block_id,
                                code_block,
                                expected_frame,
                                execution: InterpreterExecutionState {
                                    stack: &mut self.execution,
                                    registers: &mut self.registers,
                                    exceptions: &mut self.exceptions,
                                    heap: &mut self.heap,
                                },
                            },
                            runtime_helper_plan,
                            Some(&mut sidecars),
                            None,
                            &mut metrics,
                            validation,
                            dispatch_budget,
                        );
                    (
                        result,
                        sidecars.destination_root_sync_requests().to_vec(),
                        sidecars.property_load_probe_miss_records().to_vec(),
                        sidecars.guarded_property_load_probe_miss_records().to_vec(),
                        sidecars.property_store_probe_miss_records().to_vec(),
                        sidecars
                            .property_store_mutation_rejection_records()
                            .to_vec(),
                        sidecars.generated_call_link_probe_miss_records().to_vec(),
                        sidecars
                            .generated_call_link_probe_blocked_records()
                            .to_vec(),
                        sidecars.generated_direct_call_hot_slot_hits(),
                        metrics,
                    )
                };
                let outcome =
                    Self::baseline_generated_execution_with_runtime_helpers_outcome(&result);
                self.record_baseline_generated_execution_metrics(
                    code_block_id,
                    bytecode_snapshot,
                    entry_kind,
                    current_tier,
                    metrics,
                    outcome,
                );
                self.record_generated_property_load_sidecar_probe_miss_records(
                    bytecode_snapshot,
                    &probe_miss_records,
                    &guarded_probe_miss_records,
                );
                self.record_generated_property_store_sidecar_probe_miss_records(
                    bytecode_snapshot,
                    &store_probe_miss_records,
                );
                for rejection in store_mutation_rejection_records {
                    self.tiering
                        .record_generated_property_store_mutation_rejection(
                            VmGeneratedPropertyStoreMutationRejectionRequest {
                                owner: rejection.owner,
                                bytecode_index: rejection.bytecode_index,
                                bytecode_snapshot,
                                slot: rejection.slot,
                                store_plan_ordinal: rejection.store_plan_ordinal,
                                readiness_ordinal: rejection.readiness_ordinal,
                                key: rejection.key,
                                plan_kind: rejection.plan_kind,
                                base_structure: rejection.base_structure,
                                planned_new_structure: rejection.planned_new_structure,
                                planned_offset: rejection.offset,
                                stored_value_kind: rejection.stored_value_kind,
                                reason: rejection.reason,
                            },
                        );
                }
                self.record_generated_call_link_probe_miss_records(
                    bytecode_snapshot,
                    &call_link_probe_miss_records,
                );
                self.record_generated_call_link_probe_blocked_records(
                    bytecode_snapshot,
                    &call_link_probe_blocked_records,
                );
                self.tiering
                    .record_generated_direct_call_sidecar_hot_slot_hits(direct_call_hot_slot_hits);
                let mut active_destination_roots = Vec::new();
                let control = match self.sync_generated_property_load_destination_roots(
                    host,
                    &destination_root_sync_requests,
                    &mut active_destination_roots,
                ) {
                    Ok(()) => self.finish_baseline_generated_with_runtime_helpers_result_control(
                        result,
                        code_block_id,
                        entry_kind,
                        current_tier,
                        code_block,
                        host,
                        config,
                    ),
                    Err(error) => GeneratedExecutionControl::failed(error),
                };
                return self.finish_generated_property_load_destination_root_control(
                    &mut active_destination_roots,
                    control,
                );
            }
            if !generated_call_link_candidate_table.is_empty() {
                let (
                    result,
                    call_link_probe_miss_records,
                    call_link_probe_blocked_records,
                    direct_call_hot_slot_hits,
                    metrics,
                ) = {
                    let mut sidecar =
                        BaselineGeneratedCallLinkExecutionSidecar::new_with_direct_call_hot_slots(
                            &generated_call_link_candidate_table,
                            &generated_direct_call_hot_slots,
                            host,
                        );
                    let mut metrics =
                        BaselineGeneratedExecutionMetrics::for_dispatch_config(config);
                    let mut local_dispatch_budget = DispatchBudget::from_config(config);
                    let dispatch_budget = self
                        .active_dispatch_budget
                        .as_mut()
                        .unwrap_or(&mut local_dispatch_budget);
                    let result =
                        execute_baseline_generated_code_with_runtime_helpers_and_sidecars_and_metrics_and_validation_and_dispatch_budget(
                            BaselineGeneratedExecutionRequest {
                                artifact: &artifact,
                                owner: code_block_id,
                                code_block,
                                expected_frame,
                                execution: InterpreterExecutionState {
                                    stack: &mut self.execution,
                                    registers: &mut self.registers,
                                    exceptions: &mut self.exceptions,
                                    heap: &mut self.heap,
                                },
                            },
                            runtime_helper_plan,
                            None,
                            Some(&mut sidecar),
                            &mut metrics,
                            validation,
                            dispatch_budget,
                        );
                    (
                        result,
                        sidecar.probe_miss_records().to_vec(),
                        sidecar.probe_blocked_records().to_vec(),
                        sidecar.direct_call_hot_slot_hits(),
                        metrics,
                    )
                };
                let outcome =
                    Self::baseline_generated_execution_with_runtime_helpers_outcome(&result);
                self.record_baseline_generated_execution_metrics(
                    code_block_id,
                    bytecode_snapshot,
                    entry_kind,
                    current_tier,
                    metrics,
                    outcome,
                );
                self.record_generated_call_link_probe_miss_records(
                    bytecode_snapshot,
                    &call_link_probe_miss_records,
                );
                self.record_generated_call_link_probe_blocked_records(
                    bytecode_snapshot,
                    &call_link_probe_blocked_records,
                );
                self.tiering
                    .record_generated_direct_call_sidecar_hot_slot_hits(direct_call_hot_slot_hits);
                return self.finish_baseline_generated_with_runtime_helpers_result_control(
                    result,
                    code_block_id,
                    entry_kind,
                    current_tier,
                    code_block,
                    host,
                    config,
                );
            }

            let mut metrics = BaselineGeneratedExecutionMetrics::for_dispatch_config(config);
            let mut local_dispatch_budget = DispatchBudget::from_config(config);
            let dispatch_budget = self
                .active_dispatch_budget
                .as_mut()
                .unwrap_or(&mut local_dispatch_budget);
            let result =
                execute_baseline_generated_code_with_runtime_helpers_and_metrics_and_validation_and_dispatch_budget(
                    BaselineGeneratedExecutionRequest {
                        artifact: &artifact,
                        owner: code_block_id,
                        code_block,
                        expected_frame,
                        execution: InterpreterExecutionState {
                            stack: &mut self.execution,
                            registers: &mut self.registers,
                            exceptions: &mut self.exceptions,
                            heap: &mut self.heap,
                        },
                    },
                    runtime_helper_plan,
                    &mut metrics,
                    validation,
                    dispatch_budget,
                );
            let outcome = Self::baseline_generated_execution_with_runtime_helpers_outcome(&result);
            self.record_baseline_generated_execution_metrics(
                code_block_id,
                bytecode_snapshot,
                entry_kind,
                current_tier,
                metrics,
                outcome,
            );
            self.finish_baseline_generated_with_runtime_helpers_result_control(
                result,
                code_block_id,
                entry_kind,
                current_tier,
                code_block,
                host,
                config,
            )
        } else {
            let bytecode_snapshot = artifact.eligibility_proof.bytecode_snapshot_fingerprint();
            let generated_call_link_candidate_table = match self
                .generated_call_link_candidate_table_for_owner_cached(
                    code_block_id,
                    bytecode_snapshot,
                ) {
                Ok(table) => table,
                Err(_) => {
                    return GeneratedExecutionControl::failed(
                        ExecutionError::BaselineGeneratedExecutionRejected,
                    )
                }
            };
            let GeneratedPropertySidecarProjection {
                property_load_plan_table,
                guarded_candidate_table,
                megamorphic_load_candidate_table,
                store_candidate_table,
                megamorphic_store_candidate_table,
                megamorphic_has_candidate_table,
            } = match self.generated_property_sidecar_projection_for_owner_cached(
                code_block_id,
                bytecode_snapshot,
            ) {
                Ok(projection) => projection,
                Err(_) => {
                    return GeneratedExecutionControl::failed(
                        ExecutionError::BaselineGeneratedExecutionRejected,
                    )
                }
            };
            let _call_link_attachment_plan_table = match self
                .tiering
                .call_link_attachment_plan_table_for_owner(code_block_id, bytecode_snapshot)
            {
                Ok(table) => table,
                Err(_) => {
                    return GeneratedExecutionControl::failed(
                        ExecutionError::BaselineGeneratedExecutionRejected,
                    )
                }
            };
            let generated_direct_call_hot_slots =
                self.generated_direct_call_hot_slot_projections_for_owner(code_block_id);
            if !property_load_plan_table.is_empty()
                || !guarded_candidate_table.is_empty()
                || !megamorphic_load_candidate_table.is_empty()
                || !store_candidate_table.is_empty()
                || !megamorphic_store_candidate_table.is_empty()
                || !megamorphic_has_candidate_table.is_empty()
            {
                let (
                    result,
                    destination_root_sync_requests,
                    probe_miss_records,
                    guarded_probe_miss_records,
                    store_probe_miss_records,
                    store_mutation_rejection_records,
                    call_link_probe_miss_records,
                    call_link_probe_blocked_records,
                    direct_call_hot_slot_hits,
                    metrics,
                ) = {
                    let mut sidecars = if generated_call_link_candidate_table.is_empty() {
                        BaselineGeneratedPropertyExecutionSidecars::new(
                            host,
                            Some((&property_load_plan_table, &guarded_candidate_table)),
                            Some(&store_candidate_table),
                        )
                    } else {
                        BaselineGeneratedPropertyExecutionSidecars::new_with_generated_call_link_and_direct_call_hot_slots(
                            host,
                            Some((&property_load_plan_table, &guarded_candidate_table)),
                            Some(&store_candidate_table),
                            &generated_call_link_candidate_table,
                            &generated_direct_call_hot_slots,
                        )
                    }
                    .with_property_load_megamorphic_candidate_table(Some(
                        &megamorphic_load_candidate_table,
                    ))
                    .with_property_store_megamorphic_candidate_table(Some(
                        &megamorphic_store_candidate_table,
                    ))
                    .with_property_has_megamorphic_candidate_table(Some(
                        &megamorphic_has_candidate_table,
                    ));
                    let mut metrics =
                        BaselineGeneratedExecutionMetrics::for_dispatch_config(config);
                    let mut local_dispatch_budget = DispatchBudget::from_config(config);
                    let dispatch_budget = self
                        .active_dispatch_budget
                        .as_mut()
                        .unwrap_or(&mut local_dispatch_budget);
                    let result = execute_baseline_generated_code_with_property_sidecars_and_metrics_and_validation_and_dispatch_budget(
                        BaselineGeneratedExecutionRequest {
                            artifact: &artifact,
                            owner: code_block_id,
                            code_block,
                            expected_frame,
                            execution: InterpreterExecutionState {
                                stack: &mut self.execution,
                                registers: &mut self.registers,
                                exceptions: &mut self.exceptions,
                                heap: &mut self.heap,
                            },
                        },
                        &mut sidecars,
                        &mut metrics,
                        validation,
                        dispatch_budget,
                    );
                    (
                        result,
                        sidecars.destination_root_sync_requests().to_vec(),
                        sidecars.property_load_probe_miss_records().to_vec(),
                        sidecars.guarded_property_load_probe_miss_records().to_vec(),
                        sidecars.property_store_probe_miss_records().to_vec(),
                        sidecars
                            .property_store_mutation_rejection_records()
                            .to_vec(),
                        sidecars.generated_call_link_probe_miss_records().to_vec(),
                        sidecars
                            .generated_call_link_probe_blocked_records()
                            .to_vec(),
                        sidecars.generated_direct_call_hot_slot_hits(),
                        metrics,
                    )
                };
                let outcome = Self::baseline_generated_execution_outcome(&result);
                self.record_baseline_generated_execution_metrics(
                    code_block_id,
                    bytecode_snapshot,
                    entry_kind,
                    current_tier,
                    metrics,
                    outcome,
                );
                self.record_generated_property_load_sidecar_probe_miss_records(
                    bytecode_snapshot,
                    &probe_miss_records,
                    &guarded_probe_miss_records,
                );
                self.record_generated_property_store_sidecar_probe_miss_records(
                    bytecode_snapshot,
                    &store_probe_miss_records,
                );
                for rejection in store_mutation_rejection_records {
                    self.tiering
                        .record_generated_property_store_mutation_rejection(
                            VmGeneratedPropertyStoreMutationRejectionRequest {
                                owner: rejection.owner,
                                bytecode_index: rejection.bytecode_index,
                                bytecode_snapshot,
                                slot: rejection.slot,
                                store_plan_ordinal: rejection.store_plan_ordinal,
                                readiness_ordinal: rejection.readiness_ordinal,
                                key: rejection.key,
                                plan_kind: rejection.plan_kind,
                                base_structure: rejection.base_structure,
                                planned_new_structure: rejection.planned_new_structure,
                                planned_offset: rejection.offset,
                                stored_value_kind: rejection.stored_value_kind,
                                reason: rejection.reason,
                            },
                        );
                }
                self.record_generated_call_link_probe_miss_records(
                    bytecode_snapshot,
                    &call_link_probe_miss_records,
                );
                self.record_generated_call_link_probe_blocked_records(
                    bytecode_snapshot,
                    &call_link_probe_blocked_records,
                );
                self.tiering
                    .record_generated_direct_call_sidecar_hot_slot_hits(direct_call_hot_slot_hits);
                let mut active_destination_roots = Vec::new();
                let control = match result {
                    Ok(result) => match self.sync_generated_property_load_destination_roots(
                        host,
                        &destination_root_sync_requests,
                        &mut active_destination_roots,
                    ) {
                        Ok(()) => self.finish_baseline_generated_result_control(
                            Ok(result),
                            code_block_id,
                            entry_kind,
                            current_tier,
                            code_block,
                            host,
                            config,
                        ),
                        Err(error) => GeneratedExecutionControl::failed(error),
                    },
                    Err(BaselineGeneratedExecutionError::Execution(error)) => {
                        GeneratedExecutionControl::failed(error)
                    }
                    Err(error) => {
                        let _ = error;
                        GeneratedExecutionControl::failed(
                            ExecutionError::BaselineGeneratedExecutionRejected,
                        )
                    }
                };
                return self.finish_generated_property_load_destination_root_control(
                    &mut active_destination_roots,
                    control,
                );
            }
            if !generated_call_link_candidate_table.is_empty() {
                let (
                    result,
                    call_link_probe_miss_records,
                    call_link_probe_blocked_records,
                    direct_call_hot_slot_hits,
                    metrics,
                ) = {
                    let mut sidecar =
                        BaselineGeneratedCallLinkExecutionSidecar::new_with_direct_call_hot_slots(
                            &generated_call_link_candidate_table,
                            &generated_direct_call_hot_slots,
                            host,
                        );
                    let mut metrics =
                        BaselineGeneratedExecutionMetrics::for_dispatch_config(config);
                    let mut local_dispatch_budget = DispatchBudget::from_config(config);
                    let dispatch_budget = self
                        .active_dispatch_budget
                        .as_mut()
                        .unwrap_or(&mut local_dispatch_budget);
                    let result =
                        execute_baseline_generated_code_with_generated_call_link_sidecar_and_metrics_and_validation_and_dispatch_budget(
                            BaselineGeneratedExecutionRequest {
                                artifact: &artifact,
                                owner: code_block_id,
                                code_block,
                                expected_frame,
                                execution: InterpreterExecutionState {
                                    stack: &mut self.execution,
                                    registers: &mut self.registers,
                                    exceptions: &mut self.exceptions,
                                    heap: &mut self.heap,
                                },
                            },
                            &mut sidecar,
                            &mut metrics,
                            validation,
                            dispatch_budget,
                        );
                    (
                        result,
                        sidecar.probe_miss_records().to_vec(),
                        sidecar.probe_blocked_records().to_vec(),
                        sidecar.direct_call_hot_slot_hits(),
                        metrics,
                    )
                };
                let outcome = Self::baseline_generated_execution_outcome(&result);
                self.record_baseline_generated_execution_metrics(
                    code_block_id,
                    bytecode_snapshot,
                    entry_kind,
                    current_tier,
                    metrics,
                    outcome,
                );
                self.record_generated_call_link_probe_miss_records(
                    bytecode_snapshot,
                    &call_link_probe_miss_records,
                );
                self.record_generated_call_link_probe_blocked_records(
                    bytecode_snapshot,
                    &call_link_probe_blocked_records,
                );
                self.tiering
                    .record_generated_direct_call_sidecar_hot_slot_hits(direct_call_hot_slot_hits);
                return self.finish_baseline_generated_result_control(
                    result,
                    code_block_id,
                    entry_kind,
                    current_tier,
                    code_block,
                    host,
                    config,
                );
            }
            let mut metrics = BaselineGeneratedExecutionMetrics::for_dispatch_config(config);
            let mut local_dispatch_budget = DispatchBudget::from_config(config);
            let dispatch_budget = self
                .active_dispatch_budget
                .as_mut()
                .unwrap_or(&mut local_dispatch_budget);
            let result =
                execute_baseline_generated_code_with_metrics_and_validation_and_dispatch_budget(
                    BaselineGeneratedExecutionRequest {
                        artifact: &artifact,
                        owner: code_block_id,
                        code_block,
                        expected_frame,
                        execution: InterpreterExecutionState {
                            stack: &mut self.execution,
                            registers: &mut self.registers,
                            exceptions: &mut self.exceptions,
                            heap: &mut self.heap,
                        },
                    },
                    &mut metrics,
                    validation,
                    dispatch_budget,
                );
            let outcome = Self::baseline_generated_execution_outcome(&result);
            self.record_baseline_generated_execution_metrics(
                code_block_id,
                bytecode_snapshot,
                entry_kind,
                current_tier,
                metrics,
                outcome,
            );
            self.finish_baseline_generated_result_control(
                result,
                code_block_id,
                entry_kind,
                current_tier,
                code_block,
                host,
                config,
            )
        }
    }

    fn record_baseline_generated_execution_metrics(
        &mut self,
        owner: CodeBlockId,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        entry_kind: crate::interpreter::ExecutionEntryKind,
        current_tier: JitType,
        metrics: BaselineGeneratedExecutionMetrics,
        outcome: VmBaselineGeneratedExecutionOutcome,
    ) {
        // C++ Baseline JIT increments BaselineJITData::m_executeCounter in
        // JIT::emit_op_loop_hint and slow-paths operationOptimize at the
        // LoopHint bytecode index. The Rust generated executor cannot borrow
        // Vm tiering while it also owns the interpreter stack/register/heap
        // execution borrows, so baseline.rs aggregates executed LoopHint
        // indices and Vm replays them into tiering after the generated body
        // returns. Real loop OSR entry remains a separate tiering batch.
        for observation in metrics.loop_hint_observations() {
            for _ in 0..observation.count {
                self.tiering.observe_loop_backedge(
                    owner,
                    self.config.tiering_policy(),
                    observation.bytecode_index,
                );
            }
        }
        self.tiering
            .record_baseline_generated_execution(VmBaselineGeneratedExecutionRequest {
                owner,
                bytecode_snapshot,
                entry_kind,
                current_tier,
                executed_bytecode_count: metrics.executed_bytecode_count,
                dispatched_opcode_counts: metrics.dispatched_opcode_counts().to_vec(),
                dispatched_site_opcode_counts: metrics.dispatched_site_opcode_counts().to_vec(),
                outcome,
            });
    }

    fn baseline_generated_execution_outcome(
        result: &Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
    ) -> VmBaselineGeneratedExecutionOutcome {
        match result {
            Ok(BaselineGeneratedExecutionResult::Completed(completion)) => {
                Self::baseline_generated_completion_outcome(completion)
            }
            Ok(BaselineGeneratedExecutionResult::Fallback(_)) => {
                VmBaselineGeneratedExecutionOutcome::Fallback
            }
            Ok(BaselineGeneratedExecutionResult::JsCall(_)) => {
                VmBaselineGeneratedExecutionOutcome::JsCall
            }
            Ok(BaselineGeneratedExecutionResult::Property(_)) => {
                VmBaselineGeneratedExecutionOutcome::Property
            }
            Err(BaselineGeneratedExecutionError::Execution(_)) => {
                VmBaselineGeneratedExecutionOutcome::Failed
            }
            Err(_) => VmBaselineGeneratedExecutionOutcome::Rejected,
        }
    }

    fn baseline_generated_execution_with_runtime_helpers_outcome(
        result: &Result<
            BaselineGeneratedExecutionWithRuntimeHelpersResult,
            BaselineGeneratedExecutionError,
        >,
    ) -> VmBaselineGeneratedExecutionOutcome {
        match result {
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion)) => {
                Self::baseline_generated_completion_outcome(completion)
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(_)) => {
                VmBaselineGeneratedExecutionOutcome::Fallback
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(_)) => {
                VmBaselineGeneratedExecutionOutcome::JsCall
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(_)) => {
                VmBaselineGeneratedExecutionOutcome::Property
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(_)) => {
                VmBaselineGeneratedExecutionOutcome::RuntimeHelper
            }
            Err(BaselineGeneratedExecutionError::Execution(_)) => {
                VmBaselineGeneratedExecutionOutcome::Failed
            }
            Err(_) => VmBaselineGeneratedExecutionOutcome::Rejected,
        }
    }

    fn baseline_generated_completion_outcome(
        completion: &ExecutionCompletion,
    ) -> VmBaselineGeneratedExecutionOutcome {
        match completion {
            ExecutionCompletion::Returned(_) => VmBaselineGeneratedExecutionOutcome::Returned,
            ExecutionCompletion::Threw(_) => VmBaselineGeneratedExecutionOutcome::Threw,
            ExecutionCompletion::OrdinaryBytecodeCall(_) => {
                VmBaselineGeneratedExecutionOutcome::OrdinaryBytecodeCall
            }
            ExecutionCompletion::OrdinaryBytecodeConstruct(_) => {
                VmBaselineGeneratedExecutionOutcome::OrdinaryBytecodeConstruct
            }
            ExecutionCompletion::FunctionValueCall(_) => {
                VmBaselineGeneratedExecutionOutcome::FunctionValueCall
            }
            // An eval deferral never originates from baseline-generated code (eval
            // is a native call routed through interpreter dispatch). If one ever
            // surfaces here, fall back to the interpreter loop, which owns the
            // `ExecutionCompletion::EvalRequest` handler.
            ExecutionCompletion::EvalRequest(_) => VmBaselineGeneratedExecutionOutcome::Fallback,
            ExecutionCompletion::BaselineLoopHandoff(_) => {
                VmBaselineGeneratedExecutionOutcome::Fallback
            }
            ExecutionCompletion::Terminated(_) => VmBaselineGeneratedExecutionOutcome::Terminated,
            ExecutionCompletion::Suspended(_) => VmBaselineGeneratedExecutionOutcome::Suspended,
            ExecutionCompletion::Failed(_) => VmBaselineGeneratedExecutionOutcome::Failed,
        }
    }

    fn finish_baseline_generated_with_runtime_helpers_result_control<H: DispatchHost>(
        &mut self,
        result: Result<
            BaselineGeneratedExecutionWithRuntimeHelpersResult,
            BaselineGeneratedExecutionError,
        >,
        code_block_id: CodeBlockId,
        entry_kind: crate::interpreter::ExecutionEntryKind,
        current_tier: JitType,
        code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> GeneratedExecutionControl {
        match result {
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion)) => {
                GeneratedExecutionControl::complete(completion)
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback)) => self
                .finish_baseline_generated_fallback_control(
                    fallback,
                    code_block_id,
                    entry_kind,
                    current_tier,
                    code_block,
                    host,
                    config,
                ),
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff)) => {
                self.execute_generated_js_call_handoff_control(handoff, code_block, host, config)
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff)) => {
                self.execute_generated_property_handoff_control(handoff, code_block, host, config)
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff)) => self
                .execute_generated_runtime_helper_handoff_control(
                    handoff, code_block, host, config,
                ),
            Err(BaselineGeneratedExecutionError::Execution(error)) => {
                GeneratedExecutionControl::failed(error)
            }
            Err(error) => {
                let _ = error;
                GeneratedExecutionControl::failed(
                    ExecutionError::BaselineGeneratedExecutionRejected,
                )
            }
        }
    }

    fn finish_baseline_generated_result_control<H: DispatchHost>(
        &mut self,
        result: Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        code_block_id: CodeBlockId,
        entry_kind: crate::interpreter::ExecutionEntryKind,
        current_tier: JitType,
        code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> GeneratedExecutionControl {
        match result {
            Ok(BaselineGeneratedExecutionResult::Completed(completion)) => {
                GeneratedExecutionControl::complete(completion)
            }
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback)) => self
                .finish_baseline_generated_fallback_control(
                    fallback,
                    code_block_id,
                    entry_kind,
                    current_tier,
                    code_block,
                    host,
                    config,
                ),
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff)) => {
                self.execute_generated_js_call_handoff_control(handoff, code_block, host, config)
            }
            Ok(BaselineGeneratedExecutionResult::Property(handoff)) => {
                self.execute_generated_property_handoff_control(handoff, code_block, host, config)
            }
            Err(BaselineGeneratedExecutionError::Execution(error)) => {
                GeneratedExecutionControl::failed(error)
            }
            Err(error) => {
                let _ = error;
                GeneratedExecutionControl::failed(
                    ExecutionError::BaselineGeneratedExecutionRejected,
                )
            }
        }
    }

    fn finish_baseline_generated_fallback_control<H: DispatchHost>(
        &mut self,
        fallback: BaselineGeneratedFallback,
        code_block_id: CodeBlockId,
        entry_kind: crate::interpreter::ExecutionEntryKind,
        current_tier: JitType,
        code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> GeneratedExecutionControl {
        let request = fallback.request;
        self.record_baseline_generated_fallback(code_block_id, entry_kind, current_tier, fallback);
        if Self::generated_fallback_single_dispatch_opcode(fallback).is_some() {
            let outcome = self.execute_generated_fallback_single_dispatch_exit_transaction(
                request, code_block, host,
            );
            return self.finish_generated_single_dispatch_handoff_control(
                GeneratedSingleDispatchResume {
                    owner: request.code_block,
                    frame: request.frame,
                    bytecode_index: request.bytecode_index,
                },
                outcome,
            );
        }

        GeneratedExecutionControl::Complete(
            self.execute_baseline_fallback_in_current_region(request, code_block, host, config),
        )
    }

    fn generated_fallback_single_dispatch_opcode(
        fallback: BaselineGeneratedFallback,
    ) -> Option<CoreOpcode> {
        if fallback.reason.cause != BaselineGeneratedFallbackCause::UnsupportedOpcode {
            return None;
        }
        match fallback.reason.opcode {
            BaselineGeneratedFallbackOpcode::Core(
                opcode @ (CoreOpcode::LoadFunction
                | CoreOpcode::LoadCallee
                | CoreOpcode::GetGlobalLexical
                | CoreOpcode::InitializeGlobalLexical),
            ) => Some(opcode),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(super) fn execute_baseline_generated_code_with_runtime_helper_plan_in_current_region<
        H: DispatchHost,
    >(
        &mut self,
        request: BaselineGeneratedRuntimeHelperExecutionRequest<'_, '_>,
        host: &mut H,
        config: DispatchConfig,
    ) -> ExecutionCompletion {
        let BaselineGeneratedRuntimeHelperExecutionRequest {
            artifact,
            owner,
            code_block,
            expected_frame,
            runtime_helper_plan,
        } = request;

        if !self.config.baseline_generated_execution_enabled() {
            return ExecutionCompletion::Failed(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let mut local_dispatch_budget = DispatchBudget::from_config(config);
        let dispatch_budget = self
            .active_dispatch_budget
            .as_mut()
            .unwrap_or(&mut local_dispatch_budget);
        match execute_baseline_generated_code_with_runtime_helpers_and_dispatch_budget(
            BaselineGeneratedExecutionRequest {
                artifact,
                owner,
                code_block,
                expected_frame,
                execution: InterpreterExecutionState {
                    stack: &mut self.execution,
                    registers: &mut self.registers,
                    exceptions: &mut self.exceptions,
                    heap: &mut self.heap,
                },
            },
            runtime_helper_plan,
            dispatch_budget,
        ) {
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion)) => {
                completion
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback)) => {
                let request = fallback.request;
                self.execute_baseline_fallback_in_current_region(request, code_block, host, config)
            }
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff)) => self
                .execute_generated_js_call_handoff_in_current_region(
                    handoff, code_block, host, config,
                ),
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff)) => self
                .execute_generated_property_handoff_in_current_region(
                    handoff, code_block, host, config,
                ),
            Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff)) => self
                .execute_generated_runtime_helper_handoff_in_current_region(
                    handoff, code_block, host, config,
                ),
            Err(BaselineGeneratedExecutionError::Execution(error)) => {
                ExecutionCompletion::Failed(error)
            }
            Err(_) => {
                ExecutionCompletion::Failed(ExecutionError::BaselineGeneratedExecutionRejected)
            }
        }
    }
}
