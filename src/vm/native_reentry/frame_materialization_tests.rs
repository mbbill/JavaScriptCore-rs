use super::super::super::arm64_native_entry::Arm64NativeEntryJscStackDispatchRequestError;
use super::super::super::entry::FrameAddress;
use super::super::arm64_exception_unwind::{
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof,
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError,
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch,
    P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord, P6Arm64VmEntryExceptionHandlerTarget,
    P6Arm64VmEntryExceptionHandlerTargetKind,
    P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch,
    P6Arm64VmEntryExceptionUnwindStagingRecord,
    P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord,
};
use super::super::arm64_public_dispatch::{
    P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord,
    P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
    P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord,
    P6Arm64PublicJscStackDispatchPlatformRequestField,
    P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch,
    P6Arm64PublicJscStackDispatchVmEntryWindowRecord,
    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof,
    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError,
};
use super::super::arm64_vm_entry_normal_return::{
    P6Arm64VerifiedVmEntryNormalReturnRestorationProof,
    P6Arm64VerifiedVmEntryNormalReturnRestorationProofError,
    P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch,
    P6Arm64VmEntryNormalReturnRestorationLinkageMismatch,
};
use super::super::rooting::{
    P6Arm64JscStackDispatchExecutableEntryProofError,
    P6Arm64JscStackDispatchMaterializationLinkageMismatch,
    P6Arm64VerifiedGeneratedNativeFrameMaterializationProofError,
    P6Arm64VerifiedJscStackDispatchRequestProof, P6Arm64VerifiedJscStackDispatchRequestProofError,
    P6Arm64VerifiedJscStackDispatchRequestProofMismatch,
};
use super::tests;
use super::*;
use crate::bytecode::BytecodeIndex;
use crate::bytecode::{
    CodeBlockLifecycleState, ExecutableAritySelection, ExecutableBaselineNativeEntryRecord,
    ExecutableBaselineNativeEntrySelection, ExecutableEntryCacheKey, ExecutableEntryCacheRecord,
    JitCodeSlot,
};
use crate::jit::arm64_baseline::{
    Arm64BaselineGeneratedNativeFrameMaterializationMismatch, JSC_REGISTER_BYTES,
    JSC_STACK_ALIGNMENT_BYTES,
};
use crate::jit::code::BaselineEntryArtifact;
use crate::jit::{
    BaselineNativeEntryToken, BaselineNativeEntryTokenKind, CodeFinalizationAuthority,
    CodeLiveness, CodeOrigin, CodeOriginKind, CodeOwnership, CodeRetentionPolicy, EntryAbi,
    ExecutableAllocationLifecycle, ExecutableMemoryProtection, JitCodeArtifact, JitType,
};
use crate::platform::executable_memory_compartment::ExecutableMemoryArm64JscStackCallRequest;
use crate::runtime::NativeCodeId;
use core::ffi::c_void;
use core::ptr::NonNull;

fn attach_materialization_descriptor_to_fixture(
    fixture: &mut tests::NativeFrameResidencyFixture<'_>,
) {
    let call_frame = fixture
        .top_call_frame_publication
        .publication
        .published_top_frame
        .0;
    let expected_live_local_slots = 1;
    let required_live_local_bytes = expected_live_local_slots * JSC_REGISTER_BYTES;
    let aligned_frame_bytes = required_live_local_bytes.next_multiple_of(JSC_STACK_ALIGNMENT_BYTES);
    let aligned_post_allocation_sp = call_frame - aligned_frame_bytes;
    let frame_top_offset_bytes =
        isize::try_from(aligned_post_allocation_sp).unwrap() - isize::try_from(call_frame).unwrap();
    fixture.native_frame_residency_proof = fixture
        .native_frame_residency_proof
        .clone()
        .with_generated_native_frame_materialization(
            &fixture.top_call_frame_publication,
            frame_top_offset_bytes,
            u32::try_from(expected_live_local_slots).unwrap(),
        )
        .expect("fixture should attach generated native frame materialization");
}

fn assert_missing_verified_generated_materialization(
    result: Result<Infallible, P6Arm64BranchAwareCallableAdmissionRejection<'_>>,
) {
    assert!(matches!(
        result,
        Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64GeneratedNativeFrameMaterializationProof { .. })
    ));
}

fn caught_exception_dispatch_restore_record(
    normal_return_restoration_proof: &P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'_>,
) -> P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord {
    let dispatch = normal_return_restoration_proof
        .jsc_stack_dispatch_request_proof()
        .jsc_stack_dispatch_request_proof();
    let materialization = normal_return_restoration_proof
        .jsc_stack_dispatch_request_proof()
        .generated_native_frame_materialization_proof()
        .materialization_descriptor();
    P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord {
        staging: P6Arm64VmEntryExceptionUnwindStagingRecord {
            call_frame_for_catch: dispatch.call_frame,
            target_machine_pc_for_throw: P6Arm64VmEntryExceptionHandlerTarget {
                kind: P6Arm64VmEntryExceptionHandlerTargetKind::LlIntOpCatch,
                address: NativeCodeId(701),
            },
            target_machine_pc_after_catch: Some(P6Arm64VmEntryExceptionHandlerTarget {
                kind: P6Arm64VmEntryExceptionHandlerTargetKind::DispatchAndCatch,
                address: NativeCodeId(702),
            }),
            target_interpreter_pc_for_throw: Some(BytecodeIndex::from_offset(12)),
            target_interpreter_metadata_pc_for_throw: Some(0x1230),
            target_try_depth_for_throw: 2,
        },
        callee_save_restore: P6Arm64VmEntryCalleeSaveBufferRestorationRecord {
            buffer: dispatch.vm_entry_record_callee_save_buffer,
            register_count: dispatch.vm_entry_record_callee_save_register_count,
            buffer_bytes: dispatch.vm_entry_record_callee_save_buffer_bytes,
        },
        reconstructed_catch_sp: FrameAddress(
            materialization.post_frame_allocation.post_allocation_sp,
        ),
        pending_exception_cleared: true,
        catchable_exception_retrieved: true,
        exception_operand_store_frame: dispatch.call_frame,
        thrown_value_operand_store_frame: dispatch.call_frame,
        catch_profile_recorded: true,
        dispatches_after_catch: true,
    }
}

