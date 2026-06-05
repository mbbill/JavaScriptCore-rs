use super::super::rooting::P6Arm64BranchAwareCallableFallbackRootingProof;
use super::tests;
use super::*;
use crate::gc::VerifierSlotVisitorConservativeRootAppendProof;

fn branch_request<'a, 'publication>(
    side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
) -> P6Arm64BranchAwareCallableAdmissionProofRequest<'a, 'publication> {
    tests::valid_request(side_exits)
}

#[test]
fn public_arm64_verifier_stage_rejects_collector_effects_without_verifier_state_proof() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let (
            conservative_scan_append_receipt,
            conservative_root_marking_plan,
            collector_effects_plan,
        ) = tests::conservative_root_marking_and_collector_effects_proof();
        let machine_stack_conservative_rooting_proof =
            tests::machine_stack_conservative_rooting_proof_from_marker(
                top_call_frame_publication,
                &conservative_scan_append_receipt,
            );
        let vm_root_gather_plan = tests::vm_root_gather_proof(&conservative_scan_append_receipt);
        let mut request = branch_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherAndCollectorEffectsPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
                vm_root_gather_plan: vm_root_gather_plan.clone(),
                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                collector_effects_plan: collector_effects_plan.clone(),
            };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingVerifierSlotVisitorAppendOrAbsenceProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                    conservative_root_marking_plan,
                    collector_effects_plan,
                }
            )
        );
    });
}

#[test]
fn public_arm64_verifier_stage_accepts_no_verifier_absence_and_rejects_missing_jit_trace() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let (
            conservative_scan_append_receipt,
            conservative_root_marking_plan,
            collector_effects_plan,
        ) = tests::conservative_root_marking_and_collector_effects_proof();
        let machine_stack_conservative_rooting_proof =
            tests::machine_stack_conservative_rooting_proof_from_marker(
                top_call_frame_publication,
                &conservative_scan_append_receipt,
            );
        let vm_root_gather_plan = tests::vm_root_gather_proof(&conservative_scan_append_receipt);
        let verifier_append_proof = tests::verifier_append_proof(&collector_effects_plan);
        let mut request = branch_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsAndVerifierAppendProof {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
                vm_root_gather_plan: vm_root_gather_plan.clone(),
                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                collector_effects_plan: collector_effects_plan.clone(),
                verifier_append_proof: verifier_append_proof.clone(),
            };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingJitStubRoutineTraceProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                    conservative_root_marking_plan,
                    collector_effects_plan,
                    verifier_append_proof,
                }
            )
        );
    });
}

#[test]
fn arm64_verifier_absence_proof_is_derived_from_collector_effects_proof() {
    let (_, _, collector_effects_proof) =
        tests::conservative_root_marking_and_collector_effects_proof();
    let collector_effects_plan = collector_effects_proof.collector_effects_plan();
    let verifier_append_proof = tests::verifier_append_proof(&collector_effects_proof);

    assert_eq!(verifier_append_proof.heap(), collector_effects_plan.heap);
    assert_eq!(
        verifier_append_proof.marking_epoch(),
        collector_effects_plan.marking_epoch
    );
    assert!(matches!(
        verifier_append_proof.verifier_append_proof(),
        VerifierSlotVisitorConservativeRootAppendProof::NoVerifierSlotVisitor {
            heap,
            marking_epoch
        } if *heap == collector_effects_plan.heap
            && *marking_epoch == collector_effects_plan.marking_epoch
    ));
}
