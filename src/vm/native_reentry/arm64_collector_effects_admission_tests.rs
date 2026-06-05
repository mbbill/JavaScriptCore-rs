use super::super::rooting::{
    P6Arm64BranchAwareCallableFallbackRootingProof, P6Arm64CollectorEffectsProofMismatch,
    P6Arm64SlotVisitorCollectorEffectsProof, P6Arm64SlotVisitorConservativeRootMarkingProof,
};
use super::tests;
use super::*;
use crate::gc::SlotVisitorCollectorEffectAction;

fn branch_request<'a, 'publication>(
    side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
) -> P6Arm64BranchAwareCallableAdmissionProofRequest<'a, 'publication> {
    tests::valid_request(side_exits)
}

#[test]
fn public_arm64_collector_effects_stage_rejects_marking_without_effects_proof() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let (mut heap, conservative_scan_append_receipt) =
            tests::heap_with_conservative_scan_append_receipt();
        let conservative_root_marking_plan =
            P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
                &conservative_scan_append_receipt,
                &mut heap,
            )
            .expect("heap-produced conservative-root marking proof");
        let machine_stack_conservative_rooting_proof =
            tests::machine_stack_conservative_rooting_proof_from_marker(
                top_call_frame_publication,
                &conservative_scan_append_receipt,
            );
        let vm_root_gather_plan = tests::vm_root_gather_proof(&conservative_scan_append_receipt);
        let mut request = branch_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherAndConservativeRootMarkingPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
                vm_root_gather_plan: vm_root_gather_plan.clone(),
                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
            };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingRealCollectorMarkStackCellStateAndContainerProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                    conservative_root_marking_plan,
                }
            )
        );
    });
}

#[test]
fn public_arm64_collector_effects_stage_progresses_with_heap_produced_effects_proof() {
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
fn public_arm64_collector_effects_stage_rejects_proof_from_prior_marking_state() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let (mut heap, conservative_scan_append_receipt) =
            tests::heap_with_conservative_scan_append_receipt();
        let first_marking_proof =
            P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
                &conservative_scan_append_receipt,
                &mut heap,
            )
            .expect("first heap-produced conservative-root marking proof");
        let collector_effects_plan =
            P6Arm64SlotVisitorCollectorEffectsProof::from_conservative_root_marking_proof(
                &first_marking_proof,
                &mut heap,
            )
            .expect("first heap-produced collector-effects proof");
        let conservative_root_marking_plan =
            P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
                &conservative_scan_append_receipt,
                &mut heap,
            )
            .expect("second heap-produced conservative-root marking proof");
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
                P6Arm64BranchAwareCallableAdmissionRejection::CollectorEffectsProofMismatch {
                    top_call_frame_publication,
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                    conservative_root_marking_plan,
                    collector_effects_plan,
                    mismatch:
                        P6Arm64CollectorEffectsProofMismatch::CollectorMarkingRecordMismatch {
                            order: 0,
                        },
                }
            )
        );
    });
}

#[test]
fn arm64_collector_effects_proof_constructor_reflects_already_marked_heap_state() {
    let (mut heap, conservative_scan_append_receipt) =
        tests::heap_with_conservative_scan_append_receipt();
    let first_marking_proof =
        P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
            &conservative_scan_append_receipt,
            &mut heap,
        )
        .expect("first heap-produced conservative-root marking proof");
    let first_collector_effects_proof =
        P6Arm64SlotVisitorCollectorEffectsProof::from_conservative_root_marking_proof(
            &first_marking_proof,
            &mut heap,
        )
        .expect("first heap-produced collector-effects proof");
    let first_effects_plan = first_collector_effects_proof.collector_effects_plan();
    assert_eq!(first_effects_plan.mark_stack_append_count, 1);
    assert_eq!(first_effects_plan.already_marked_count, 0);
    assert_eq!(
        first_effects_plan.records[0].action,
        expected_p6_arm64_collector_effect_action(&first_marking_proof.marking_plan().records[0])
    );

    let second_marking_proof =
        P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
            &conservative_scan_append_receipt,
            &mut heap,
        )
        .expect("second heap-produced conservative-root marking proof");
    let second_collector_effects_proof =
        P6Arm64SlotVisitorCollectorEffectsProof::from_conservative_root_marking_proof(
            &second_marking_proof,
            &mut heap,
        )
        .expect("second heap-produced collector-effects proof");
    let second_effects_plan = second_collector_effects_proof.collector_effects_plan();
    assert_eq!(second_effects_plan.mark_stack_append_count, 0);
    assert_eq!(second_effects_plan.already_marked_count, 1);
    assert_eq!(
        second_effects_plan.records[0].action,
        SlotVisitorCollectorEffectAction::AlreadyMarkedReturn
    );
}