fn uncaught_exception_entry_restore_record(
    normal_return_restoration_proof: &P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'_>,
) -> P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord {
    let dispatch = normal_return_restoration_proof
        .jsc_stack_dispatch_request_proof()
        .jsc_stack_dispatch_request_proof();
    let exit_record = normal_return_restoration_proof.normal_return_exit_record();
    P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord {
        staging: P6Arm64VmEntryExceptionUnwindStagingRecord {
            call_frame_for_catch: dispatch.call_frame,
            target_machine_pc_for_throw: P6Arm64VmEntryExceptionHandlerTarget {
                kind: P6Arm64VmEntryExceptionHandlerTargetKind::LlIntHandleUncaughtException,
                address: NativeCodeId(801),
            },
            target_machine_pc_after_catch: None,
            target_interpreter_pc_for_throw: None,
            target_interpreter_metadata_pc_for_throw: None,
            target_try_depth_for_throw: 0,
        },
        callee_save_restore: P6Arm64VmEntryCalleeSaveBufferRestorationRecord {
            buffer: dispatch.vm_entry_record_callee_save_buffer,
            register_count: dispatch.vm_entry_record_callee_save_register_count,
            buffer_bytes: dispatch.vm_entry_record_callee_save_buffer_bytes,
        },
        top_entry_frame_loaded: exit_record.closed_publication.top_entry_frame,
        vm_entry_record: exit_record.closed_publication.vm_entry_record,
        restored_top_call_frame: exit_record.restored_top_call_frame,
        restored_top_entry_frame: exit_record.restored_top_entry_frame,
        call_frame_for_catch_cleared: true,
        returned_undefined: true,
    }
}

fn verified_exception_unwind_restoration_proof_from_normal_return<'publication>(
    normal_return_restoration_proof: P6Arm64VerifiedVmEntryNormalReturnRestorationProof<
        'publication,
    >,
) -> Result<
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication>,
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError,
> {
    let caught = caught_exception_dispatch_restore_record(&normal_return_restoration_proof);
    let uncaught = uncaught_exception_entry_restore_record(&normal_return_restoration_proof);
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof::from_exception_unwind_restoration_records(
        normal_return_restoration_proof,
        caught,
        uncaught,
    )
}

fn full_exception_unwind_restoration_fallback<'publication>(
    fixture: &tests::NativeFrameResidencyFixture<'publication>,
    vm_entry_exception_unwind_restoration_proof:
        P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication>,
) -> P6Arm64BranchAwareCallableFallbackRootingProof<'publication> {
    P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceVmEntryNormalReturnAndExceptionUnwindRestorationProof {
        top_call_frame_publication: fixture.top_call_frame_publication,
        machine_stack_conservative_rooting_proof: fixture
            .machine_stack_conservative_rooting_proof
            .clone(),
        vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
        conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
        collector_effects_plan: fixture.collector_effects_plan.clone(),
        verifier_append_proof: fixture.verifier_append_proof.clone(),
        jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
        vm_entry_exception_unwind_restoration_proof,
    }
}

fn baseline_entry_artifact_from_token(
    token: BaselineNativeEntryToken,
    liveness: CodeLiveness,
) -> BaselineEntryArtifact {
    JitCodeArtifact {
        id: token.artifact_id,
        tier: JitType::Baseline,
        origin: CodeOrigin {
            kind: CodeOriginKind::BaselineCodeBlock,
            owner: Some(token.owner),
            executable: None,
            bytecode_index: Some(0),
        },
        ownership: CodeOwnership::CodeBlockOwned,
        native_code: Some(token.native_symbol),
        machine_code: Some(token.machine_code),
        entrypoint: token.entrypoint,
        patchpoints: Vec::new(),
        dependencies: Vec::new(),
        byproducts: Vec::new(),
        disassembly: None,
        liveness,
        finalization_authority: CodeFinalizationAuthority::MainThread,
    }
    .validate_baseline_entry_artifact(token.owner)
    .unwrap_or_else(|_| {
        let live = JitCodeArtifact {
            id: token.artifact_id,
            tier: JitType::Baseline,
            origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: Some(token.owner),
                executable: None,
                bytecode_index: Some(0),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: Some(token.native_symbol),
            machine_code: Some(token.machine_code),
            entrypoint: token.entrypoint,
            patchpoints: Vec::new(),
            dependencies: Vec::new(),
            byproducts: Vec::new(),
            disassembly: None,
            liveness: CodeLiveness::Live,
            finalization_authority: CodeFinalizationAuthority::MainThread,
        }
        .validate_baseline_entry_artifact(token.owner)
        .expect("live baseline artifact from selected token");
        BaselineEntryArtifact { liveness, ..live }
    })
}

fn executable_entry_cache_record_from_token(
    token: BaselineNativeEntryToken,
    baseline_jit_slot: JitCodeSlot,
) -> ExecutableEntryCacheRecord {
    ExecutableEntryCacheRecord {
        key: ExecutableEntryCacheKey::new(
            crate::runtime::CodeSpecializationKind::Call,
            ExecutableAritySelection::AlreadyChecked,
        ),
        executable: None,
        owner: token.owner,
        code_block: token.owner,
        readiness_ordinal: 7,
        baseline_jit_slot,
        baseline_native_entry: ExecutableBaselineNativeEntryRecord {
            artifact_id: token.artifact_id,
            native_symbol: token.native_symbol,
            machine_code: token.machine_code,
            machine_range: token.machine_code.range,
            entrypoint: token.entrypoint,
            selection: ExecutableBaselineNativeEntrySelection::Normal(token),
        },
    }
}

