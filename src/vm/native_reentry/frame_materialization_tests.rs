use super::super::rooting::P6Arm64VerifiedGeneratedNativeFrameMaterializationProofError;
use super::tests;
use super::*;
use crate::jit::arm64_baseline::{
    Arm64BaselineGeneratedNativeFrameMaterializationMismatch, JSC_REGISTER_BYTES,
    JSC_STACK_ALIGNMENT_BYTES,
};

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
