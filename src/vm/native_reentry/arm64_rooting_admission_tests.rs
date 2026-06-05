use super::super::rooting::{
    P6Arm64BranchAwareCallableFallbackRootingProof, P6Arm64MachineStackConservativeRootingProof,
    P6Arm64MachineStackConservativeRootingProofMismatch, P6Arm64NativeFrameMachineStackSpanKind,
};
use super::tests;
use super::*;
use crate::gc::{ConservativeRootCell, ConservativeRootSpan, ConservativeRoots, HeapEpoch, HeapId};

fn assert_machine_stack_rooting_mismatch(
    top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
    mismatch: P6Arm64MachineStackConservativeRootingProofMismatch,
) {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    let mut request = tests::valid_request(&side_exits);
    request.fallback_rooting_proof =
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithMachineStackConservativeRootingProof {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof:
                machine_stack_conservative_rooting_proof.clone(),
        };

    assert_eq!(
        p6_arm64_public_branch_aware_callable_admission_proof(&request),
        Err(
            P6Arm64BranchAwareCallableAdmissionRejection::MachineStackAndConservativeRootingProofMismatch {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
                mismatch,
            }
        )
    );
}

fn rebuild_machine_stack_roots(
    proof: &P6Arm64MachineStackConservativeRootingProof,
    root: ConservativeRootCell,
) -> ConservativeRoots {
    let mut roots = ConservativeRoots::new();
    for span in &proof.machine_stack_spans {
        roots.add_span(span.span);
    }
    roots.add_validated_cell(root);
    roots
}

#[test]
fn public_arm64_rooting_stage_rejects_stack_publication_without_machine_stack_scan() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let mut request = tests::valid_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithoutConservativeScanAppend(
                top_call_frame_publication,
            );

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingMachineStackAndConservativeRootingProof {
                    top_call_frame_publication,
                }
            )
        );
    });
}

#[test]
fn public_arm64_rooting_stage_progresses_with_same_scope_machine_stack_scan() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut request = tests::valid_request(&side_exits);
        request.fallback_rooting_proof =
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithMachineStackConservativeRootingProof {
            top_call_frame_publication: fixture.top_call_frame_publication,
            machine_stack_conservative_rooting_proof: fixture
                .machine_stack_conservative_rooting_proof
                .clone(),
        };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingVmRootGatherProof {
                    top_call_frame_publication: fixture.top_call_frame_publication,
                    conservative_scan_append_receipt: fixture.conservative_scan_append_receipt,
                }
            )
        );
    });
}

#[test]
fn public_arm64_rooting_stage_accepts_vm_roots_after_machine_stack_roots() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut machine_stack_conservative_rooting_proof =
            fixture.machine_stack_conservative_rooting_proof.clone();
        let machine_stack_root = machine_stack_conservative_rooting_proof
            .machine_stack_roots
            .validated_cells()[0];
        let mut receipt = machine_stack_conservative_rooting_proof
            .conservative_scan_append_receipt
            .clone();
        let mut vm_root_record = receipt.append_plan.records[0];
        vm_root_record.order = receipt.append_plan.records.len();
        vm_root_record.root = ConservativeRootCell {
            candidate_address: machine_stack_root.candidate_address + 0x1000,
            cell: crate::runtime::CellId(machine_stack_root.cell.0 + 1),
        };
        vm_root_record.cell = vm_root_record.root.cell;
        receipt.append_plan.records.push(vm_root_record);
        receipt.appended_record_count = receipt.append_plan.records.len();
        receipt.conservative_root_count = receipt.appended_record_count;
        machine_stack_conservative_rooting_proof.conservative_scan_append_receipt = receipt.clone();

        let mut request = tests::valid_request(&side_exits);
        request.fallback_rooting_proof =
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithMachineStackConservativeRootingProof {
            top_call_frame_publication: fixture.top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
        };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingVmRootGatherProof {
                    top_call_frame_publication: fixture.top_call_frame_publication,
                    conservative_scan_append_receipt: receipt,
                }
            )
        );
    });
}