fn public_dispatch_precondition_records<'publication>(
    exception_unwind_restoration_proof: &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<
        'publication,
    >,
) -> (
    P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
    P6Arm64PublicJscStackDispatchVmEntryWindowRecord,
    P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord,
    P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord,
) {
    let dispatch_proof = exception_unwind_restoration_proof
        .vm_entry_normal_return_restoration_proof()
        .jsc_stack_dispatch_request_proof();
    let token = dispatch_proof.executable_entry().selected_token;
    let baseline_jit_slot = JitCodeSlot(313);
    let executable_lifetime = P6Arm64PublicJscStackDispatchExecutableLifetimeRecord {
        baseline_entry_artifact: baseline_entry_artifact_from_token(token, CodeLiveness::Live),
        executable_entry_record: executable_entry_cache_record_from_token(token, baseline_jit_slot),
        selected_token: token,
        current_baseline_jit_slot: baseline_jit_slot,
        code_block_lifecycle: CodeBlockLifecycleState::BaselineInstalled,
        code_liveness: CodeLiveness::Live,
        retention_policy: CodeRetentionPolicy::CodeBlockKeepsAlive,
        machine_code: token.machine_code,
        entry_offset: dispatch_proof.executable_entry().entry_offset,
    };
    let vm_entry_window = P6Arm64PublicJscStackDispatchVmEntryWindowRecord {
        traps_deferred: true,
        no_gc: true,
        retained_jit_code_ref: true,
    };
    let caught = exception_unwind_restoration_proof.caught_exception_dispatch_restore();
    let frame_extent_bytes = caught
        .staging
        .call_frame_for_catch
        .0
        .checked_sub(caught.reconstructed_catch_sp.0)
        .expect("fixture catch SP is below CallFrame");
    let code_block_frame_extent = P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord {
        code_block: token.owner,
        call_frame: caught.staging.call_frame_for_catch,
        num_callee_locals: 0,
        max_frame_extent_for_slow_path_call_bytes: frame_extent_bytes,
        restored_stack_pointer: caught.reconstructed_catch_sp,
    };
    let platform_envelope = P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord {
        platform_request: dispatch_proof
            .jsc_stack_dispatch_request_proof()
            .platform_request,
        supports_normal_return: true,
        supports_caught_exception_exit: false,
        supports_uncaught_exception_exit: false,
    };
    (
        executable_lifetime,
        vm_entry_window,
        code_block_frame_extent,
        platform_envelope,
    )
}

fn verified_public_dispatch_preconditions_proof_from_exception_unwind<'publication>(
    exception_unwind_restoration_proof: P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<
        'publication,
    >,
) -> Result<
    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'publication>,
    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError,
> {
    let (executable_lifetime, vm_entry_window, code_block_frame_extent, platform_envelope) =
        public_dispatch_precondition_records(&exception_unwind_restoration_proof);
    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
        exception_unwind_restoration_proof,
        executable_lifetime,
        vm_entry_window,
        code_block_frame_extent,
        platform_envelope,
    )
}

fn full_public_dispatch_preconditions_fallback<'publication>(
    fixture: &tests::NativeFrameResidencyFixture<'publication>,
    public_jsc_stack_dispatch_preconditions_proof:
        P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'publication>,
) -> P6Arm64BranchAwareCallableFallbackRootingProof<'publication> {
    P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceExceptionUnwindAndPublicJscStackDispatchPreconditionsProof {
        top_call_frame_publication: fixture.top_call_frame_publication,
        machine_stack_conservative_rooting_proof: fixture
            .machine_stack_conservative_rooting_proof
            .clone(),
        vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
        conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
        collector_effects_plan: fixture.collector_effects_plan.clone(),
        verifier_append_proof: fixture.verifier_append_proof.clone(),
        jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
        public_jsc_stack_dispatch_preconditions_proof,
    }
}

fn non_null_c_void(address: usize) -> NonNull<c_void> {
    NonNull::new(address as *mut c_void).expect("non-null test address")
}

#[test]
fn public_arm64_branch_aware_admission_ignores_raw_materialized_frame_descriptor() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_native_frame_residency_fixture(|mut fixture| {
        let mut request = tests::valid_request(&side_exits);
        attach_materialization_descriptor_to_fixture(&mut fixture);
        request.fallback_rooting_proof = fixture.fallback();

        assert_missing_verified_generated_materialization(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
        );
    });
}

#[test]
fn arm64_materialized_frame_descriptor_uses_actual_argument_count_not_padding() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let mut fixture =
            tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
        assert_eq!(
            fixture
                .top_call_frame_publication
                .publication
                .argument_count_excluding_this,
            2
        );
        assert_eq!(
            fixture
                .top_call_frame_publication
                .publication
                .padded_argument_count,
            5
        );
        attach_materialization_descriptor_to_fixture(&mut fixture);

        let descriptor = fixture
            .native_frame_residency_proof
            .residency_proof()
            .generated_native_frame_materialization
            .as_ref()
            .expect("attached frame materialization descriptor");
        assert_eq!(descriptor.header.arguments.len(), 2);
        assert_eq!(descriptor.header.live_locals.len(), 1);

        let mut request = tests::valid_request(&side_exits);
        request.fallback_rooting_proof = fixture.fallback();
        assert_missing_verified_generated_materialization(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
        );
    });
}

#[test]
fn public_arm64_branch_aware_admission_reaches_stack_dispatch_blocker_after_verified_materialized_frame(
) {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_and_stack_call(
        |top_call_frame_publication, stack_call_proof| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let generated_native_frame_materialization_proof =
                tests::verified_generated_native_frame_materialization_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    1,
                )
                .expect("stack-call materialization proof should verify");

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof =
                tests::full_generated_native_frame_materialization_fallback(
                    fixture.top_call_frame_publication,
                    fixture.machine_stack_conservative_rooting_proof.clone(),
                    fixture.vm_root_gather_plan.clone(),
                    fixture.conservative_root_marking_plan.clone(),
                    fixture.collector_effects_plan.clone(),
                    fixture.verifier_append_proof.clone(),
                    fixture.jit_stub_trace_plan.clone(),
                    generated_native_frame_materialization_proof.clone(),
                );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64JscStackDispatchAdmissionAuthority {
                        top_call_frame_publication: fixture.top_call_frame_publication,
                        conservative_scan_append_receipt: fixture.conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
                        collector_effects_plan: fixture.collector_effects_plan.clone(),
                        verifier_append_proof: fixture.verifier_append_proof.clone(),
                        jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
                        generated_native_frame_materialization_proof,
                    }
                )
            );
        },
    );
}

