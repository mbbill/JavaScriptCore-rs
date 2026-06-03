//! VM-owned generated direct-call execution.
//!
//! This is the Rust home for the generated direct-call / CallLinkInfo fast-path
//! execution cluster. It mirrors the JSC boundary around `CallLinkInfo` and
//! `DirectCallLinkInfo`: validation of linked callee metadata, fast-path target
//! selection, slow-path preparation of missing callee artifacts, and call result
//! completion/profile handling all remain VM-owned because they depend on the
//! frame stack, roots, exceptions, tiering telemetry, and CodeBlock registry.

use super::*;

#[derive(Clone, Copy)]
pub(super) enum GeneratedJsDirectCallReturnMode<'a> {
    VmContinuation,
    OwnerPostCallReentry(&'a P9X86_64BaselineOwnerPostCallReturnTargetProof),
}

pub(super) enum GeneratedJsDirectCallTransactionResult {
    Outcome(SingleDispatchOutcome),
    P9OwnerPostCallReentry(P9OwnerPostCallReentryInvocation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GeneratedDirectCallRootlessProofEpoch {
    pub(super) call_link_projection: GeneratedCallLinkProjectionEpoch,
    pub(super) baseline_generated_code_artifacts: VmRecordSequenceEpoch,
    pub(super) baseline_generated_code_invalidations: VmRecordSequenceEpoch,
    pub(super) property_inline_cache_attachment_records: VmRecordSequenceEpoch,
    pub(super) property_inline_cache_clear_records: VmRecordSequenceEpoch,
    pub(super) structure_stub_repatch_transactions: VmRecordSequenceEpoch,
    pub(super) structure_stub_access_case_links: VmRecordSequenceEpoch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GeneratedDirectCallRootlessProofCache {
    pub(super) epoch: GeneratedDirectCallRootlessProofEpoch,
    pub(super) target_code_block_id: CodeBlockId,
    pub(super) artifact_id: JitCodeId,
    pub(super) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    pub(super) proof: GeneratedDirectCallRootlessGeneratedEntryProof,
}

pub(super) const GENERATED_DIRECT_CALL_HOT_SLOT_RETAIN_LIMIT: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GeneratedDirectCallHotSlot {
    pub(super) owner: CodeBlockId,
    pub(super) opcode: CoreOpcode,
    pub(super) bytecode_index: BytecodeIndex,
    pub(super) projection_epoch: GeneratedCallLinkProjectionEpoch,
    pub(super) candidate: GeneratedCallLinkCandidate,
    pub(super) authorization: GeneratedCallLinkDirectCall,
    pub(super) callee_object: ObjectId,
    pub(super) argument_count_including_this: u32,
    pub(super) preferred_route: VmGeneratedDirectCallTransactionRoute,
    pub(super) rootless_generated_entry_proof: Option<GeneratedDirectCallRootlessProofCache>,
}

#[derive(Clone, Debug)]
pub(super) struct GeneratedJsDirectCallValidation {
    pub(super) owner: CodeBlockId,
    pub(super) opcode: CoreOpcode,
    pub(super) bytecode_index: BytecodeIndex,
    pub(super) continuation: CallReturnContinuation,
    pub(super) target_code_block_id: CodeBlockId,
    // C++ JSC divergence (one shared instance): the generated-direct-call hot path
    // (box2d/raytrace residency) holds the shared `Rc<CodeBlock>` (refcount bump)
    // instead of a deep registry copy, so the per-instance memo + feedback persist.
    pub(super) target_code_block: Rc<CodeBlock>,
    pub(super) argument_values: Vec<crate::runtime::RuntimeValue>,
    pub(super) direct_call: BaselineGeneratedJsDirectCall,
    pub(super) hot_slot_hit: bool,
    pub(super) preferred_route: Option<VmGeneratedDirectCallTransactionRoute>,
    pub(super) rootless_generated_entry_proof: Option<GeneratedDirectCallRootlessProofCache>,
}

#[derive(Clone, Debug)]
pub(super) struct GeneratedDirectCallCalleeExecution {
    pub(super) completion: ExecutionCompletion,
    pub(super) route: VmGeneratedDirectCallTransactionRoute,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GeneratedDirectCallCalleeRoutePolicy {
    AnyAvailable,
    GeneratedEntryOnly,
    NativeEntryOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GeneratedDirectCallRootlessGeneratedEntryProof {
    StrictLocalLeaf,
    DeferredNonThrowingExitRooting,
    DeferredThrowingSideExitRooting,
    DeferredGeneratedExitRooting,
}

impl GeneratedDirectCallRootlessGeneratedEntryProof {
    pub(super) const fn needs_deferred_exit_rooting(self) -> bool {
        matches!(
            self,
            Self::DeferredNonThrowingExitRooting
                | Self::DeferredThrowingSideExitRooting
                | Self::DeferredGeneratedExitRooting
        )
    }

    pub(super) const fn allows_throw(self) -> bool {
        matches!(
            self,
            Self::DeferredThrowingSideExitRooting | Self::DeferredGeneratedExitRooting
        )
    }
}

impl Vm {
    pub(super) fn execute_generated_js_direct_call_transaction<H: DispatchHost>(
        &mut self,
        handoff: BaselineGeneratedJsCallHandoff,
        caller_code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
    ) -> SingleDispatchOutcome {
        match self.execute_generated_js_direct_call_transaction_with_return_mode(
            handoff,
            caller_code_block,
            host,
            config,
            GeneratedJsDirectCallReturnMode::VmContinuation,
        ) {
            GeneratedJsDirectCallTransactionResult::Outcome(outcome) => outcome,
            GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(_) => {
                SingleDispatchOutcome::Failed(ExecutionError::BaselineGeneratedExecutionRejected)
            }
        }
    }

    pub(super) fn execute_generated_js_direct_call_transaction_with_return_mode<
        'a,
        H: DispatchHost,
    >(
        &mut self,
        handoff: BaselineGeneratedJsCallHandoff,
        caller_code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
        return_mode: GeneratedJsDirectCallReturnMode<'a>,
    ) -> GeneratedJsDirectCallTransactionResult {
        if !handoff.requires_no_gc_exit_reentry {
            return GeneratedJsDirectCallTransactionResult::Outcome(SingleDispatchOutcome::Failed(
                ExecutionError::GcBoundaryViolation,
            ));
        }
        if !handoff.may_throw {
            return GeneratedJsDirectCallTransactionResult::Outcome(SingleDispatchOutcome::Failed(
                ExecutionError::BaselineGeneratedExecutionRejected,
            ));
        }

        let resume_frame = handoff.resume.frame;

        let failed = |error| {
            GeneratedJsDirectCallTransactionResult::Outcome(SingleDispatchOutcome::Failed(error))
        };

        let normalize_rootless =
            |vm: &mut Self,
             proof: GeneratedDirectCallRootlessGeneratedEntryProof,
             exception_snapshot: ExceptionState,
             result: GeneratedJsDirectCallTransactionResult| {
                match result {
                    GeneratedJsDirectCallTransactionResult::Outcome(outcome) => {
                        GeneratedJsDirectCallTransactionResult::Outcome(
                            vm.normalize_rootless_generated_direct_call_outcome(
                                proof,
                                exception_snapshot,
                                outcome,
                            ),
                        )
                    }
                    GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(reentry) => {
                        GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(reentry)
                    }
                }
            };

        let resume_generated_no_gc = |vm: &mut Self,
                                      suspended,
                                      outcome: SingleDispatchOutcome|
         -> GeneratedJsDirectCallTransactionResult {
            GeneratedJsDirectCallTransactionResult::Outcome(
                vm.resume_generated_single_dispatch_no_gc(suspended, outcome),
            )
        };

        let validation = match self
            .try_validate_generated_js_direct_call_hot_slot(&handoff, caller_code_block)
        {
            Ok(Some(validation)) => validation,
            Ok(None) => {
                match self.validate_generated_js_direct_call_handoff(&handoff, caller_code_block) {
                    Ok(validation) => validation,
                    Err(error) => return failed(error),
                }
            }
            Err(error) => return failed(error),
        };

        let rootless_generated_entry_rejection =
            match self.generated_js_direct_call_rootless_generated_entry_proof(&validation) {
                Ok(proof) => {
                    self.tiering
                        .record_generated_direct_call_rootless_generated_entry();
                    let exception_snapshot = self.exceptions.clone();
                    if proof.needs_deferred_exit_rooting() {
                        self.enter_generated_direct_call_deferred_rooting();
                    }
                    let result = self.execute_validated_generated_js_direct_call_with_return_mode(
                        validation,
                        caller_code_block,
                        host,
                        config,
                        GeneratedDirectCallCalleeRoutePolicy::GeneratedEntryOnly,
                        return_mode,
                    );
                    if proof.needs_deferred_exit_rooting() {
                        self.leave_generated_direct_call_deferred_rooting();
                    }
                    return normalize_rootless(self, proof, exception_snapshot, result);
                }
                Err(reason) => reason,
            };

        if matches!(
            rootless_generated_entry_rejection,
            VmGeneratedDirectCallRootlessRejectionReason::PreferredRouteNotGeneratedEntry {
                native_entry_kind: Some(
                    BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry
                )
            }
        ) {
            match self.generated_js_direct_call_rootless_emitted_native_entry_proof(&validation) {
                Ok(proof) => {
                    self.tiering
                        .record_generated_direct_call_rootless_native_entry();
                    let exception_snapshot = self.exceptions.clone();
                    if proof.needs_deferred_exit_rooting() {
                        self.enter_generated_direct_call_deferred_rooting();
                    }
                    let result = self.execute_validated_generated_js_direct_call_with_return_mode(
                        validation,
                        caller_code_block,
                        host,
                        config,
                        GeneratedDirectCallCalleeRoutePolicy::NativeEntryOnly,
                        return_mode,
                    );
                    if proof.needs_deferred_exit_rooting() {
                        self.leave_generated_direct_call_deferred_rooting();
                    }
                    return normalize_rootless(self, proof, exception_snapshot, result);
                }
                Err(reason) => self
                    .tiering
                    .record_generated_direct_call_rootless_native_entry_rejection(reason),
            }
        }

        self.tiering
            .record_generated_direct_call_rootless_rejection(rootless_generated_entry_rejection);

        let suspended = match self.suspend_no_gc_execution_region() {
            Ok(suspended) => suspended,
            Err(_) => return failed(ExecutionError::GcBoundaryViolation),
        };

        let mut active_register_roots = Vec::new();
        let mut active_frame_roots = Vec::new();
        if let Err(error) = self.sync_generated_direct_call_deferred_nonlocal_roots(
            resume_frame,
            host,
            &mut active_register_roots,
            &mut active_frame_roots,
        ) {
            let cleanup = cleanup_targeted_root_sets(
                &mut self.heap,
                &mut active_register_roots,
                &mut active_frame_roots,
            );
            return resume_generated_no_gc(
                self,
                suspended,
                SingleDispatchOutcome::Failed(cleanup.err().unwrap_or(error)),
            );
        }
        if let Err(error) = sync_targeted_frame_roots(
            resume_frame,
            &self.execution,
            &self.registers,
            &mut self.heap,
            &mut active_frame_roots,
        ) {
            let cleanup = cleanup_targeted_root_sets(
                &mut self.heap,
                &mut active_register_roots,
                &mut active_frame_roots,
            );
            return resume_generated_no_gc(
                self,
                suspended,
                SingleDispatchOutcome::Failed(cleanup.err().unwrap_or(error)),
            );
        }
        if let Err(error) = sync_targeted_register_roots(
            resume_frame,
            &self.execution,
            &self.registers,
            &mut self.heap,
            host,
            &mut active_register_roots,
        ) {
            let cleanup = cleanup_targeted_root_sets(
                &mut self.heap,
                &mut active_register_roots,
                &mut active_frame_roots,
            );
            return resume_generated_no_gc(
                self,
                suspended,
                SingleDispatchOutcome::Failed(cleanup.err().unwrap_or(error)),
            );
        }

        let result = self.execute_validated_generated_js_direct_call_with_return_mode(
            validation,
            caller_code_block,
            host,
            config,
            GeneratedDirectCallCalleeRoutePolicy::AnyAvailable,
            return_mode,
        );
        let result = match result {
            GeneratedJsDirectCallTransactionResult::Outcome(outcome) => {
                GeneratedJsDirectCallTransactionResult::Outcome(outcome)
            }
            GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(reentry) => {
                GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(reentry)
            }
        };
        let outcome = match cleanup_targeted_root_sets(
            &mut self.heap,
            &mut active_register_roots,
            &mut active_frame_roots,
        ) {
            Ok(()) => match result {
                GeneratedJsDirectCallTransactionResult::Outcome(outcome) => outcome,
                GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(reentry) => {
                    return match self.resume_no_gc_execution_region(suspended) {
                        Ok(_) => {
                            GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(reentry)
                        }
                        Err(_) => GeneratedJsDirectCallTransactionResult::Outcome(
                            SingleDispatchOutcome::Failed(ExecutionError::GcBoundaryViolation),
                        ),
                    }
                }
            },
            Err(error) => SingleDispatchOutcome::Failed(error),
        };
        let outcome = if self.exceptions.pending().is_some() {
            match self.sync_exception_targeted_roots() {
                Ok(()) => outcome,
                Err(error) => SingleDispatchOutcome::Failed(error.into()),
            }
        } else {
            outcome
        };

        GeneratedJsDirectCallTransactionResult::Outcome(
            self.resume_generated_single_dispatch_no_gc(suspended, outcome),
        )
    }

    fn execute_validated_generated_js_direct_call_with_return_mode<'a, H: DispatchHost>(
        &mut self,
        validation: GeneratedJsDirectCallValidation,
        caller_code_block: &CodeBlock,
        host: &mut H,
        config: DispatchConfig,
        route_policy: GeneratedDirectCallCalleeRoutePolicy,
        return_mode: GeneratedJsDirectCallReturnMode<'a>,
    ) -> GeneratedJsDirectCallTransactionResult {
        let GeneratedJsDirectCallValidation {
            owner,
            opcode,
            bytecode_index,
            continuation,
            target_code_block_id,
            target_code_block,
            argument_values,
            direct_call,
            hot_slot_hit,
            preferred_route,
            rootless_generated_entry_proof: _,
        } = validation;
        let argument_count_including_this = argument_values.len().try_into().unwrap_or(u32::MAX);
        let frame = match self.execution.push_frame(
            &mut self.registers,
            FramePushRequest {
                code_block: Some(target_code_block_id),
                callee: None,
                callee_value: continuation.callee_value,
                lexical_scope: None,
                shape: target_code_block.unlinked().frame(),
                argument_count_including_this,
                argument_values,
                start_bytecode_index: Some(BytecodeIndex::from_offset(0)),
                return_bytecode_index: Some(continuation.call_bytecode_index),
            },
        ) {
            Ok(frame) => frame,
            Err(error) => {
                self.record_generated_direct_call_transaction(
                    continuation,
                    target_code_block_id,
                    argument_count_including_this,
                    VmGeneratedDirectCallTransactionRoute::FrameSetupFailed,
                    VmGeneratedDirectCallTransactionOutcome::Failed,
                );
                return GeneratedJsDirectCallTransactionResult::Outcome(
                    SingleDispatchOutcome::Failed(error),
                );
            }
        };
        let continuation = match self
            .execution
            .attach_return_continuation(frame, continuation)
        {
            Ok(continuation) => continuation,
            Err(error) => {
                if let Err(cleanup_error) = self.execution.pop_frame(&mut self.registers, frame) {
                    self.record_generated_direct_call_transaction(
                        continuation,
                        target_code_block_id,
                        argument_count_including_this,
                        VmGeneratedDirectCallTransactionRoute::ContinuationAttachFailed,
                        VmGeneratedDirectCallTransactionOutcome::Failed,
                    );
                    return GeneratedJsDirectCallTransactionResult::Outcome(
                        SingleDispatchOutcome::Failed(cleanup_error),
                    );
                }
                self.record_generated_direct_call_transaction(
                    continuation,
                    target_code_block_id,
                    argument_count_including_this,
                    VmGeneratedDirectCallTransactionRoute::ContinuationAttachFailed,
                    VmGeneratedDirectCallTransactionOutcome::Failed,
                );
                return GeneratedJsDirectCallTransactionResult::Outcome(
                    SingleDispatchOutcome::Failed(error),
                );
            }
        };

        let callee_execution = self.execute_generated_direct_call_callee_code_block(
            continuation,
            target_code_block_id,
            &target_code_block,
            frame,
            argument_count_including_this,
            preferred_route,
            route_policy,
            host,
            config,
        );
        let route = callee_execution.route;
        if matches!(
            route,
            VmGeneratedDirectCallTransactionRoute::GeneratedEntry
                | VmGeneratedDirectCallTransactionRoute::NativeEntry
        ) && (!hot_slot_hit || preferred_route != Some(route))
        {
            // C++ CallLinkInfo keeps the linked target entrypoint mutable; when
            // Rust's validated direct-call route changes, refresh the VM-owned
            // hot slot instead of preserving a stale GeneratedEntry preference.
            self.install_generated_direct_call_hot_slot(
                owner,
                opcode,
                bytecode_index,
                &direct_call,
                route,
            );
        }
        let completion = callee_execution.completion;
        let thrown = match &completion {
            ExecutionCompletion::Threw(pending) => Some(*pending),
            _ => None,
        };
        if let (
            GeneratedJsDirectCallReturnMode::OwnerPostCallReentry(proof),
            ExecutionCompletion::Returned(value),
        ) = (return_mode, &completion)
        {
            let reentry = match self.prepare_p9_owner_post_call_reentry_invocation(
                continuation,
                *value,
                proof,
                &target_code_block,
            ) {
                Ok(reentry) => reentry,
                Err(error) => {
                    self.record_generated_direct_call_transaction(
                        continuation,
                        target_code_block_id,
                        argument_count_including_this,
                        route,
                        VmGeneratedDirectCallTransactionOutcome::Failed,
                    );
                    return GeneratedJsDirectCallTransactionResult::Outcome(
                        SingleDispatchOutcome::Failed(error),
                    );
                }
            };
            self.record_generated_direct_call_transaction(
                continuation,
                target_code_block_id,
                argument_count_including_this,
                route,
                VmGeneratedDirectCallTransactionOutcome::Continue,
            );
            return GeneratedJsDirectCallTransactionResult::P9OwnerPostCallReentry(reentry);
        }
        let profile_error = self
            .record_ordinary_js_call_result_value_profile_sample(&continuation, &completion)
            .err();
        let completion = profile_error
            .map(ExecutionCompletion::Failed)
            .unwrap_or(completion);
        let dispatch_outcome = {
            let mut state = DispatchState {
                stack: &mut self.execution,
                registers: &mut self.registers,
                exceptions: &mut self.exceptions,
                heap: &mut self.heap,
                code_block: &target_code_block,
                ordinary_bytecode_call_handling: OrdinaryBytecodeCallHandling::DirectInterpreter,
                function_value_call_handling: FunctionValueCallHandling::DirectInterpreter,
            };
            finish_ordinary_js_call_return(&mut state, continuation, completion)
        };

        let outcome = self.generated_js_direct_call_single_dispatch_outcome(
            caller_code_block,
            dispatch_outcome,
            thrown,
        );
        self.record_generated_direct_call_transaction(
            continuation,
            target_code_block_id,
            argument_count_including_this,
            route,
            Self::generated_direct_call_transaction_outcome(&outcome),
        );
        GeneratedJsDirectCallTransactionResult::Outcome(outcome)
    }

    fn prepare_p9_owner_post_call_reentry_invocation(
        &mut self,
        continuation: CallReturnContinuation,
        value: RuntimeValue,
        proof: &P9X86_64BaselineOwnerPostCallReturnTargetProof,
        target_code_block: &CodeBlock,
    ) -> Result<P9OwnerPostCallReentryInvocation, ExecutionError> {
        if proof.owner != continuation.owner
            || proof.call_bytecode_index != continuation.call_bytecode_index
            || proof.opcode != continuation.kind.opcode()
            || proof.destination != continuation.destination
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }
        let metadata_table_base_address = match proof.result_profile_status {
            P9X86_64BaselineOwnerCallResultProfileStatus::X86_64MetadataTableRelativeStore64 => {
                proof
                    .post_call_reentry_metadata_table_base_address
                    .filter(|address| *address != 0)
                    .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?
            }
            P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending
            | P9X86_64BaselineOwnerCallResultProfileStatus::DisabledByPolicy => proof
                .post_call_reentry_metadata_table_base_address
                .unwrap_or_else(|| NonNull::<c_void>::dangling().as_ptr() as usize),
        };
        {
            let mut state = DispatchState {
                stack: &mut self.execution,
                registers: &mut self.registers,
                exceptions: &mut self.exceptions,
                heap: &mut self.heap,
                code_block: target_code_block,
                ordinary_bytecode_call_handling: OrdinaryBytecodeCallHandling::DirectInterpreter,
                function_value_call_handling: FunctionValueCallHandling::DirectInterpreter,
            };
            pop_call_return_callee(&mut state, continuation)?;
            validate_call_return_continuation(state.stack, state.registers, &continuation)?;
        }
        Ok(P9OwnerPostCallReentryInvocation {
            entry_offset: proof.post_call_reentry_stub_start_offset,
            result_bits: value.encoded().0,
            metadata_table_base_address,
        })
    }

    pub(super) fn execute_generated_direct_call_callee_code_block<H: DispatchHost>(
        &mut self,
        continuation: CallReturnContinuation,
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        expected_frame: CallFrameId,
        argument_count_including_this: u32,
        preferred_route: Option<VmGeneratedDirectCallTransactionRoute>,
        route_policy: GeneratedDirectCallCalleeRoutePolicy,
        host: &mut H,
        config: DispatchConfig,
    ) -> GeneratedDirectCallCalleeExecution {
        if preferred_route == Some(VmGeneratedDirectCallTransactionRoute::NativeEntry)
            && matches!(
                route_policy,
                GeneratedDirectCallCalleeRoutePolicy::AnyAvailable
                    | GeneratedDirectCallCalleeRoutePolicy::NativeEntryOnly
            )
        {
            if let Some(execution) = self
                .try_execute_generated_direct_call_callee_with_native_entry(
                    code_block_id,
                    code_block,
                    expected_frame,
                    host,
                    config,
                    route_policy != GeneratedDirectCallCalleeRoutePolicy::NativeEntryOnly,
                )
            {
                self.tiering
                    .record_generated_direct_call_preferred_route_hit();
                return execution;
            }
        }

        if route_policy == GeneratedDirectCallCalleeRoutePolicy::NativeEntryOnly {
            return self
                .try_execute_generated_direct_call_callee_with_native_entry(
                    code_block_id,
                    code_block,
                    expected_frame,
                    host,
                    config,
                    false,
                )
                .unwrap_or_else(|| GeneratedDirectCallCalleeExecution {
                    completion: ExecutionCompletion::Failed(
                        ExecutionError::BaselineGeneratedExecutionRejected,
                    ),
                    route: VmGeneratedDirectCallTransactionRoute::NativeEntry,
                });
        }

        if route_policy == GeneratedDirectCallCalleeRoutePolicy::AnyAvailable {
            // C++ Baseline CallLinkInfo calls the entrypoint that linkFor()
            // prepared for the callee (JITCall.cpp compileOpCall ->
            // CallLinkInfo.cpp emitFastPathImpl). When Rust has both a sealed
            // native callable entry and a diagnostic generated artifact, the
            // native entry is the faithful target; the generated artifact still
            // exists as fallback/residency evidence.
            if let Some(execution) = self
                .try_execute_generated_direct_call_callee_with_native_entry(
                    code_block_id,
                    code_block,
                    expected_frame,
                    host,
                    config,
                    true,
                )
            {
                return execution;
            }
        }

        if preferred_route == Some(VmGeneratedDirectCallTransactionRoute::GeneratedEntry) {
            if let Some(execution) = self
                .try_execute_generated_direct_call_callee_with_generated_entry(
                    code_block_id,
                    code_block,
                    expected_frame,
                    host,
                    config,
                )
            {
                self.tiering
                    .record_generated_direct_call_preferred_route_hit();
                return execution;
            }
        }

        let generated = self.try_execute_generated_direct_call_callee_with_generated_entry(
            code_block_id,
            code_block,
            expected_frame,
            host,
            config,
        );
        if let Some(execution) = generated {
            return execution;
        }
        if let Some(execution) = self.try_prepare_host_blocked_generated_direct_call_callee_entry(
            code_block_id,
            code_block,
            expected_frame,
            host,
            config,
        ) {
            return execution;
        }
        if route_policy == GeneratedDirectCallCalleeRoutePolicy::GeneratedEntryOnly {
            return GeneratedDirectCallCalleeExecution {
                completion: ExecutionCompletion::Failed(
                    ExecutionError::BaselineGeneratedExecutionRejected,
                ),
                route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
            };
        }

        self.try_execute_generated_direct_call_callee_with_native_entry(
            code_block_id,
            code_block,
            expected_frame,
            host,
            config,
            true,
        )
        .unwrap_or_else(|| {
            let generated_entry_miss = self.generated_direct_call_generated_entry_miss_reason(
                code_block_id,
                code_block,
                expected_frame,
            );
            let native_entry_miss = self.generated_direct_call_native_entry_miss_reason(
                code_block_id,
                code_block,
                expected_frame,
            );
            self.record_generated_direct_call_callee_fallback(
                continuation,
                code_block_id,
                argument_count_including_this,
                preferred_route,
                generated_entry_miss,
                native_entry_miss,
            );
            GeneratedDirectCallCalleeExecution {
                completion: self.execute_nested_callee_code_block(
                    code_block_id,
                    code_block,
                    host,
                    config,
                ),
                route: VmGeneratedDirectCallTransactionRoute::NestedInterpreterFallback,
            }
        })
    }

    fn try_prepare_host_blocked_generated_direct_call_callee_entry<H: DispatchHost>(
        &mut self,
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        expected_frame: CallFrameId,
        host: &mut H,
        config: DispatchConfig,
    ) -> Option<GeneratedDirectCallCalleeExecution> {
        if self.generated_direct_call_generated_entry_miss_reason(
            code_block_id,
            code_block,
            expected_frame,
        ) != VmGeneratedDirectCallGeneratedEntryMissReason::MissingArtifact
            || self.generated_direct_call_native_entry_miss_reason(
                code_block_id,
                code_block,
                expected_frame,
            ) != VmGeneratedDirectCallNativeEntryMissReason::HostBlockedX86_64
            || !self.p15_host_blocked_native_generated_install_should_retry(code_block_id)
            || !self
                .selected_entry_code_block_matches_registered_code_block(code_block_id, code_block)
        {
            return None;
        }

        // C++ JSC's linkFor() prepares the JS callee executable before
        // returning the linked entrypoint (RepatchInlines.h:191). Rust may
        // already have x86_64 semantic native metadata for this callee on an
        // arm64 host; keep that audit artifact, but publish the portable
        // generated baseline body as the executable callee entry.
        let (generated, generated_detail) =
            self.p15_auto_install_generated_baseline_artifact(code_block_id);
        let request = BaselineEntryAutoMaterializationRequest {
            owner: code_block_id,
            requested_tier: JitType::Baseline,
            native:
                BaselineEntryAutoNativeMaterializationOutcome::SkippedHostBlockedX86_64NativeEntry,
            native_detail: None,
            generated: Some(generated),
            generated_detail,
        };
        self.tiering
            .record_baseline_entry_auto_materialization(request);

        self.try_execute_generated_direct_call_callee_with_generated_entry(
            code_block_id,
            code_block,
            expected_frame,
            host,
            config,
        )
    }

    fn try_execute_generated_direct_call_callee_with_generated_entry<H: DispatchHost>(
        &mut self,
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        expected_frame: CallFrameId,
        host: &mut H,
        config: DispatchConfig,
    ) -> Option<GeneratedDirectCallCalleeExecution> {
        let artifact = self
            .tiering
            .baseline_generated_code_artifact_for(code_block_id)?;
        if artifact.validate().is_err() || artifact.owner != code_block_id {
            return None;
        }
        let expected_snapshot = artifact.eligibility_proof.bytecode_snapshot_fingerprint();
        let actual_snapshot =
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block).ok()?;
        if actual_snapshot != expected_snapshot {
            return None;
        }
        let active_frame = self.execution.top_frame()?;
        if active_frame.id != expected_frame || active_frame.code_block != Some(code_block_id) {
            return None;
        }

        let region = self.enter_no_gc_execution_region();
        #[cfg(test)]
        self.record_no_gc_execution_depth_observation_for_test(
            VmNoGcExecutionPathForTest::GeneratedDirectCallCalleeGeneratedEntry,
        );
        let completion = self.execute_baseline_generated_code_in_current_region_with_entry_context(
            code_block_id,
            code_block,
            Some(expected_frame),
            crate::interpreter::ExecutionEntryKind::Function,
            JitType::Baseline,
            code_block_id,
            crate::interpreter::ExecutionEntryKind::Function,
            JitType::Baseline,
            host,
            config,
            BaselineGeneratedExecutionValidation::Prevalidated {
                bytecode_snapshot: actual_snapshot,
            },
        );
        let completion = self.drain_nested_callee_completion_in_current_region(
            completion,
            code_block_id,
            code_block,
            host,
            config,
        );
        Some(GeneratedDirectCallCalleeExecution {
            completion: self.leave_no_gc_region_with_completion(region, completion),
            route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
        })
    }

    fn try_execute_generated_direct_call_callee_with_native_entry<H: DispatchHost>(
        &mut self,
        code_block_id: CodeBlockId,
        code_block: &CodeBlock,
        expected_frame: CallFrameId,
        host: &mut H,
        config: DispatchConfig,
        allow_interpreter_fallback: bool,
    ) -> Option<GeneratedDirectCallCalleeExecution> {
        let gate = self
            .tiering
            .baseline_native_entry_gate_for_owner(code_block_id)?;
        if gate.outcome != BaselineEntryGateOutcome::NativeEntryReady {
            return None;
        }
        let active_frame = self.execution.top_frame()?;
        if active_frame.id != expected_frame || active_frame.code_block != Some(code_block_id) {
            return None;
        }

        let region = self.enter_no_gc_execution_region();
        let execution = self.try_execute_baseline_native_entry_shim_for_gate_in_current_region(
            code_block_id,
            code_block,
            Some(expected_frame),
            crate::interpreter::ExecutionEntryKind::Function,
            JitType::Baseline,
            &gate,
            host,
            config,
            allow_interpreter_fallback,
        );
        let Some(execution) = execution else {
            return match self.leave_no_gc_execution_region(region) {
                Ok(_) => None,
                Err(_) => Some(GeneratedDirectCallCalleeExecution {
                    completion: ExecutionCompletion::Failed(ExecutionError::GcBoundaryViolation),
                    route: VmGeneratedDirectCallTransactionRoute::NativeEntry,
                }),
            };
        };
        let (completion, route) = match execution {
            BaselineNativeEntryVmExecution::Native(completion) => (
                completion,
                VmGeneratedDirectCallTransactionRoute::NativeEntry,
            ),
            BaselineNativeEntryVmExecution::InterpreterFallback(completion) => (
                completion,
                VmGeneratedDirectCallTransactionRoute::NativeEntryInterpreterFallback,
            ),
            BaselineNativeEntryVmExecution::DeferredRootedInterpreterFallback {
                completion,
                roots,
            } => {
                let completion = self.drain_nested_callee_completion_in_current_region(
                    completion,
                    code_block_id,
                    code_block,
                    host,
                    config,
                );
                let completion = match self.cleanup_baseline_native_entry_deferred_roots(roots) {
                    Ok(()) => completion,
                    Err(error) => ExecutionCompletion::Failed(error),
                };
                return Some(GeneratedDirectCallCalleeExecution {
                    completion: self.leave_no_gc_region_with_completion(region, completion),
                    route: VmGeneratedDirectCallTransactionRoute::NativeEntryInterpreterFallback,
                });
            }
            BaselineNativeEntryVmExecution::P9OwnerPostCallReentry(_) => (
                ExecutionCompletion::Failed(ExecutionError::BaselineGeneratedExecutionRejected),
                VmGeneratedDirectCallTransactionRoute::NativeEntry,
            ),
            BaselineNativeEntryVmExecution::P6SideExitReentry(_) => (
                ExecutionCompletion::Failed(ExecutionError::BaselineGeneratedExecutionRejected),
                VmGeneratedDirectCallTransactionRoute::NativeEntry,
            ),
        };
        let completion = self.drain_nested_callee_completion_in_current_region(
            completion,
            code_block_id,
            code_block,
            host,
            config,
        );
        Some(GeneratedDirectCallCalleeExecution {
            completion: self.leave_no_gc_region_with_completion(region, completion),
            route,
        })
    }

    fn generated_js_direct_call_single_dispatch_outcome(
        &mut self,
        caller_code_block: &CodeBlock,
        outcome: DispatchOutcome,
        thrown: Option<PendingException>,
    ) -> SingleDispatchOutcome {
        match outcome {
            DispatchOutcome::ContinueTo(target) => {
                if target
                    .is_some_and(|target| caller_code_block.decoded_instruction_at(target).is_err())
                {
                    SingleDispatchOutcome::Failed(ExecutionError::InvalidBytecodeIndex(
                        target.unwrap(),
                    ))
                } else {
                    SingleDispatchOutcome::Continue(target)
                }
            }
            DispatchOutcome::Throw(value) => {
                if let Some(pending) = thrown {
                    SingleDispatchOutcome::Threw(pending)
                } else {
                    self.exceptions.throw(value);
                    let pending = self
                        .exceptions
                        .pending()
                        .unwrap_or(PendingException { value });
                    SingleDispatchOutcome::Threw(pending)
                }
            }
            DispatchOutcome::Fail(error) => SingleDispatchOutcome::Failed(error),
            DispatchOutcome::Return(value) => SingleDispatchOutcome::Return(value),
            DispatchOutcome::Suspend(record) => SingleDispatchOutcome::Suspended(record),
            DispatchOutcome::OrdinaryBytecodeCall(_) => {
                SingleDispatchOutcome::Failed(ExecutionError::InvalidCallCompletion)
            }
            DispatchOutcome::OrdinaryBytecodeConstruct(_) => {
                SingleDispatchOutcome::Failed(ExecutionError::InvalidCallCompletion)
            }
            DispatchOutcome::FunctionValueCall(_) => {
                SingleDispatchOutcome::Failed(ExecutionError::InvalidCallCompletion)
            }
            DispatchOutcome::EvalRequest(_) => {
                SingleDispatchOutcome::Failed(ExecutionError::InvalidCallCompletion)
            }
            DispatchOutcome::BaselineLoopHandoff(_) => {
                SingleDispatchOutcome::Failed(ExecutionError::BaselineGeneratedExecutionRejected)
            }
            DispatchOutcome::Continue | DispatchOutcome::Jump(_) => {
                SingleDispatchOutcome::Failed(ExecutionError::InvalidCallCompletion)
            }
        }
    }

    fn try_validate_generated_js_direct_call_hot_slot(
        &mut self,
        handoff: &BaselineGeneratedJsCallHandoff,
        code_block: &CodeBlock,
    ) -> Result<Option<GeneratedJsDirectCallValidation>, ExecutionError> {
        let Some(direct) = handoff.direct_call.as_deref() else {
            return Ok(None);
        };
        let Some(slot) = self.generated_direct_call_hot_slot_for_handoff(handoff, direct) else {
            return Ok(None);
        };
        self.validate_generated_js_call_handoff_continuation(handoff, code_block)?;
        let call_continuation = handoff
            .continuation
            .as_call()
            .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;

        let active_frame = self
            .execution
            .top_frame()
            .ok_or(ExecutionError::NoActiveFrame)?;
        if active_frame.id != handoff.resume.frame
            || active_frame.code_block != Some(handoff.resume.owner)
            || active_frame.bytecode_index != Some(handoff.resume.bytecode_index)
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }
        if call_continuation.callee_value != Some(direct.callee_value)
            || call_continuation.callee_object != Some(slot.callee_object)
            || call_continuation.argument_count_including_this != slot.argument_count_including_this
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let target = self
            .validate_call_link_metadata_target(
                slot.candidate.target.executable,
                slot.candidate.target.target_code_block,
                slot.candidate.target.callee,
            )
            .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        if target.executable != slot.candidate.target.executable
            || target.target_code_block != slot.candidate.target.target_code_block
            || target.callee != slot.candidate.target.callee
            || target.specialization != CodeSpecialization::Call
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        // C++ JSC divergence (one shared instance): share the registered `Rc`
        // (refcount bump) instead of a deep clone so the per-instance memo stays warm.
        let target_code_block = self
            .code_blocks
            .code_block_shared(target.target_code_block)
            .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
        let argument_values = self.generated_js_direct_call_argument_values(
            handoff,
            code_block,
            call_continuation,
            direct,
            &target_code_block,
        )?;

        self.tiering.record_generated_direct_call_hot_slot_hit();
        Ok(Some(GeneratedJsDirectCallValidation {
            owner: handoff.resume.owner,
            opcode: handoff.resume.opcode,
            bytecode_index: handoff.resume.bytecode_index,
            continuation: *call_continuation,
            target_code_block_id: target.target_code_block,
            target_code_block,
            argument_values,
            direct_call: direct.clone(),
            hot_slot_hit: true,
            preferred_route: Some(slot.preferred_route),
            rootless_generated_entry_proof: slot.rootless_generated_entry_proof,
        }))
    }

    fn generated_js_direct_call_argument_values(
        &self,
        handoff: &BaselineGeneratedJsCallHandoff,
        code_block: &CodeBlock,
        call_continuation: &CallReturnContinuation,
        direct: &BaselineGeneratedJsDirectCall,
        target_code_block: &CodeBlock,
    ) -> Result<Vec<RuntimeValue>, ExecutionError> {
        let instruction = code_block
            .decoded_instruction_at(handoff.resume.bytecode_index)
            .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        let caller_window =
            validate_call_return_continuation(&self.execution, &self.registers, call_continuation)
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        let (this_value, first_argument_operand) = match handoff.resume.opcode {
            CoreOpcode::Call => (RuntimeValue::undefined(), 3usize),
            CoreOpcode::CallWithThis => {
                let this_register = instruction
                    .register_operand(2)
                    .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
                let this_value = self
                    .registers
                    .read(caller_window, this_register, Some(code_block.constants()))
                    .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
                (this_value, 4usize)
            }
            _ => return Err(ExecutionError::BaselineGeneratedExecutionRejected),
        };
        let this_object = this_value
            .as_cell()
            .and_then(|cell| self.heap.cell_for_payload(cell.pointer_payload_bits()))
            .map(ObjectId);
        if direct.this_value != this_value || direct.this_object != this_object {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let provided_argument_count = direct.argument_count_including_this.saturating_sub(1);
        let mut argument_values = Vec::with_capacity(
            usize::try_from(direct.argument_count_including_this).unwrap_or(usize::MAX),
        );
        argument_values.push(this_value);
        for argument_index in 0..provided_argument_count {
            let operand_index = usize::try_from(argument_index)
                .unwrap_or(usize::MAX)
                .saturating_add(first_argument_operand);
            let register = instruction
                .register_operand(operand_index)
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
            let value = self
                .registers
                .read(caller_window, register, Some(code_block.constants()))
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
            argument_values.push(value);
        }

        let formal_parameter_count = target_code_block
            .unlinked()
            .frame()
            .num_parameters_including_this
            .saturating_sub(1);
        while argument_values.len()
            < usize::try_from(formal_parameter_count.saturating_add(1)).unwrap_or(usize::MAX)
        {
            argument_values.push(RuntimeValue::undefined());
        }

        Ok(argument_values)
    }

    fn validate_generated_js_direct_call_handoff(
        &self,
        handoff: &BaselineGeneratedJsCallHandoff,
        code_block: &CodeBlock,
    ) -> Result<GeneratedJsDirectCallValidation, ExecutionError> {
        let direct = handoff
            .direct_call
            .as_deref()
            .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
        self.validate_generated_js_call_handoff_continuation(handoff, code_block)?;
        let call_continuation = handoff
            .continuation
            .as_call()
            .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;

        let active_frame = self
            .execution
            .top_frame()
            .ok_or(ExecutionError::NoActiveFrame)?;
        if active_frame.id != handoff.resume.frame
            || active_frame.code_block != Some(handoff.resume.owner)
            || active_frame.bytecode_index != Some(handoff.resume.bytecode_index)
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }
        if call_continuation.callee_value != Some(direct.callee_value)
            || call_continuation.callee_object != Some(direct.callee_object)
            || call_continuation.argument_count_including_this
                != direct.argument_count_including_this
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let bytecode_snapshot =
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block)
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        let registered_code_block = self
            .code_blocks
            .get(handoff.resume.owner)
            .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?
            .code_block();
        let registered_snapshot =
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(
                registered_code_block,
            )
            .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        if registered_snapshot != bytecode_snapshot {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }
        if let Some(artifact) = self
            .tiering
            .baseline_generated_code_artifact_for(handoff.resume.owner)
        {
            artifact
                .validate()
                .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
            if artifact.eligibility_proof.bytecode_snapshot_fingerprint() != bytecode_snapshot {
                return Err(ExecutionError::BaselineGeneratedExecutionRejected);
            }
        }

        let direct_argument_count = u8::try_from(direct.argument_count_including_this)
            .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        if direct.authorization != GeneratedCallLinkDirectCall::from_candidate(&direct.candidate) {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let current_candidate_table = self.generated_call_link_candidate_table_for_owner_cached(
            handoff.resume.owner,
            bytecode_snapshot,
        )?;
        if !current_candidate_table
            .candidates_for_bytecode_index(handoff.resume.bytecode_index.offset())
            .any(|candidate| candidate == &direct.candidate)
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let mut unsupported_blockers = direct.candidate.remaining_blockers;
        unsupported_blockers.remove(CallLinkReadinessBlocker::MayCallJsBoundary);
        if direct.candidate.owner != handoff.resume.owner
            || direct.candidate.opcode != handoff.resume.opcode
            || direct.candidate.bytecode_index != handoff.resume.bytecode_index.offset()
            || direct.candidate.direct_call_status != GeneratedCallLinkDirectCallStatus::Authorized
            || !unsupported_blockers.is_empty()
            || direct.candidate.target.specialization != CodeSpecialization::Call
            || direct.candidate.target.executable != direct.authorization.target_executable
            || direct.candidate.target.callee != direct.authorization.target_callee
            || direct.candidate.target.target_code_block != direct.authorization.target_code_block
            || direct.candidate.boundary.id != direct.authorization.target_boundary
            || direct.candidate.target.callee != direct.callee_object
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let descriptor = direct.candidate.descriptor;
        if descriptor.owner != Some(handoff.resume.owner)
            || descriptor.mode != CallLinkMode::Monomorphic
            || descriptor.call_kind != LinkedCallKind::Call
            || descriptor.executable != Some(direct.candidate.target.executable)
            || descriptor.callee != Some(direct.candidate.target.callee)
            || descriptor.target_code_block != Some(direct.candidate.target.target_code_block)
            || descriptor.boundary != Some(direct.candidate.boundary.id)
            || descriptor.max_argument_count_including_this != direct_argument_count
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let boundary = &direct.candidate.boundary;
        if boundary.owner != Some(handoff.resume.owner)
            || boundary.abi != EntryAbi::LlIntCompatible
            || boundary.entry_kind != EntrypointKind::InterpreterThunk
            || boundary.native_symbol.is_some()
            || boundary.arguments.len() != usize::from(direct_argument_count)
            || boundary
                .arguments
                .iter()
                .any(|argument| *argument != AbiValue::JsValue)
            || boundary.returns.as_slice() != [AbiValue::JsValue]
            || !boundary.requires_vm_entry_scope
            || !boundary.may_call_js
            || !boundary.may_throw
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        let target = self
            .validate_call_link_metadata_target(
                direct.candidate.target.executable,
                direct.candidate.target.target_code_block,
                direct.candidate.target.callee,
            )
            .map_err(|_| ExecutionError::BaselineGeneratedExecutionRejected)?;
        if target.executable != direct.candidate.target.executable
            || target.target_code_block != direct.candidate.target.target_code_block
            || target.callee != direct.candidate.target.callee
            || target.specialization != CodeSpecialization::Call
        {
            return Err(ExecutionError::BaselineGeneratedExecutionRejected);
        }

        // C++ JSC divergence (one shared instance): share the registered `Rc`
        // (refcount bump) instead of a deep clone so the per-instance memo stays warm.
        let target_code_block = self
            .code_blocks
            .code_block_shared(target.target_code_block)
            .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
        let argument_values = self.generated_js_direct_call_argument_values(
            handoff,
            code_block,
            call_continuation,
            direct,
            &target_code_block,
        )?;

        Ok(GeneratedJsDirectCallValidation {
            owner: handoff.resume.owner,
            opcode: handoff.resume.opcode,
            bytecode_index: handoff.resume.bytecode_index,
            continuation: *call_continuation,
            target_code_block_id: target.target_code_block,
            target_code_block,
            argument_values,
            direct_call: direct.clone(),
            hot_slot_hit: false,
            preferred_route: None,
            rootless_generated_entry_proof: None,
        })
    }
}
