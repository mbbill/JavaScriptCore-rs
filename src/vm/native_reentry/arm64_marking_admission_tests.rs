use super::super::rooting::{
    P6Arm64BranchAwareCallableFallbackRootingProof, P6Arm64ConservativeRootMarkingProofMismatch,
    P6Arm64SlotVisitorConservativeRootMarkingProof, P6Arm64VmRootGatherProofMismatch,
};
use super::tests;
use super::*;
use crate::gc::{
    CellState, ConservativeRootCell, ConservativeRoots, MarkWorklistId,
    SlotVisitorConservativeRootMarkingAction,
};
use crate::runtime::CellId;

fn branch_request<'a, 'publication>(
    side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
) -> P6Arm64BranchAwareCallableAdmissionProofRequest<'a, 'publication> {
    tests::valid_request(side_exits)
}

#[test]
fn public_arm64_marking_stage_rejects_vm_roots_without_marking_proof() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let (_heap, conservative_scan_append_receipt) =
            tests::heap_with_conservative_scan_append_receipt();
        let machine_stack_conservative_rooting_proof =
            tests::machine_stack_conservative_rooting_proof_from_marker(
                top_call_frame_publication,
                &conservative_scan_append_receipt,
            );
        let vm_root_gather_plan = tests::vm_root_gather_proof(&conservative_scan_append_receipt);
        let mut request = branch_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
                vm_root_gather_plan: vm_root_gather_plan.clone(),
            };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingRealSlotVisitorConservativeRootMarkingProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                }
            )
        );
    });
}

#[test]
fn public_arm64_marking_stage_progresses_with_heap_produced_marking_proof() {
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
fn public_arm64_marking_stage_rejects_marking_proof_from_different_receipt() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let (mut heap, original_receipt) = tests::heap_with_conservative_scan_append_receipt();
        let conservative_root_marking_plan =
            P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
                &original_receipt,
                &mut heap,
            )
            .expect("heap-produced conservative-root marking proof");

        let mut different_receipt = original_receipt.clone();
        let original_append_record = original_receipt.append_plan.records[0];
        let different_root = ConservativeRootCell {
            candidate_address: original_append_record.root.candidate_address + 0x1000,
            cell: CellId(original_append_record.root.cell.0 + 1),
        };
        different_receipt.append_plan.records[0].root = different_root;
        different_receipt.append_plan.records[0].cell = different_root.cell;

        let machine_stack_conservative_rooting_proof =
            tests::machine_stack_conservative_rooting_proof_from_marker(
                top_call_frame_publication,
                &different_receipt,
            );
        let vm_root_gather_plan = tests::vm_root_gather_proof(&different_receipt);
        let receipt_append_record = different_receipt.append_plan.records[0];
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
                P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
                    top_call_frame_publication,
                    conservative_scan_append_receipt: different_receipt,
                    vm_root_gather_plan,
                    conservative_root_marking_plan,
                    mismatch:
                        P6Arm64ConservativeRootMarkingProofMismatch::AppendRecordMismatch {
                            order: 0,
                            receipt: receipt_append_record,
                            marking: original_append_record,
                        },
                }
            )
        );
    });
}

#[test]
fn arm64_marking_proof_constructor_reflects_already_marked_heap_state() {
    let (mut heap, conservative_scan_append_receipt) =
        tests::heap_with_conservative_scan_append_receipt();
    let first =
        P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
            &conservative_scan_append_receipt,
            &mut heap,
        )
        .expect("first conservative-root marking proof");
    assert_eq!(first.marking_plan().queued_js_cell_count, 1);
    assert_eq!(first.marking_plan().already_marked_count, 0);
    assert_eq!(
        first.marking_plan().records[0].action,
        SlotVisitorConservativeRootMarkingAction::QueueJsCell {
            cell_state: CellState::PossiblyGrey,
            worklist: MarkWorklistId::default(),
        }
    );

    let second =
        P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
            &conservative_scan_append_receipt,
            &mut heap,
        )
        .expect("second conservative-root marking proof");
    assert_eq!(second.marking_plan().queued_js_cell_count, 0);
    assert_eq!(second.marking_plan().already_marked_count, 1);
    assert_eq!(
        second.marking_plan().records[0].action,
        SlotVisitorConservativeRootMarkingAction::AlreadyMarked
    );
}

#[test]
fn public_arm64_marking_stage_preserves_vm_root_receipt_coherence_check() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let (_heap, conservative_scan_append_receipt) =
            tests::heap_with_conservative_scan_append_receipt();
        let machine_stack_conservative_rooting_proof =
            tests::machine_stack_conservative_rooting_proof_from_marker(
                top_call_frame_publication,
                &conservative_scan_append_receipt,
            );
        let mut vm_root_gather_plan =
            tests::vm_root_gather_proof(&conservative_scan_append_receipt);
        let receipt_root = conservative_scan_append_receipt.append_plan.records[0].root;
        let missing_root = ConservativeRootCell {
            candidate_address: receipt_root.candidate_address + 0x2000,
            cell: CellId(receipt_root.cell.0 + 2),
        };
        vm_root_gather_plan.scratch_buffer_records[0].candidates[0].candidate_address =
            missing_root.candidate_address;
        let mut vm_roots = ConservativeRoots::new();
        for span in vm_root_gather_plan.conservative_roots.spans() {
            vm_roots.add_span(*span);
        }
        vm_roots.add_candidate_address(missing_root.candidate_address);
        vm_roots.add_validated_cell(missing_root);
        vm_root_gather_plan.conservative_roots = vm_roots;
        let mut request = branch_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
                vm_root_gather_plan: vm_root_gather_plan.clone(),
            };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication,
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                    mismatch: P6Arm64VmRootGatherProofMismatch::ReceiptMissingVmRoot {
                        root: missing_root,
                    },
                }
            )
        );
    });
}