#[test]
fn public_arm64_branch_aware_admission_reaches_vm_entry_exit_restoration_blocker_after_verified_dispatch(
) {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_stack_call_and_frame(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof = tests::full_jsc_stack_dispatch_request_fallback(
                fixture.top_call_frame_publication,
                fixture.machine_stack_conservative_rooting_proof.clone(),
                fixture.vm_root_gather_plan.clone(),
                fixture.conservative_root_marking_plan.clone(),
                fixture.collector_effects_plan.clone(),
                fixture.verifier_append_proof.clone(),
                fixture.jit_stub_trace_plan.clone(),
                jsc_stack_dispatch_request_proof.clone(),
            );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64VmEntryExitRestorationAuthority {
                        top_call_frame_publication: fixture.top_call_frame_publication,
                        conservative_scan_append_receipt: fixture.conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
                        collector_effects_plan: fixture.collector_effects_plan.clone(),
                        verifier_append_proof: fixture.verifier_append_proof.clone(),
                        jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
                        jsc_stack_dispatch_request_proof,
                    }
                )
            );
        },
    );
}

#[test]
fn public_arm64_branch_aware_admission_reaches_exception_unwind_blocker_after_normal_return_restoration(
) {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof =
                tests::full_vm_entry_normal_return_restoration_fallback(
                    fixture.top_call_frame_publication,
                    fixture.machine_stack_conservative_rooting_proof.clone(),
                    fixture.vm_root_gather_plan.clone(),
                    fixture.conservative_root_marking_plan.clone(),
                    fixture.collector_effects_plan.clone(),
                    fixture.verifier_append_proof.clone(),
                    fixture.jit_stub_trace_plan.clone(),
                    normal_return_restoration_proof.clone(),
                );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64VmEntryExceptionUnwindRestorationAuthority {
                        top_call_frame_publication: fixture.top_call_frame_publication,
                        conservative_scan_append_receipt: fixture.conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
                        collector_effects_plan: fixture.collector_effects_plan.clone(),
                        verifier_append_proof: fixture.verifier_append_proof.clone(),
                        jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
                        vm_entry_normal_return_restoration_proof: normal_return_restoration_proof,
                    }
                )
            );
        },
    );
}

#[test]
fn public_arm64_branch_aware_admission_reaches_public_jsc_stack_dispatch_execution_blocker_after_exception_unwind_restoration(
) {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof = full_exception_unwind_restoration_fallback(
                &fixture,
                exception_unwind_restoration_proof.clone(),
            );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64PublicJscStackDispatchExecutionAuthority {
                        top_call_frame_publication: fixture.top_call_frame_publication,
                        conservative_scan_append_receipt: fixture.conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
                        collector_effects_plan: fixture.collector_effects_plan.clone(),
                        verifier_append_proof: fixture.verifier_append_proof.clone(),
                        jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
                        vm_entry_exception_unwind_restoration_proof:
                            exception_unwind_restoration_proof,
                    }
                )
            );
        },
    );
}

#[test]
fn arm64_public_dispatch_preconditions_advance_admission_to_exception_exit_routing_blocker() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");
            let public_dispatch_preconditions_proof =
                verified_public_dispatch_preconditions_proof_from_exception_unwind(
                    exception_unwind_restoration_proof,
                )
                .expect("public dispatch preconditions proof should verify");

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof = full_public_dispatch_preconditions_fallback(
                &fixture,
                public_dispatch_preconditions_proof.clone(),
            );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64PublicJscStackDispatchExceptionExitRoutingAuthority {
                        top_call_frame_publication: fixture.top_call_frame_publication,
                        conservative_scan_append_receipt: fixture.conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
                        collector_effects_plan: fixture.collector_effects_plan.clone(),
                        verifier_append_proof: fixture.verifier_append_proof.clone(),
                        jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
                        public_jsc_stack_dispatch_preconditions_proof:
                            public_dispatch_preconditions_proof,
                    }
                )
            );
        },
    );
}

#[test]
fn arm64_public_dispatch_preconditions_reject_non_live_or_jettisoning_executable_evidence() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");
            let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            executable_lifetime.baseline_entry_artifact.liveness = CodeLiveness::PendingJettison;
            executable_lifetime.code_liveness = CodeLiveness::PendingJettison;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime.clone(),
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactLivenessMismatch {
                            actual: CodeLiveness::PendingJettison,
                        },
                    )
                )
            );

            for lifecycle in [
                CodeBlockLifecycleState::Jettisoned,
                CodeBlockLifecycleState::Finalizing,
                CodeBlockLifecycleState::Destructed,
            ] {
                let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                    public_dispatch_precondition_records(&exception_unwind_restoration_proof);
                executable_lifetime.code_block_lifecycle = lifecycle;
                assert_eq!(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                        exception_unwind_restoration_proof.clone(),
                        executable_lifetime,
                        vm_entry_window,
                        frame_extent,
                        platform_envelope,
                    ),
                    Err(
                        P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::CodeBlockLifecycleMismatch {
                                actual: lifecycle,
                            },
                        )
                    )
                );
            }

            let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            executable_lifetime.machine_code.protection = ExecutableMemoryProtection::Writable;
            executable_lifetime.machine_code.lifecycle =
                ExecutableAllocationLifecycle::AllocatedWritable;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof,
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::MachineCodeNotExecutable {
                            protection: ExecutableMemoryProtection::Writable,
                            lifecycle: ExecutableAllocationLifecycle::AllocatedWritable,
                        },
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_public_dispatch_preconditions_reject_executable_identity_drift() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");
            let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            executable_lifetime.selected_token.native_symbol = NativeCodeId(9_001);
            assert!(matches!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::SelectedTokenMismatch {
                            ..
                        },
                    )
                )
            ));

            let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            executable_lifetime.baseline_entry_artifact.id = crate::jit::JitCodeId(9_002);
            assert!(matches!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactIdMismatch {
                            ..
                        },
                    )
                )
            ));

            let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            executable_lifetime.baseline_entry_artifact.native_code = NativeCodeId(9_003);
            assert!(matches!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactNativeSymbolMismatch {
                            ..
                        },
                    )
                )
            ));

            let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            executable_lifetime.entry_offset += 4;
            assert!(matches!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::EntryOffsetMismatch {
                            ..
                        },
                    )
                )
            ));

            let (mut executable_lifetime, vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            executable_lifetime.machine_code.symbol = Some(NativeCodeId(9_004));
            assert!(matches!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof,
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::MachineCodeDrift {
                            ..
                        },
                    )
                )
            ));
        },
    );
}

