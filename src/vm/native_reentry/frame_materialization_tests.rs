use super::super::super::arm64_native_entry::Arm64NativeEntryJscStackDispatchRequestError;
use super::super::super::entry::FrameAddress;
use super::super::rooting::{
    P6Arm64JscStackDispatchExecutableEntryProofError,
    P6Arm64JscStackDispatchMaterializationLinkageMismatch,
    P6Arm64VerifiedGeneratedNativeFrameMaterializationProofError,
    P6Arm64VerifiedJscStackDispatchRequestProof, P6Arm64VerifiedJscStackDispatchRequestProofError,
    P6Arm64VerifiedJscStackDispatchRequestProofMismatch,
};
use super::tests;
use super::*;
use crate::jit::arm64_baseline::{
    Arm64BaselineGeneratedNativeFrameMaterializationMismatch, JSC_REGISTER_BYTES,
    JSC_STACK_ALIGNMENT_BYTES,
};
use crate::jit::{BaselineNativeEntryTokenKind, EntryAbi, ExecutableMemoryProtection};

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
