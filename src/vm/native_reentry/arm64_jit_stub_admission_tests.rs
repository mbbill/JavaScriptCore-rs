use super::super::rooting::{
    P6Arm64BranchAwareCallableFallbackRootingProof, P6Arm64JitStubRoutineConservativeScanHookProof,
    P6Arm64JitStubRoutineTraceProof,
};
use super::tests;
use super::*;
use crate::gc::RootMarkReason;
use crate::jit::{
    ExecutableAllocationId, JitStubRoutineCandidateAddress, JitStubRoutineTraceError,
};

fn branch_request<'a, 'publication>(
    side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
) -> P6Arm64BranchAwareCallableAdmissionProofRequest<'a, 'publication> {
    tests::valid_request(side_exits)
}

#[test]
fn public_arm64_jit_stub_stage_rejects_missing_trace_proof() {
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
fn public_arm64_jit_stub_stage_accepts_hook_trace_proof_and_rejects_missing_native_frame_residency()
{
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
        let jit_stub_trace_plan =
            tests::jit_stub_trace_proof(&collector_effects_plan, &verifier_append_proof);
        let mut request = branch_request(&side_exits);
        request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierAppendAndJitStubTracePlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
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
                    conservative_scan_append_receipt,
                    vm_root_gather_plan,
                    conservative_root_marking_plan,
                    collector_effects_plan,
                    verifier_append_proof,
                    jit_stub_trace_plan,
                }
            )
        );
    });
}

#[test]
fn arm64_jit_stub_scan_hook_proof_replays_candidate_marks_after_prepare() {
    let proof = tests::jit_stub_scan_hook_proof();
    let scan_plan = proof.scan_plan();

    assert_eq!(scan_plan.immutable_routine_count, 1);
    assert_eq!(scan_plan.mark_records.len(), 1);
    assert_eq!(
        scan_plan.mark_records[0].candidate,
        tests::jit_stub_candidate_for_first_mutable_routine()
    );
    assert!(!scan_plan.mark_records[0].was_may_be_executing);
    assert!(scan_plan.mark_records[0].may_be_executing_after);
    assert!(scan_plan.mutable_routines[0].may_be_executing);
    assert!(!scan_plan.mutable_routines[1].may_be_executing);
}

#[test]
fn arm64_jit_stub_scan_hook_proof_rejects_candidates_outside_prepared_range() {
    let set = tests::jit_stub_set_descriptor();
    let candidate = JitStubRoutineCandidateAddress {
        allocation: ExecutableAllocationId(17),
        offset: 999,
    };

    let error =
        P6Arm64JitStubRoutineConservativeScanHookProof::from_prepared_set_and_conservative_scan_hook_candidates(
            &set,
            [candidate],
        )
        .expect_err("candidate outside prepared JIT-stub range");

    assert!(matches!(
        error,
        JitStubRoutineTraceError::CandidateOutsidePreparedRange {
            candidate: actual,
            prepared_range: Some(_),
        } if actual == candidate
    ));
}

#[test]
fn arm64_jit_stub_trace_proof_uses_collector_heap_epoch_worklist_and_jit_root_reason() {
    let (_, _, collector_effects_proof) =
        tests::conservative_root_marking_and_collector_effects_proof();
    let verifier_append_proof = tests::verifier_append_proof(&collector_effects_proof);
    let scan_hook_proof = tests::jit_stub_scan_hook_proof();

    let trace_proof =
        P6Arm64JitStubRoutineTraceProof::from_collector_effects_verifier_and_scan_hook_proofs(
            &collector_effects_proof,
            &verifier_append_proof,
            &scan_hook_proof,
        )
        .expect("JIT stub trace proof");

    let collector_effects_plan = collector_effects_proof.collector_effects_plan();
    let trace_plan = trace_proof.trace_plan();
    assert_eq!(trace_plan.heap, collector_effects_plan.heap);
    assert_eq!(
        trace_plan.marking_epoch,
        collector_effects_plan.marking_epoch
    );
    assert_eq!(trace_plan.worklist, collector_effects_plan.worklist);
    assert_eq!(trace_plan.root_mark_reason, RootMarkReason::JitStubRoutines);
    assert_eq!(trace_plan.prepared_scan, *scan_hook_proof.scan_plan());
}