#[test]
fn arm64_public_dispatch_preconditions_reject_platform_request_identity_drift() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");

            let cases: [(
                fn(&mut ExecutableMemoryArm64JscStackCallRequest),
                P6Arm64PublicJscStackDispatchPlatformRequestField,
            ); 7] = [
                (
                    |request: &mut ExecutableMemoryArm64JscStackCallRequest| {
                        request.entry_offset += JSC_REGISTER_BYTES as u32;
                    },
                    P6Arm64PublicJscStackDispatchPlatformRequestField::EntryOffset,
                ),
                (
                    |request: &mut ExecutableMemoryArm64JscStackCallRequest| {
                        request.call_frame = non_null_c_void(
                            request.call_frame.as_ptr() as usize + JSC_REGISTER_BYTES,
                        );
                    },
                    P6Arm64PublicJscStackDispatchPlatformRequestField::CallFrame,
                ),
                (
                    |request: &mut ExecutableMemoryArm64JscStackCallRequest| {
                        request.entry_sp = non_null_c_void(
                            request.entry_sp.as_ptr() as usize + JSC_REGISTER_BYTES,
                        );
                    },
                    P6Arm64PublicJscStackDispatchPlatformRequestField::EntryStackPointer,
                ),
                (
                    |request: &mut ExecutableMemoryArm64JscStackCallRequest| {
                        request.entry_frame = non_null_c_void(
                            request.entry_frame.as_ptr() as usize + JSC_REGISTER_BYTES,
                        );
                    },
                    P6Arm64PublicJscStackDispatchPlatformRequestField::EntryFrame,
                ),
                (
                    |request: &mut ExecutableMemoryArm64JscStackCallRequest| {
                        request.vm_entry_record_callee_save_buffer = non_null_c_void(
                            request.vm_entry_record_callee_save_buffer.as_ptr() as usize
                                + JSC_REGISTER_BYTES,
                        );
                    },
                    P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveBuffer,
                ),
                (
                    |request: &mut ExecutableMemoryArm64JscStackCallRequest| {
                        request.vm_entry_record_callee_save_register_count += 1;
                    },
                    P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveRegisterCount,
                ),
                (
                    |request: &mut ExecutableMemoryArm64JscStackCallRequest| {
                        request.vm_entry_record_callee_save_buffer_bytes += JSC_REGISTER_BYTES;
                    },
                    P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveBufferBytes,
                ),
            ];
            for (mutator, expected_field) in cases {
                let (executable_lifetime, vm_entry_window, frame_extent, mut platform_envelope) =
                    public_dispatch_precondition_records(&exception_unwind_restoration_proof);
                mutator(&mut platform_envelope.platform_request);
                assert!(matches!(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                        exception_unwind_restoration_proof.clone(),
                        executable_lifetime,
                        vm_entry_window,
                        frame_extent,
                        platform_envelope,
                    ),
                    Err(
                        P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformRequestMismatch {
                                field,
                                ..
                            },
                        )
                    ) if field == expected_field
                ));
            }
        },
    );
}

