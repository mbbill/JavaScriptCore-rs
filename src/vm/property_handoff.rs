//! VM-owned generated property IC exit and handoff execution.
//!
//! This is the Rust home for the generated property access slow-exit handoff
//! cluster. It mirrors the JSC boundary around baseline property access slow
//! exits in `JITPropertyAccess.cpp`, the optimize operations in
//! `JITOperations.cpp`, and the `PropertyInlineCache`/`Repatch.cpp` cache
//! update path. The logic remains VM-owned because Rust validates the active
//! frame and CodeBlock metadata, preserves no-GC exit/reentry state, drains IC
//! observations, and maintains exception/rooting state across the handoff.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GeneratedPropertyExitValidation {
    pub(super) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
}

impl Vm {
    pub(super) fn execute_generated_property_handoff_in_current_region<H: DispatchHost>(
        &mut self,
        handoff: BaselineGeneratedPropertyHandoff,
        code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> ExecutionCompletion {
        let control =
            self.execute_generated_property_handoff_control(handoff, code_block, host, config);
        self.drive_generated_execution_control(control, code_block, host, config)
    }

    pub(super) fn execute_generated_property_handoff_control<H: DispatchHost>(
        &mut self,
        handoff: BaselineGeneratedPropertyHandoff,
        code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> GeneratedExecutionControl {
        let outcome = self.dispatch_generated_property_handoff_in_current_region(
            handoff, code_block, host, config,
        );
        self.finish_generated_single_dispatch_handoff_control(
            GeneratedSingleDispatchResume::from_property(handoff),
            outcome,
        )
    }

    pub(super) fn validate_generated_property_exit_handoff(
        &self,
        handoff: BaselineGeneratedPropertyHandoff,
        code_block: &CodeBlock,
    ) -> Result<GeneratedPropertyExitValidation, ExecutionError> {
        if !handoff.resume.bytecode_index.is_valid() {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }
        if !matches!(
            handoff.resume.opcode,
            CoreOpcode::GetByName
                | CoreOpcode::GetGlobalObjectProperty
                | CoreOpcode::GetLength
                | CoreOpcode::PutByName
                | CoreOpcode::PutGlobalObjectProperty
                | CoreOpcode::GetByValue
                | CoreOpcode::PutByValue
                | CoreOpcode::InById
                | CoreOpcode::InByVal
        ) {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }
        if handoff.site.owner != handoff.resume.owner
            || handoff.site.bytecode_index != handoff.resume.bytecode_index
            || handoff.site.opcode != handoff.resume.opcode
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }
        validate_baseline_generated_property_handoff_site_metadata(&handoff.site)
            .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        if handoff.requires_no_gc_exit_reentry != handoff.site.requires_no_gc_exit_reentry {
            return if handoff.requires_no_gc_exit_reentry {
                Err(ExecutionError::BaselineGeneratedExecutionRejected)
            } else {
                Err(ExecutionError::GcBoundaryViolation)
            };
        }
        if !handoff.requires_no_gc_exit_reentry {
            return Err(ExecutionError::GcBoundaryViolation);
        }
        if handoff.may_throw != handoff.site.may_throw || !handoff.may_throw {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let active_frame = self
            .execution
            .top_frame()
            .ok_or(ExecutionError::NoActiveFrame)?;
        if active_frame.id != handoff.resume.frame
            || active_frame.code_block != Some(handoff.resume.owner)
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let registered_code_block = self
            .code_blocks
            .get(handoff.resume.owner)
            .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?
            .code_block();
        let current_snapshot =
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block)
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        let registered_snapshot =
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(
                registered_code_block,
            )
            .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        if registered_snapshot != current_snapshot {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        if let Some(artifact) = self
            .tiering
            .baseline_generated_code_artifact_for(handoff.resume.owner)
        {
            artifact
                .validate()
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
            if artifact.eligibility_proof.bytecode_snapshot_fingerprint() != current_snapshot {
                return Err(ExecutionError::BaselineGeneratedExecutionRejected);
            }
            let artifact_plan = artifact
                .property_handoff_plan()
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            if artifact_plan.bytecode_snapshot != current_snapshot {
                return Err(ExecutionError::BaselineGeneratedExecutionRejected);
            }
            let artifact_site = artifact_plan
                .site_for_bytecode_index(handoff.resume.bytecode_index)
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            if *artifact_site != handoff.site {
                return Err(ExecutionError::BaselineGeneratedExecutionRejected);
            }
        }

        // C++ JSC baseline property ICs run their slow operation and may
        // repatch the cache on a miss/stale stub (`JIT::emitSlow_op_get_by_id`
        // -> `operationGetByIdOptimize` -> `repatchGetBy`). They do not reject
        // the whole baseline execution because the bytecode IC has already
        // warmed past the cold install shape. Rust still keeps artifact-backed
        // exits exact above, but an invalidated/missing artifact can safely run
        // the existing single-dispatch slow path once the handoff matches the
        // current CodeBlock metadata.
        validate_baseline_generated_property_handoff_site_against_current_code_block(
            code_block,
            handoff.resume.owner,
            &handoff.site,
        )
        .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;

        Ok(GeneratedPropertyExitValidation {
            bytecode_snapshot: current_snapshot,
        })
    }

    pub(super) fn dispatch_generated_property_handoff_in_current_region<H: DispatchHost>(
        &mut self,
        handoff: BaselineGeneratedPropertyHandoff,
        code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> SingleDispatchOutcome {
        let validation = match self.validate_generated_property_exit_handoff(handoff, code_block) {
            Ok(validation) => validation,
            Err(error) => return SingleDispatchOutcome::Failed(error),
        };
        let bytecode_snapshot = validation.bytecode_snapshot;
        self.execute_generated_property_single_dispatch_exit_transaction(
            handoff,
            bytecode_snapshot,
            code_block,
            host,
            config,
        )
    }

    fn drain_generated_property_handoff_observation<H: DispatchHost>(
        &mut self,
        handoff: BaselineGeneratedPropertyHandoff,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        host: &mut H,
    ) -> Result<(), ExecutionError> {
        match handoff.site.opcode {
            CoreOpcode::GetByName
            | CoreOpcode::GetGlobalObjectProperty
            | CoreOpcode::GetLength
            | CoreOpcode::GetByValue => {
                let observation = host.drain_property_load_observation(
                    &mut self.heap,
                    PropertyLoadObservationDrainRequest {
                        owner: handoff.site.owner,
                        frame: handoff.resume.frame,
                        bytecode_index: handoff.site.bytecode_index,
                        opcode: handoff.site.opcode,
                        slot: handoff.site.slot,
                        cache_kind: handoff.site.cache_kind,
                        fallback: handoff.site.fallback,
                        property_key: handoff.site.property_key,
                        cold_miss_handoff: handoff.site.cold_miss_handoff,
                    },
                )?;
                if let Some(descriptor) = observation {
                    self.tiering
                        .record_property_load_observation(VmPropertyLoadObservationRequest {
                            owner: handoff.resume.owner,
                            frame: Some(handoff.resume.frame),
                            bytecode_index: handoff.resume.bytecode_index,
                            bytecode_snapshot,
                            descriptor,
                        })
                        .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
                }
            }
            CoreOpcode::PutByName
            | CoreOpcode::PutGlobalObjectProperty
            | CoreOpcode::PutByValue => {
                let observation = host.drain_property_store_observation(
                    &mut self.heap,
                    PropertyStoreObservationDrainRequest {
                        owner: handoff.site.owner,
                        frame: handoff.resume.frame,
                        bytecode_index: handoff.site.bytecode_index,
                        opcode: handoff.site.opcode,
                        slot: handoff.site.slot,
                        cache_kind: handoff.site.cache_kind,
                        fallback: handoff.site.fallback,
                        property_key: handoff.site.property_key,
                        cold_miss_handoff: handoff.site.cold_miss_handoff,
                    },
                )?;
                if let Some(descriptor) = observation {
                    self.tiering
                        .record_property_store_observation(VmPropertyStoreObservationRequest {
                            owner: handoff.resume.owner,
                            frame: Some(handoff.resume.frame),
                            bytecode_index: handoff.resume.bytecode_index,
                            bytecode_snapshot,
                            descriptor,
                        })
                        .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
                }
            }
            CoreOpcode::InById | CoreOpcode::InByVal => {
                let observation = host.drain_property_has_observation(
                    &mut self.heap,
                    PropertyHasObservationDrainRequest {
                        owner: handoff.site.owner,
                        frame: handoff.resume.frame,
                        bytecode_index: handoff.site.bytecode_index,
                        opcode: handoff.site.opcode,
                        slot: handoff.site.slot,
                        cache_kind: handoff.site.cache_kind,
                        fallback: handoff.site.fallback,
                        property_key: handoff.site.property_key,
                        cold_miss_handoff: handoff.site.cold_miss_handoff,
                    },
                )?;
                if let Some(descriptor) = observation {
                    self.tiering
                        .record_property_has_observation(VmPropertyHasObservationRequest {
                            owner: handoff.resume.owner,
                            frame: Some(handoff.resume.frame),
                            bytecode_index: handoff.resume.bytecode_index,
                            bytecode_snapshot,
                            descriptor,
                        })
                        .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
                }
            }
            _ => return Err(ExecutionError::BaselineGeneratedExecutionRejected),
        }
        Ok(())
    }

    pub(super) fn drain_interpreter_property_store_observations_for_generated_property_plan<
        H: DispatchHost,
    >(
        &mut self,
        owner: CodeBlockId,
        frame: Option<CallFrameId>,
        code_block: &CodeBlock,
        host: &mut H,
    ) {
        let Some(frame) = frame else {
            return;
        };
        let Ok(current_snapshot) =
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block)
        else {
            return;
        };
        let has_artifact_property_plan = self
            .tiering
            .baseline_generated_code_artifact_for(owner)
            .and_then(|artifact| {
                artifact
                    .property_handoff_plan()
                    .map(|plan| plan.bytecode_snapshot)
            })
            .is_some_and(|snapshot| snapshot == current_snapshot);
        let has_retained_property_exit_table = self
            .p6_x86_64_callable_retained_return_tables
            .iter()
            .any(|table| {
                table.owner == owner
                    && table.bytecode_snapshot == current_snapshot
                    && !table.property_exit_sites.is_empty()
            });
        if !has_artifact_property_plan && !has_retained_property_exit_table {
            return;
        }
        let Ok(derivation) =
            derive_baseline_generated_property_handoff_plan_from_current_code_block_metadata(
                code_block, owner,
            )
        else {
            return;
        };
        let Some(metadata) = derivation.metadata else {
            return;
        };
        if metadata.bytecode_snapshot() != current_snapshot {
            return;
        }
        let sites = (0..metadata.site_count())
            .filter_map(|index| metadata.site_at(index).copied())
            .collect::<Vec<_>>();
        for site in sites.into_iter().rev().filter(|site| {
            matches!(
                site.opcode,
                CoreOpcode::PutByName
                    | CoreOpcode::PutGlobalObjectProperty
                    | CoreOpcode::PutByValue
            )
        }) {
            let Ok(Some(descriptor)) = host.drain_property_store_observation(
                &mut self.heap,
                PropertyStoreObservationDrainRequest {
                    owner: site.owner,
                    frame,
                    bytecode_index: site.bytecode_index,
                    opcode: site.opcode,
                    slot: site.slot,
                    cache_kind: site.cache_kind,
                    fallback: site.fallback,
                    property_key: site.property_key,
                    cold_miss_handoff: site.cold_miss_handoff,
                },
            ) else {
                continue;
            };
            let _ =
                self.tiering
                    .record_property_store_observation(VmPropertyStoreObservationRequest {
                        owner,
                        frame: Some(frame),
                        bytecode_index: site.bytecode_index,
                        bytecode_snapshot: metadata.bytecode_snapshot(),
                        descriptor,
                    });
        }
    }

    fn execute_generated_property_single_dispatch_exit_transaction<H: DispatchHost>(
        &mut self,
        handoff: BaselineGeneratedPropertyHandoff,
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> SingleDispatchOutcome {
        let transaction = GeneratedSingleDispatchExitTransaction::from_property(handoff);
        if !transaction.requires_no_gc_exit_reentry {
            return SingleDispatchOutcome::Failed(ExecutionError::GcBoundaryViolation);
        }
        let suspended = match self.suspend_no_gc_execution_region() {
            Ok(suspended) => suspended,
            Err(_) => return SingleDispatchOutcome::Failed(ExecutionError::GcBoundaryViolation),
        };
        let mut active_register_roots = Vec::new();
        let mut active_frame_roots = Vec::new();
        // D2i/Wave 2b: the deferred direct-call exit no longer syncs the caller
        // (nonlocal) frame-header roots here; every live frame's header cells
        // are gathered at the safepoint (`gather_vm_frame_header_roots`) over
        // the whole call-frame stack, which already covers caller frames. The
        // `active_*_roots` vecs stay (empty) for the deferred-rooting cleanup
        // boundary, gated by the non-frame deferred-direct-call flag.
        let exception_snapshot = self.exceptions.clone();
        let outcome = execute_single_dispatch_deferring_ordinary_calls(
            InterpreterExecutionState {
                stack: &mut self.execution,
                registers: &mut self.registers,
                exceptions: &mut self.exceptions,
                heap: &mut self.heap,
            },
            SingleDispatchRequest::new(
                transaction.resume.owner,
                transaction.resume.frame,
                transaction.resume.bytecode_index,
            ),
            code_block,
            host,
        );

        match outcome {
            SingleDispatchOutcome::FunctionValueCall(request) => {
                let request = *request;
                let outcome = match self.resume_no_gc_execution_region(suspended) {
                    Ok(_) => self
                        .execute_function_value_call_request_as_single_dispatch_in_current_region(
                            request,
                            transaction.resume.owner,
                            code_block,
                            host,
                            config,
                        ),
                    Err(_) => self.finish_function_value_call_fail(
                        request.completion,
                        ExecutionError::GcBoundaryViolation,
                        host,
                    ),
                };
                if let Err(error) = self.drain_generated_property_handoff_observation(
                    handoff,
                    bytecode_snapshot,
                    host,
                ) {
                    let error = self
                        .restore_exception_snapshot_and_sync_roots(exception_snapshot)
                        .err()
                        .unwrap_or(error);
                    let cleanup = cleanup_targeted_root_sets(
                        &mut self.heap,
                        &mut active_register_roots,
                        &mut active_frame_roots,
                    );
                    let error = cleanup.err().unwrap_or(error);
                    return SingleDispatchOutcome::Failed(error);
                }
                let outcome = self.normalize_generated_single_dispatch_outcome(
                    transaction.may_throw,
                    exception_snapshot,
                    outcome,
                );
                match cleanup_targeted_root_sets(
                    &mut self.heap,
                    &mut active_register_roots,
                    &mut active_frame_roots,
                ) {
                    Ok(()) => outcome,
                    Err(error) => SingleDispatchOutcome::Failed(error),
                }
            }
            SingleDispatchOutcome::OrdinaryBytecodeCall(_)
            | SingleDispatchOutcome::OrdinaryBytecodeConstruct(_) => {
                let outcome = match cleanup_targeted_root_sets(
                    &mut self.heap,
                    &mut active_register_roots,
                    &mut active_frame_roots,
                ) {
                    Ok(()) => SingleDispatchOutcome::Failed(ExecutionError::InvalidCallCompletion),
                    Err(error) => SingleDispatchOutcome::Failed(error),
                };
                self.resume_generated_single_dispatch_no_gc(suspended, outcome)
            }
            outcome => {
                if let Err(error) = self.drain_generated_property_handoff_observation(
                    handoff,
                    bytecode_snapshot,
                    host,
                ) {
                    let error = self
                        .restore_exception_snapshot_and_sync_roots(exception_snapshot)
                        .err()
                        .unwrap_or(error);
                    let cleanup = cleanup_targeted_root_sets(
                        &mut self.heap,
                        &mut active_register_roots,
                        &mut active_frame_roots,
                    );
                    return self.resume_generated_single_dispatch_no_gc(
                        suspended,
                        SingleDispatchOutcome::Failed(cleanup.err().unwrap_or(error)),
                    );
                }
                let outcome = self.normalize_generated_single_dispatch_outcome(
                    transaction.may_throw,
                    exception_snapshot,
                    outcome,
                );
                let outcome = match cleanup_targeted_root_sets(
                    &mut self.heap,
                    &mut active_register_roots,
                    &mut active_frame_roots,
                ) {
                    Ok(()) => outcome,
                    Err(error) => SingleDispatchOutcome::Failed(error),
                };
                self.resume_generated_single_dispatch_no_gc(suspended, outcome)
            }
        }
    }
}
