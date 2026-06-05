use super::super::rooting::{
    P6Arm64BranchAwareCallableFallbackRootingProof,
    P6Arm64MachineStackConservativeRootingProofMismatch,
    P6Arm64NativeFrameMachineStackResidencyProofMismatch,
    P6Arm64VerifiedNativeFrameMachineStackResidencyProof,
    P6Arm64VerifiedNativeFrameMachineStackResidencyProofError,
};
use super::tests;
use super::*;
use crate::gc::{ConservativeRootSpan, ConservativeRoots};

#[test]
fn verified_native_frame_residency_constructor_rejects_top_frame_outside_scanned_stack_span() {
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut machine_stack_proof = fixture.machine_stack_conservative_rooting_proof.clone();
        let register_span = machine_stack_proof.machine_stack_spans[0].span;
        let stack_span = ConservativeRootSpan {
            begin: 0x2000,
            end: 0x2100,
        };
        let root = fixture.conservative_scan_append_receipt.append_plan.records[0].root;
        machine_stack_proof.machine_stack_spans[1].span = stack_span;
        machine_stack_proof.machine_stack_roots = roots_for_spans(register_span, stack_span, root);

        assert_eq!(
            P6Arm64VerifiedNativeFrameMachineStackResidencyProof::from_machine_stack_conservative_rooting_proof(
                &fixture.top_call_frame_publication,
                &machine_stack_proof,
                tests::native_frame_root_slot_records_for_publication(
                    fixture.top_call_frame_publication,
                    &fixture.conservative_scan_append_receipt,
                ),
            ),
            Err(P6Arm64VerifiedNativeFrameMachineStackResidencyProofError::MachineStack(
                P6Arm64MachineStackConservativeRootingProofMismatch::TopCallFrameOutsideScannedSpans {
                    address: fixture
                        .top_call_frame_publication
                        .publication
                        .published_top_frame,
                },
            ))
        );
    });
}

#[test]
fn verified_native_frame_residency_constructor_rejects_unaligned_live_root_slot() {
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut slot_records = tests::native_frame_root_slot_records_for_publication(
            fixture.top_call_frame_publication,
            &fixture.conservative_scan_append_receipt,
        );
        let slot_address = fixture
            .machine_stack_conservative_rooting_proof
            .machine_stack_spans[1]
            .span
            .begin
            + 1;
        slot_records[0].slot_address = slot_address;

        assert_eq!(
            P6Arm64VerifiedNativeFrameMachineStackResidencyProof::from_machine_stack_conservative_rooting_proof(
                &fixture.top_call_frame_publication,
                &fixture.machine_stack_conservative_rooting_proof,
                slot_records,
            ),
            Err(P6Arm64VerifiedNativeFrameMachineStackResidencyProofError::Residency(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotAddressUnaligned {
                    order: 0,
                    slot_address,
                },
            ))
        );
    });
}

#[test]
fn verified_native_frame_residency_constructor_rejects_live_root_absent_from_machine_stack_roots() {
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut machine_stack_proof = fixture.machine_stack_conservative_rooting_proof.clone();
        let mut roots = ConservativeRoots::new();
        for span in &machine_stack_proof.machine_stack_spans {
            roots.add_span(span.span);
        }
        machine_stack_proof.machine_stack_roots = roots;
        let slot_records = tests::native_frame_root_slot_records_for_publication(
            fixture.top_call_frame_publication,
            &fixture.conservative_scan_append_receipt,
        );
        let root = slot_records[0].expected_root;

        assert_eq!(
            P6Arm64VerifiedNativeFrameMachineStackResidencyProof::from_machine_stack_conservative_rooting_proof(
                &fixture.top_call_frame_publication,
                &machine_stack_proof,
                slot_records,
            ),
            Err(P6Arm64VerifiedNativeFrameMachineStackResidencyProofError::Residency(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotRootAbsentFromMachineStackRoots {
                    order: 0,
                    root,
                },
            ))
        );
    });
}

#[test]
fn public_arm64_residency_stage_rejects_cross_paired_machine_stack_proof() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut machine_stack_proof = fixture.machine_stack_conservative_rooting_proof.clone();
        let register_span = machine_stack_proof.machine_stack_spans[0].span;
        let stack_span = machine_stack_proof.machine_stack_spans[1].span;
        let expanded_stack_span = ConservativeRootSpan {
            begin: stack_span.begin - core::mem::size_of::<usize>(),
            end: stack_span.end + core::mem::size_of::<usize>(),
        };
        let root = fixture.conservative_scan_append_receipt.append_plan.records[0].root;
        machine_stack_proof.machine_stack_spans[1].span = expanded_stack_span;
        machine_stack_proof.machine_stack_roots =
            roots_for_spans(register_span, expanded_stack_span, root);

        let mut request = tests::valid_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceAndMachineStackResidencyProof {
                top_call_frame_publication: fixture.top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_proof.clone(),
                vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
                conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
                collector_effects_plan: fixture.collector_effects_plan.clone(),
                verifier_append_proof: fixture.verifier_append_proof.clone(),
                jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
                native_frame_residency_proof: fixture.native_frame_residency_proof.clone(),
            };

        match p6_arm64_public_branch_aware_callable_admission_proof(&request) {
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NativeFrameMachineStackResidencyProofMismatch {
                    mismatch,
                    ..
                },
            ) => assert_eq!(
                mismatch,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::ResidencySourceMachineStackSpansMismatch {
                    source: machine_stack_proof.machine_stack_spans,
                    residency: fixture
                        .native_frame_residency_proof
                        .residency_proof()
                        .machine_stack_spans
                        .clone(),
                }
            ),
            actual => panic!("expected cross-paired residency rejection, got {actual:?}"),
        }
    });
}

fn roots_for_spans(
    register_span: ConservativeRootSpan,
    stack_span: ConservativeRootSpan,
    root: crate::gc::ConservativeRootCell,
) -> ConservativeRoots {
    let mut roots = ConservativeRoots::new();
    roots.add_span(register_span);
    roots.add_span(stack_span);
    roots.add_validated_cell(root);
    roots
}