#[test]
fn arm64_public_dispatch_preconditions_reject_code_block_frame_extent_mismatch() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");
            let (executable_lifetime, vm_entry_window, mut frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            let expected = frame_extent.restored_stack_pointer;
            frame_extent.restored_stack_pointer = FrameAddress(expected.0 + JSC_REGISTER_BYTES);

            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime.clone(),
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::FrameExtentRestoredStackPointerMismatch {
                            expected,
                            actual: frame_extent.restored_stack_pointer,
                        },
                    )
                )
            );

            let (executable_lifetime, vm_entry_window, mut frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            let caught_sp = exception_unwind_restoration_proof
                .caught_exception_dispatch_restore()
                .reconstructed_catch_sp;
            let reduced_extent = frame_extent
                .max_frame_extent_for_slow_path_call_bytes
                .checked_sub(JSC_REGISTER_BYTES)
                .expect("fixture frame extent leaves room for one register word");
            frame_extent.max_frame_extent_for_slow_path_call_bytes = reduced_extent;
            frame_extent.restored_stack_pointer = FrameAddress(caught_sp.0 + JSC_REGISTER_BYTES);
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof,
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::CaughtRestoreStackPointerMismatch {
                            expected: frame_extent.restored_stack_pointer,
                            actual: caught_sp,
                        },
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_public_dispatch_preconditions_reject_missing_vm_entry_window_guards() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");

            let (executable_lifetime, mut vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            vm_entry_window.traps_deferred = false;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime.clone(),
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::TrapDeferralMissing,
                    )
                )
            );

            let (executable_lifetime, mut vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            vm_entry_window.no_gc = false;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime.clone(),
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::NoGcMissing,
                    )
                )
            );

            let (executable_lifetime, mut vm_entry_window, frame_extent, platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            vm_entry_window.retained_jit_code_ref = false;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof,
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::RetainedJitCodeRefMissing,
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_public_dispatch_preconditions_reject_caught_or_uncaught_platform_exit_claims() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");

            let (executable_lifetime, vm_entry_window, frame_extent, mut platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            platform_envelope.supports_normal_return = false;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformNormalReturnUnsupported,
                    )
                )
            );

            let (executable_lifetime, vm_entry_window, frame_extent, mut platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            platform_envelope.supports_caught_exception_exit = true;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof.clone(),
                    executable_lifetime.clone(),
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformCaughtExceptionExitClaimed,
                    )
                )
            );

            let (executable_lifetime, vm_entry_window, frame_extent, mut platform_envelope) =
                public_dispatch_precondition_records(&exception_unwind_restoration_proof);
            platform_envelope.supports_uncaught_exception_exit = true;
            assert_eq!(
                P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof::from_public_jsc_stack_dispatch_preconditions(
                    exception_unwind_restoration_proof,
                    executable_lifetime,
                    vm_entry_window,
                    frame_extent,
                    platform_envelope,
                ),
                Err(
                    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage(
                        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformUncaughtExceptionExitClaimed,
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_exception_unwind_wrapper_rejects_forged_caught_restore_record() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let mut caught =
                caught_exception_dispatch_restore_record(&normal_return_restoration_proof);
            caught.pending_exception_cleared = false;
            let uncaught =
                uncaught_exception_entry_restore_record(&normal_return_restoration_proof);

            assert_eq!(
                P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof::from_exception_unwind_restoration_records(
                    normal_return_restoration_proof,
                    caught,
                    uncaught,
                ),
                Err(
                    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError::Linkage(
                        P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtPendingExceptionNotCleared,
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_exception_unwind_wrapper_rejects_zero_caught_handler_targets() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");

            let mut caught =
                caught_exception_dispatch_restore_record(&normal_return_restoration_proof);
            caught.staging.target_machine_pc_for_throw.address = NativeCodeId::default();
            let uncaught =
                uncaught_exception_entry_restore_record(&normal_return_restoration_proof);
            assert_eq!(
                P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof::from_exception_unwind_restoration_records(
                    normal_return_restoration_proof.clone(),
                    caught,
                    uncaught,
                ),
                Err(
                    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError::Linkage(
                        P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtTargetAddressZero,
                    )
                )
            );

            let mut caught =
                caught_exception_dispatch_restore_record(&normal_return_restoration_proof);
            caught
                .staging
                .target_machine_pc_after_catch
                .as_mut()
                .expect("fixture has dispatch-and-catch target")
                .address = NativeCodeId::default();
            let uncaught =
                uncaught_exception_entry_restore_record(&normal_return_restoration_proof);
            assert_eq!(
                P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof::from_exception_unwind_restoration_records(
                    normal_return_restoration_proof,
                    caught,
                    uncaught,
                ),
                Err(
                    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError::Linkage(
                        P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtDispatchAndCatchTargetAddressZero,
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_exception_unwind_wrapper_rejects_forged_uncaught_restore_record() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let caught = caught_exception_dispatch_restore_record(&normal_return_restoration_proof);
            let mut uncaught =
                uncaught_exception_entry_restore_record(&normal_return_restoration_proof);
            uncaught.returned_undefined = false;

            assert_eq!(
                P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof::from_exception_unwind_restoration_records(
                    normal_return_restoration_proof,
                    caught,
                    uncaught,
                ),
                Err(
                    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError::Linkage(
                        P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtUndefinedReturnMissing,
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_exception_unwind_wrapper_rejects_zero_uncaught_handler_target() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let caught = caught_exception_dispatch_restore_record(&normal_return_restoration_proof);
            let mut uncaught =
                uncaught_exception_entry_restore_record(&normal_return_restoration_proof);
            uncaught.staging.target_machine_pc_for_throw.address = NativeCodeId::default();

            assert_eq!(
                P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof::from_exception_unwind_restoration_records(
                    normal_return_restoration_proof,
                    caught,
                    uncaught,
                ),
                Err(
                    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError::Linkage(
                        P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtTargetAddressZero,
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_exception_unwind_wrapper_rejects_cross_paired_restore_records() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");

            tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
                |other_top_call_frame_publication,
                 other_stack_call_proof,
                 other_stack_frame_proof,
                 other_exit_record| {
                    let other_fixture = tests::native_frame_residency_fixture_for_publication(
                        other_top_call_frame_publication,
                    );
                    let other_jsc_stack_dispatch_request_proof =
                        tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                            &other_fixture,
                            &other_stack_call_proof,
                            other_stack_frame_proof,
                            1,
                        )
                        .expect("other JSC stack dispatch request proof should verify");
                    let other_normal_return_restoration_proof =
                        tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                            &other_fixture,
                            other_jsc_stack_dispatch_request_proof,
                            other_exit_record,
                        )
                        .expect("other VM-entry normal return proof should verify");
                    let caught = caught_exception_dispatch_restore_record(
                        &other_normal_return_restoration_proof,
                    );
                    let uncaught = uncaught_exception_entry_restore_record(
                        &other_normal_return_restoration_proof,
                    );

                    assert!(matches!(
                        P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof::from_exception_unwind_restoration_records(
                            normal_return_restoration_proof,
                            caught,
                            uncaught,
                        ),
                        Err(
                            P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError::Linkage(
                                P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtCallFrameForCatchMismatch {
                                    ..
                                },
                            )
                        )
                    ));
                },
            );
        },
    );
}

#[test]
fn public_arm64_branch_aware_admission_rejects_exception_unwind_admission_time_drift() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let expected = FrameAddress(
                normal_return_restoration_proof
                    .jsc_stack_dispatch_request_proof()
                    .generated_native_frame_materialization_proof()
                    .materialization_descriptor()
                    .post_frame_allocation
                    .post_allocation_sp,
            );
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");
            let actual = FrameAddress(expected.0 + JSC_REGISTER_BYTES);
            let mut caught =
                *exception_unwind_restoration_proof.caught_exception_dispatch_restore();
            caught.reconstructed_catch_sp = actual;
            let exception_unwind_restoration_proof = exception_unwind_restoration_proof
                .with_caught_exception_dispatch_restore_for_testing(caught);

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof = full_exception_unwind_restoration_fallback(
                &fixture,
                exception_unwind_restoration_proof,
            );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::Arm64VmEntryExceptionUnwindRestorationAuthorityMismatch {
                        mismatch:
                            P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch::Linkage(
                                P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtReconstructedStackPointerMismatch {
                                    expected,
                                    actual,
                                },
                            ),
                    }
                )
            );
        },
    );
}

#[test]
fn arm64_vm_entry_normal_return_wrapper_rejects_restored_previous_top_pair_drift() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, mut exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let actual = Some(FrameAddress(0xfeed_0000));
            exit_record.restored_top_call_frame = actual;

            assert_eq!(
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                ),
                Err(
                    P6Arm64VerifiedVmEntryNormalReturnRestorationProofError::Linkage(
                        P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::RestoredTopCallFrameMismatch {
                            expected: None,
                            actual,
                        },
                    )
                )
            );
        },
    );
}