#[test]
fn public_arm64_rooting_stage_rejects_receipt_not_derived_from_machine_stack_roots() {
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut proof = fixture.machine_stack_conservative_rooting_proof.clone();
        let machine_stack_root = proof.machine_stack_roots.validated_cells()[0];
        let receipt_root = ConservativeRootCell {
            candidate_address: machine_stack_root.candidate_address + 0x1000,
            cell: crate::runtime::CellId(machine_stack_root.cell.0 + 1),
        };
        proof.conservative_scan_append_receipt.append_plan.records[0].root = receipt_root;

        assert_machine_stack_rooting_mismatch(
            fixture.top_call_frame_publication,
            proof,
            P6Arm64MachineStackConservativeRootingProofMismatch::ConservativeScanAppendRootMismatch {
                order: 0,
                machine_stack: machine_stack_root,
                receipt: receipt_root,
            },
        );
    });
}

#[test]
fn public_arm64_rooting_stage_rejects_mismatched_machine_stack_evidence() {
    tests::with_native_frame_residency_fixture(|fixture| {
        let mut proof = fixture.machine_stack_conservative_rooting_proof.clone();
        proof.heap = HeapId(proof.conservative_scan_append_receipt.heap.0 + 1);
        assert_machine_stack_rooting_mismatch(
            fixture.top_call_frame_publication,
            proof.clone(),
            P6Arm64MachineStackConservativeRootingProofMismatch::HeapMismatch {
                receipt: proof.conservative_scan_append_receipt.heap,
                machine_stack: proof.heap,
            },
        );
    });

    tests::with_native_frame_residency_fixture(|fixture| {
        let mut proof = fixture.machine_stack_conservative_rooting_proof.clone();
        proof.marking_epoch = HeapEpoch(proof.conservative_scan_append_receipt.epoch.0 + 1);
        assert_machine_stack_rooting_mismatch(
            fixture.top_call_frame_publication,
            proof.clone(),
            P6Arm64MachineStackConservativeRootingProofMismatch::MarkingEpochMismatch {
                receipt: proof.conservative_scan_append_receipt.epoch,
                machine_stack: proof.marking_epoch,
            },
        );
    });

    tests::with_native_frame_residency_fixture(|fixture| {
        let mut proof = fixture.machine_stack_conservative_rooting_proof.clone();
        let root = proof.machine_stack_roots.validated_cells()[0];
        proof.machine_stack_spans.swap(0, 1);
        proof.machine_stack_roots = rebuild_machine_stack_roots(&proof, root);
        assert_machine_stack_rooting_mismatch(
            fixture.top_call_frame_publication,
            proof,
            P6Arm64MachineStackConservativeRootingProofMismatch::CurrentThreadSpanOrderMismatch {
                observed: vec![
                    P6Arm64NativeFrameMachineStackSpanKind::Stack,
                    P6Arm64NativeFrameMachineStackSpanKind::RegisterState,
                ],
            },
        );
    });

    tests::with_native_frame_residency_fixture(|fixture| {
        let mut proof = fixture.machine_stack_conservative_rooting_proof.clone();
        let root = proof.machine_stack_roots.validated_cells()[0];
        proof.machine_stack_spans[1].span = ConservativeRootSpan {
            begin: 0x2000,
            end: 0x2100,
        };
        proof.machine_stack_roots = rebuild_machine_stack_roots(&proof, root);
        assert_machine_stack_rooting_mismatch(
            fixture.top_call_frame_publication,
            proof,
            P6Arm64MachineStackConservativeRootingProofMismatch::TopCallFrameOutsideScannedSpans {
                address: fixture
                    .top_call_frame_publication
                    .publication
                    .published_top_frame,
            },
        );
    });

    tests::with_native_frame_residency_fixture(|fixture| {
        let mut proof = fixture.machine_stack_conservative_rooting_proof.clone();
        let root = proof.machine_stack_roots.validated_cells()[0];
        let fake_root = ConservativeRootCell {
            candidate_address: root.candidate_address + 0x2000,
            cell: crate::runtime::CellId(root.cell.0 + 2),
        };
        proof.machine_stack_roots = rebuild_machine_stack_roots(&proof, fake_root);
        assert_machine_stack_rooting_mismatch(
            fixture.top_call_frame_publication,
            proof,
            P6Arm64MachineStackConservativeRootingProofMismatch::ConservativeScanAppendRootMismatch {
                order: 0,
                machine_stack: fake_root,
                receipt: root,
            },
        );
    });
}