#[test]
fn public_arm64_branch_aware_admission_rejects_normal_return_restored_top_pair_drift() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let actual = Some(FrameAddress(0xfeed_1000));
            let mut stale_exit_record =
                *normal_return_restoration_proof.normal_return_exit_record();
            stale_exit_record.restored_top_entry_frame = actual;
            let normal_return_restoration_proof = normal_return_restoration_proof
                .with_normal_return_exit_record_for_testing(stale_exit_record);

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof =
                tests::full_vm_entry_normal_return_restoration_fallback(
                    fixture.top_call_frame_publication,
                    fixture.machine_stack_conservative_rooting_proof.clone(),
                    fixture.vm_root_gather_plan.clone(),
                    fixture.conservative_root_marking_plan.clone(),
                    fixture.collector_effects_plan.clone(),
                    fixture.verifier_append_proof.clone(),
                    fixture.jit_stub_trace_plan.clone(),
                    normal_return_restoration_proof,
                );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::Arm64VmEntryNormalReturnRestorationAuthorityMismatch {
                        mismatch:
                            P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch::Linkage(
                                P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::RestoredTopEntryFrameMismatch {
                                    expected: None,
                                    actual,
                                },
                            ),
                    }
                )
            );
        },
    );
}

#[test]
fn arm64_vm_entry_normal_return_wrapper_rejects_cross_paired_dispatch_proof() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, _stack_call_proof, _stack_frame_proof, exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
                |_other_top_call_frame_publication,
                 other_stack_call_proof,
                 other_stack_frame_proof,
                 _other_exit_record| {
                    let other_fixture = tests::native_frame_residency_fixture_for_publication(
                        _other_top_call_frame_publication,
                    );
                    let other_jsc_stack_dispatch_request_proof =
                        tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                            &other_fixture,
                            &other_stack_call_proof,
                            other_stack_frame_proof,
                            1,
                        )
                        .expect("other JSC stack dispatch request proof should verify");

                    assert!(matches!(
                        tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                            &fixture,
                            other_jsc_stack_dispatch_request_proof,
                            exit_record,
                        ),
                        Err(
                            P6Arm64VerifiedVmEntryNormalReturnRestorationProofError::Linkage(
                                P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::DispatchCallFrameMismatch {
                                    ..
                                },
                            )
                        )
                    ));
                },
            );
        },
    );
}

#[test]
fn arm64_vm_entry_normal_return_wrapper_rejects_cross_paired_exit_record() {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, _exit_record| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");

            tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
                |_other_top_call_frame_publication,
                 _other_stack_call_proof,
                 _other_stack_frame_proof,
                 other_exit_record| {
                    assert!(matches!(
                        tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                            &fixture,
                            jsc_stack_dispatch_request_proof,
                            other_exit_record,
                        ),
                        Err(
                            P6Arm64VerifiedVmEntryNormalReturnRestorationProofError::Linkage(
                                P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedTopCallFrameMismatch {
                                    ..
                                },
                            )
                        )
                    ));
                },
            );
        },
    );
}

#[test]
fn arm64_jsc_stack_dispatch_wrapper_rejects_cross_paired_stack_frame() {
    tests::with_stack_top_call_frame_publication_stack_call_and_frame(
        |top_call_frame_publication, stack_call_proof, _stack_frame_proof| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let generated_native_frame_materialization_proof =
                tests::verified_generated_native_frame_materialization_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    1,
                )
                .expect("stack-call materialization proof should verify");

            tests::with_stack_top_call_frame_publication_stack_call_and_frame(
                |_other_top_call_frame_publication,
                 _other_stack_call_proof,
                 other_stack_frame_proof| {
                    assert!(matches!(
                        P6Arm64VerifiedJscStackDispatchRequestProof::from_jsc_stack_dispatch_request_proof(
                            generated_native_frame_materialization_proof.clone(),
                            &stack_call_proof,
                            other_stack_frame_proof,
                            stack_call_proof.selected_token(),
                        ),
                        Err(P6Arm64VerifiedJscStackDispatchRequestProofError::StackDispatch(
                            Arm64NativeEntryJscStackDispatchRequestError::StackFrameProofMismatch {
                                ..
                            }
                        ))
                    ));
                },
            );
        },
    );
}

#[test]
fn public_arm64_branch_aware_admission_rejects_dispatch_entry_sp_drift() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_stack_top_call_frame_publication_stack_call_and_frame(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let jsc_stack_dispatch_request_proof =
                tests::verified_jsc_stack_dispatch_request_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    stack_frame_proof,
                    1,
                )
                .expect("JSC stack dispatch request proof should verify");
            let expected_entry_sp = jsc_stack_dispatch_request_proof
                .jsc_stack_dispatch_request_proof()
                .entry_sp
                .0;
            let actual_entry_sp = expected_entry_sp + JSC_REGISTER_BYTES;
            let jsc_stack_dispatch_request_proof = jsc_stack_dispatch_request_proof
                .with_dispatch_entry_sp_for_testing(FrameAddress(actual_entry_sp));

            let mut request = tests::valid_request(&side_exits);
            request.fallback_rooting_proof = tests::full_jsc_stack_dispatch_request_fallback(
                fixture.top_call_frame_publication,
                fixture.machine_stack_conservative_rooting_proof.clone(),
                fixture.vm_root_gather_plan.clone(),
                fixture.conservative_root_marking_plan.clone(),
                fixture.collector_effects_plan.clone(),
                fixture.verifier_append_proof.clone(),
                fixture.jit_stub_trace_plan.clone(),
                jsc_stack_dispatch_request_proof,
            );

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::Arm64JscStackDispatchAdmissionAuthorityMismatch {
                        mismatch: P6Arm64VerifiedJscStackDispatchRequestProofMismatch::Linkage(
                            P6Arm64JscStackDispatchMaterializationLinkageMismatch::EntryStackPointerMismatch {
                                expected: expected_entry_sp,
                                actual: actual_entry_sp,
                            },
                        ),
                    }
                )
            );
        },
    );
}