#[test]
fn public_arm64_branch_aware_admission_progresses_through_rooting_stages() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    tests::with_padded_stack_top_call_frame_publication(|top_call_frame_publication| {
        let mut request = tests::valid_request(&side_exits);

        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithoutConservativeScanAppend(
                top_call_frame_publication,
            );
        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingMachineStackAndConservativeRootingProof {
                    top_call_frame_publication,
                }
            )
        );

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
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithMachineStackConservativeRootingProof {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
            };
        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingVmRootGatherProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                }
            )
        );

        let vm_root_gather_plan = tests::vm_root_gather_proof(&conservative_scan_append_receipt);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
                vm_root_gather_plan: vm_root_gather_plan.clone(),
            };
        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingRealSlotVisitorConservativeRootMarkingProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                }
            )
        );

        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherAndConservativeRootMarkingPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
                vm_root_gather_plan: vm_root_gather_plan.clone(),
                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
            };
        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingRealCollectorMarkStackCellStateAndContainerProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                }
            )
        );

        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherAndCollectorEffectsPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
                vm_root_gather_plan: vm_root_gather_plan.clone(),
                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                collector_effects_plan: collector_effects_plan.clone(),
            };
        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingVerifierSlotVisitorAppendOrAbsenceProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                    collector_effects_plan: collector_effects_plan.clone(),
                }
            )
        );

        let verifier_append_proof = tests::verifier_append_proof(&collector_effects_plan);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsAndVerifierAppendProof {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
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
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                    collector_effects_plan: collector_effects_plan.clone(),
                    verifier_append_proof: verifier_append_proof.clone(),
                }
            )
        );

        let jit_stub_trace_plan =
            tests::jit_stub_trace_proof(&collector_effects_plan, &verifier_append_proof);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierAppendAndJitStubTracePlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
                vm_root_gather_plan: vm_root_gather_plan.clone(),
                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                collector_effects_plan: collector_effects_plan.clone(),
                verifier_append_proof: verifier_append_proof.clone(),
                jit_stub_trace_plan: jit_stub_trace_plan.clone(),
            };
        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeFrameMachineStackResidencyProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                    collector_effects_plan: collector_effects_plan.clone(),
                    verifier_append_proof: verifier_append_proof.clone(),
                    jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                }
            )
        );

        let native_frame_residency_proof =
            tests::verified_native_frame_machine_stack_residency_proof_from_rooting(
                top_call_frame_publication,
                &machine_stack_conservative_rooting_proof,
                &conservative_scan_append_receipt,
            )
            .expect("valid wrapper should verify native frame machine-stack residency");
        request.fallback_rooting_proof = tests::full_machine_stack_residency_fallback(
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan.clone(),
            conservative_root_marking_plan.clone(),
            collector_effects_plan.clone(),
            verifier_append_proof.clone(),
            jit_stub_trace_plan.clone(),
            native_frame_residency_proof.clone(),
        );
        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64GeneratedNativeFrameMaterializationProof {
                    top_call_frame_publication,
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                    conservative_root_marking_plan,
                    collector_effects_plan,
                    verifier_append_proof,
                    jit_stub_trace_plan,
                    native_frame_residency_proof,
                }
            )
        );
    });
}