#[test]
fn arm64_jsc_stack_dispatch_wrapper_rejects_bad_executable_entry_metadata() {
    tests::with_stack_top_call_frame_publication_stack_call_and_frame(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let generated_native_frame_materialization_proof =
                tests::verified_generated_native_frame_materialization_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    1,
                )
                .expect("stack-call materialization proof should verify");

            let mut arity_token = stack_call_proof.selected_token();
            arity_token.kind = BaselineNativeEntryTokenKind::ArityCheck;
            assert_eq!(
                P6Arm64VerifiedJscStackDispatchRequestProof::from_jsc_stack_dispatch_request_proof(
                    generated_native_frame_materialization_proof.clone(),
                    &stack_call_proof,
                    stack_frame_proof,
                    arity_token,
                ),
                Err(P6Arm64VerifiedJscStackDispatchRequestProofError::ExecutableEntry(
                    P6Arm64JscStackDispatchExecutableEntryProofError::SelectedTokenKindMismatch {
                        actual: BaselineNativeEntryTokenKind::ArityCheck,
                    },
                ))
            );

            let mut wrong_abi_token = stack_call_proof.selected_token();
            wrong_abi_token.entrypoint.abi = EntryAbi::Rust;
            assert_eq!(
                P6Arm64VerifiedJscStackDispatchRequestProof::from_jsc_stack_dispatch_request_proof(
                    generated_native_frame_materialization_proof.clone(),
                    &stack_call_proof,
                    stack_frame_proof,
                    wrong_abi_token,
                ),
                Err(
                    P6Arm64VerifiedJscStackDispatchRequestProofError::ExecutableEntry(
                        P6Arm64JscStackDispatchExecutableEntryProofError::EntrypointAbiMismatch {
                            actual: EntryAbi::Rust,
                        },
                    )
                )
            );

            let mut writable_token = stack_call_proof.selected_token();
            writable_token.machine_code.protection = ExecutableMemoryProtection::Writable;
            assert!(matches!(
                P6Arm64VerifiedJscStackDispatchRequestProof::from_jsc_stack_dispatch_request_proof(
                    generated_native_frame_materialization_proof,
                    &stack_call_proof,
                    stack_frame_proof,
                    writable_token,
                ),
                Err(P6Arm64VerifiedJscStackDispatchRequestProofError::ExecutableEntry(
                    P6Arm64JscStackDispatchExecutableEntryProofError::MachineCodeNotExecutable {
                        protection: ExecutableMemoryProtection::Writable,
                        ..
                    },
                ))
            ));
        },
    );
}

#[test]
fn arm64_materialized_frame_wrapper_rejects_stack_publication_live_local_drift() {
    tests::with_stack_top_call_frame_publication_and_stack_call(
        |top_call_frame_publication, stack_call_proof| {
            let mut fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            fixture
                .top_call_frame_publication
                .publication
                .live_local_count = 0;

            assert_eq!(
                tests::verified_generated_native_frame_materialization_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    1,
                ),
                Err(
                    P6Arm64VerifiedGeneratedNativeFrameMaterializationProofError::Materialization(
                        Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotCountMismatch {
                            expected: 1,
                            actual: 0,
                        },
                    )
                )
            );
        },
    );
}

#[test]
fn arm64_materialized_frame_wrapper_rejects_cross_paired_stack_call_frame() {
    tests::with_stack_top_call_frame_publication_and_stack_call(
        |top_call_frame_publication, _stack_call_proof| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);

            tests::with_stack_top_call_frame_publication_and_stack_call(
                |_other_top_call_frame_publication, other_stack_call_proof| {
                    match tests::verified_generated_native_frame_materialization_proof_from_stack_call(
                        &fixture,
                        &other_stack_call_proof,
                        1,
                    ) {
                        Err(
                            P6Arm64VerifiedGeneratedNativeFrameMaterializationProofError::Materialization(
                                Arm64BaselineGeneratedNativeFrameMaterializationMismatch::PublishedTopFrameMismatch {
                                    frame_pointer,
                                    published_top_frame,
                                },
                            ),
                        ) => {
                            assert_eq!(
                                published_top_frame,
                                fixture
                                    .top_call_frame_publication
                                    .publication
                                    .published_top_frame
                                    .0
                            );
                            assert_ne!(frame_pointer, published_top_frame);
                        }
                        actual => {
                            panic!("expected cross-paired stack-call rejection, got {actual:?}")
                        }
                    }
                },
            );
        },
    );
}

#[test]
fn arm64_materialized_frame_validation_rejects_entry_sp_drift() {
    tests::with_stack_top_call_frame_publication_and_stack_call(
        |top_call_frame_publication, stack_call_proof| {
            let fixture =
                tests::native_frame_residency_fixture_for_publication(top_call_frame_publication);
            let generated_native_frame_materialization_proof =
                tests::verified_generated_native_frame_materialization_proof_from_stack_call(
                    &fixture,
                    &stack_call_proof,
                    1,
                )
                .expect("stack-call materialization proof should verify");
            let mut descriptor = generated_native_frame_materialization_proof
                .materialization_descriptor()
                .clone();
            let expected_entry_sp = descriptor.prologue.entry_sp;
            descriptor.prologue.entry_sp += JSC_REGISTER_BYTES;

            assert_eq!(
                validate_p6_arm64_generated_native_frame_materialization_proof(
                    &fixture.top_call_frame_publication,
                    generated_native_frame_materialization_proof
                        .native_frame_residency_proof()
                        .residency_proof(),
                    &descriptor,
                    1,
                ),
                Err(
                    Arm64BaselineGeneratedNativeFrameMaterializationMismatch::EntryStackPointerMismatch {
                        expected: expected_entry_sp,
                        actual: descriptor.prologue.entry_sp,
                    }
                )
            );
        },
    );
}
