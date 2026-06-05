//! ARM64 public-admission prior-stage validation helpers.
//!
//! C++ JSC carries this sequence as live VM-entry state across
//! `LowLevelInterpreter64.asm` VM entry top-frame publication,
//! `SlotVisitor::append(ConservativeRoots)`, verifier root append, JIT stub
//! tracing, and `JIT.cpp` ARM64 frame-prologue materialization. Rust keeps it
//! as a borrowed proof-prefix module to shrink `arm64_admission.rs` while
//! preserving the same public rejection behavior.

use crate::gc::HeapConservativeScanAppendReceipt;

use super::super::super::vm_roots::VmRootGatherPlan;
use super::super::arm64_exception_unwind::{
    validate_p6_arm64_verified_vm_entry_exception_unwind_restoration_proof,
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof,
};
use super::super::arm64_vm_entry_normal_return::{
    validate_p6_arm64_verified_vm_entry_normal_return_restoration_proof,
    P6Arm64VerifiedVmEntryNormalReturnRestorationProof,
};
use super::super::rooting::{
    validate_p6_arm64_collector_effects_plan, validate_p6_arm64_conservative_root_marking_plan,
    validate_p6_arm64_generated_native_frame_materialization_proof,
    validate_p6_arm64_jit_stub_routine_trace_plan,
    validate_p6_arm64_machine_stack_conservative_rooting_proof,
    validate_p6_arm64_verified_jsc_stack_dispatch_request_proof,
    validate_p6_arm64_verified_native_frame_machine_stack_residency_proof,
    validate_p6_arm64_verifier_append_proof, validate_p6_arm64_vm_root_gather_plan,
    P6Arm64BranchAwareCallableTopCallFramePublicationProof, P6Arm64CollectorEffectsProofMismatch,
    P6Arm64ConservativeRootMarkingProofMismatch, P6Arm64JitStubRoutineTraceProof,
    P6Arm64JitStubRoutineTraceProofMismatch, P6Arm64MachineStackConservativeRootingProof,
    P6Arm64NativeFrameMachineStackResidencyProofMismatch, P6Arm64SlotVisitorCollectorEffectsProof,
    P6Arm64SlotVisitorConservativeRootMarkingProof,
    P6Arm64VerifiedGeneratedNativeFrameMaterializationProof,
    P6Arm64VerifiedJscStackDispatchRequestProof,
    P6Arm64VerifiedNativeFrameMachineStackResidencyProof, P6Arm64VerifierAppendProofMismatch,
    P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
};
use super::P6Arm64BranchAwareCallableAdmissionRejection;

pub(super) fn p6_arm64_conservative_scan_append_receipt_or_reject<'proof, 'publication>(
    top_call_frame_publication: &'proof P6Arm64BranchAwareCallableTopCallFramePublicationProof<
        'publication,
    >,
    machine_stack_conservative_rooting_proof: &'proof P6Arm64MachineStackConservativeRootingProof,
) -> Result<
    &'proof HeapConservativeScanAppendReceipt,
    P6Arm64BranchAwareCallableAdmissionRejection<'publication>,
> {
    validate_p6_arm64_machine_stack_conservative_rooting_proof(
        top_call_frame_publication,
        machine_stack_conservative_rooting_proof,
    )
    .map(|_| machine_stack_conservative_rooting_proof.conservative_scan_append_receipt())
    .map_err(|mismatch| {
        P6Arm64BranchAwareCallableAdmissionRejection::MachineStackAndConservativeRootingProofMismatch {
            top_call_frame_publication: *top_call_frame_publication,
            machine_stack_conservative_rooting_proof:
                machine_stack_conservative_rooting_proof.clone(),
            mismatch,
        }
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) struct P6Arm64VmRootGatherAdmissionContext<'proof, 'publication> {
    top_call_frame_publication:
        &'proof P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
    conservative_scan_append_receipt: &'proof HeapConservativeScanAppendReceipt,
    vm_root_gather_plan: &'proof VmRootGatherPlan,
}

impl<'publication> P6Arm64VmRootGatherAdmissionContext<'_, 'publication> {
    pub(super) fn missing_conservative_root_marking(
        self,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        P6Arm64BranchAwareCallableAdmissionRejection::MissingRealSlotVisitorConservativeRootMarkingProof {
            top_call_frame_publication: *self.top_call_frame_publication,
            conservative_scan_append_receipt: self.conservative_scan_append_receipt.clone(),
            vm_root_gather_plan: self.vm_root_gather_plan.clone(),
        }
    }

    fn conservative_root_marking_mismatch(
        self,
        conservative_root_marking_plan: &P6Arm64SlotVisitorConservativeRootMarkingProof,
        mismatch: P6Arm64ConservativeRootMarkingProofMismatch,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
            top_call_frame_publication: *self.top_call_frame_publication,
            conservative_scan_append_receipt: self.conservative_scan_append_receipt.clone(),
            vm_root_gather_plan: self.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
            mismatch,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct P6Arm64ConservativeRootMarkingAdmissionContext<'proof, 'publication> {
    vm_root_gather: P6Arm64VmRootGatherAdmissionContext<'proof, 'publication>,
    conservative_root_marking_plan: &'proof P6Arm64SlotVisitorConservativeRootMarkingProof,
}

impl<'publication> P6Arm64ConservativeRootMarkingAdmissionContext<'_, 'publication> {
    pub(super) fn missing_collector_effects(
        self,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let vm_root_gather = self.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingRealCollectorMarkStackCellStateAndContainerProof {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather.conservative_scan_append_receipt.clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: self.conservative_root_marking_plan.clone(),
        }
    }

    fn collector_effects_mismatch(
        self,
        collector_effects_plan: &P6Arm64SlotVisitorCollectorEffectsProof,
        mismatch: P6Arm64CollectorEffectsProofMismatch,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let vm_root_gather = self.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::CollectorEffectsProofMismatch {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: self.conservative_root_marking_plan.clone(),
            collector_effects_plan: collector_effects_plan.clone(),
            mismatch,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct P6Arm64CollectorEffectsAdmissionContext<'proof, 'publication> {
    conservative_root_marking: P6Arm64ConservativeRootMarkingAdmissionContext<'proof, 'publication>,
    collector_effects_plan: &'proof P6Arm64SlotVisitorCollectorEffectsProof,
}

impl<'publication> P6Arm64CollectorEffectsAdmissionContext<'_, 'publication> {
    pub(super) fn missing_verifier_append(
        self,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let conservative_root_marking = self.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingVerifierSlotVisitorAppendOrAbsenceProof {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather.conservative_scan_append_receipt.clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: self.collector_effects_plan.clone(),
        }
    }

    fn verifier_append_mismatch(
        self,
        verifier_append_proof: &P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        mismatch: P6Arm64VerifierAppendProofMismatch,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let conservative_root_marking = self.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::VerifierAppendProofMismatch {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: self.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append_proof.clone(),
            mismatch,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct P6Arm64VerifierAppendAdmissionContext<'proof, 'publication> {
    collector_effects: P6Arm64CollectorEffectsAdmissionContext<'proof, 'publication>,
    verifier_append_proof: &'proof P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
}

impl<'publication> P6Arm64VerifierAppendAdmissionContext<'_, 'publication> {
    pub(super) fn missing_jit_stub_trace(
        self,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let collector_effects = self.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingJitStubRoutineTraceProof {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: self.verifier_append_proof.clone(),
        }
    }

    fn jit_stub_trace_mismatch(
        self,
        jit_stub_trace_plan: &P6Arm64JitStubRoutineTraceProof,
        mismatch: P6Arm64JitStubRoutineTraceProofMismatch,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let collector_effects = self.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::JitStubRoutineTraceProofMismatch {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: self.verifier_append_proof.clone(),
            jit_stub_trace_plan: jit_stub_trace_plan.clone(),
            mismatch,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct P6Arm64JitStubTraceAdmissionContext<'proof, 'publication> {
    machine_stack_conservative_rooting_proof: &'proof P6Arm64MachineStackConservativeRootingProof,
    verifier_append: P6Arm64VerifierAppendAdmissionContext<'proof, 'publication>,
    jit_stub_trace_plan: &'proof P6Arm64JitStubRoutineTraceProof,
}

impl<'publication> P6Arm64JitStubTraceAdmissionContext<'_, 'publication> {
    pub(super) fn missing_native_frame_residency(
        self,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let verifier_append = self.verifier_append;
        let collector_effects = verifier_append.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeFrameMachineStackResidencyProof {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append.verifier_append_proof.clone(),
            jit_stub_trace_plan: self.jit_stub_trace_plan.clone(),
        }
    }

    pub(super) fn missing_generated_native_frame_materialization(
        self,
        native_frame_residency_proof: &P6Arm64VerifiedNativeFrameMachineStackResidencyProof<
            'publication,
        >,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let verifier_append = self.verifier_append;
        let collector_effects = verifier_append.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64GeneratedNativeFrameMaterializationProof {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather.conservative_scan_append_receipt.clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append.verifier_append_proof.clone(),
            jit_stub_trace_plan: self.jit_stub_trace_plan.clone(),
            native_frame_residency_proof: native_frame_residency_proof.clone(),
        }
    }

    fn native_frame_residency_mismatch(
        self,
        native_frame_residency_proof: &P6Arm64VerifiedNativeFrameMachineStackResidencyProof<
            'publication,
        >,
        mismatch: P6Arm64NativeFrameMachineStackResidencyProofMismatch,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let verifier_append = self.verifier_append;
        let collector_effects = verifier_append.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::NativeFrameMachineStackResidencyProofMismatch {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather.conservative_scan_append_receipt.clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append.verifier_append_proof.clone(),
            jit_stub_trace_plan: self.jit_stub_trace_plan.clone(),
            native_frame_residency_proof: native_frame_residency_proof.clone(),
            mismatch,
        }
    }

    pub(super) fn missing_jsc_stack_dispatch(
        self,
        generated_native_frame_materialization_proof:
            &P6Arm64VerifiedGeneratedNativeFrameMaterializationProof<'publication>,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let verifier_append = self.verifier_append;
        let collector_effects = verifier_append.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64JscStackDispatchAdmissionAuthority {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather.conservative_scan_append_receipt.clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append.verifier_append_proof.clone(),
            jit_stub_trace_plan: self.jit_stub_trace_plan.clone(),
            generated_native_frame_materialization_proof:
                generated_native_frame_materialization_proof.clone(),
        }
    }

    pub(super) fn missing_vm_entry_exit_restoration(
        self,
        jsc_stack_dispatch_request_proof: &P6Arm64VerifiedJscStackDispatchRequestProof<
            'publication,
        >,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let verifier_append = self.verifier_append;
        let collector_effects = verifier_append.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64VmEntryExitRestorationAuthority {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append.verifier_append_proof.clone(),
            jit_stub_trace_plan: self.jit_stub_trace_plan.clone(),
            jsc_stack_dispatch_request_proof: jsc_stack_dispatch_request_proof.clone(),
        }
    }

    pub(super) fn missing_vm_entry_exception_unwind_restoration(
        self,
        vm_entry_normal_return_restoration_proof:
            &P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'publication>,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let verifier_append = self.verifier_append;
        let collector_effects = verifier_append.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64VmEntryExceptionUnwindRestorationAuthority {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append.verifier_append_proof.clone(),
            jit_stub_trace_plan: self.jit_stub_trace_plan.clone(),
            vm_entry_normal_return_restoration_proof:
                vm_entry_normal_return_restoration_proof.clone(),
        }
    }

    pub(super) fn missing_public_jsc_stack_dispatch_execution(
        self,
        vm_entry_exception_unwind_restoration_proof:
            &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication>,
    ) -> P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
        let verifier_append = self.verifier_append;
        let collector_effects = verifier_append.collector_effects;
        let conservative_root_marking = collector_effects.conservative_root_marking;
        let vm_root_gather = conservative_root_marking.vm_root_gather;
        P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64PublicJscStackDispatchExecutionAuthority {
            top_call_frame_publication: *vm_root_gather.top_call_frame_publication,
            conservative_scan_append_receipt: vm_root_gather
                .conservative_scan_append_receipt
                .clone(),
            vm_root_gather_plan: vm_root_gather.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: conservative_root_marking
                .conservative_root_marking_plan
                .clone(),
            collector_effects_plan: collector_effects.collector_effects_plan.clone(),
            verifier_append_proof: verifier_append.verifier_append_proof.clone(),
            jit_stub_trace_plan: self.jit_stub_trace_plan.clone(),
            vm_entry_exception_unwind_restoration_proof:
                vm_entry_exception_unwind_restoration_proof.clone(),
        }
    }
}

// C++ JSC carries this sequence as live VM-entry state:
// `LowLevelInterpreter64.asm` publishes the top CallFrame, the GC appends
// ConservativeRoots to SlotVisitor and VerifierSlotVisitor, JIT stub routines
// are traced, and `JIT.cpp` materializes the ARM64 frame prologue. Rust keeps
// the sequence as borrowed proof contexts so the admission arms share the same
// validation order without adding a Rust-only admission stage.
pub(super) fn p6_arm64_vm_root_gather_context_or_reject<'proof, 'publication>(
    top_call_frame_publication: &'proof P6Arm64BranchAwareCallableTopCallFramePublicationProof<
        'publication,
    >,
    machine_stack_conservative_rooting_proof: &'proof P6Arm64MachineStackConservativeRootingProof,
    vm_root_gather_plan: &'proof VmRootGatherPlan,
) -> Result<
    P6Arm64VmRootGatherAdmissionContext<'proof, 'publication>,
    P6Arm64BranchAwareCallableAdmissionRejection<'publication>,
> {
    let conservative_scan_append_receipt = p6_arm64_conservative_scan_append_receipt_or_reject(
        top_call_frame_publication,
        machine_stack_conservative_rooting_proof,
    )?;
    validate_p6_arm64_vm_root_gather_plan(conservative_scan_append_receipt, vm_root_gather_plan)
        .map_err(|mismatch| {
            P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                top_call_frame_publication: *top_call_frame_publication,
                conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                vm_root_gather_plan: vm_root_gather_plan.clone(),
                mismatch,
            }
        })?;
    Ok(P6Arm64VmRootGatherAdmissionContext {
        top_call_frame_publication,
        conservative_scan_append_receipt,
        vm_root_gather_plan,
    })
}

pub(super) fn p6_arm64_conservative_root_marking_context_or_reject<'proof, 'publication>(
    top_call_frame_publication: &'proof P6Arm64BranchAwareCallableTopCallFramePublicationProof<
        'publication,
    >,
    machine_stack_conservative_rooting_proof: &'proof P6Arm64MachineStackConservativeRootingProof,
    vm_root_gather_plan: &'proof VmRootGatherPlan,
    conservative_root_marking_plan: &'proof P6Arm64SlotVisitorConservativeRootMarkingProof,
) -> Result<
    P6Arm64ConservativeRootMarkingAdmissionContext<'proof, 'publication>,
    P6Arm64BranchAwareCallableAdmissionRejection<'publication>,
> {
    let vm_root_gather = p6_arm64_vm_root_gather_context_or_reject(
        top_call_frame_publication,
        machine_stack_conservative_rooting_proof,
        vm_root_gather_plan,
    )?;
    validate_p6_arm64_conservative_root_marking_plan(
        vm_root_gather.conservative_scan_append_receipt,
        conservative_root_marking_plan,
    )
    .map_err(|mismatch| {
        vm_root_gather.conservative_root_marking_mismatch(conservative_root_marking_plan, mismatch)
    })?;
    Ok(P6Arm64ConservativeRootMarkingAdmissionContext {
        vm_root_gather,
        conservative_root_marking_plan,
    })
}

pub(super) fn p6_arm64_collector_effects_context_or_reject<'proof, 'publication>(
    top_call_frame_publication: &'proof P6Arm64BranchAwareCallableTopCallFramePublicationProof<
        'publication,
    >,
    machine_stack_conservative_rooting_proof: &'proof P6Arm64MachineStackConservativeRootingProof,
    vm_root_gather_plan: &'proof VmRootGatherPlan,
    conservative_root_marking_plan: &'proof P6Arm64SlotVisitorConservativeRootMarkingProof,
    collector_effects_plan: &'proof P6Arm64SlotVisitorCollectorEffectsProof,
) -> Result<
    P6Arm64CollectorEffectsAdmissionContext<'proof, 'publication>,
    P6Arm64BranchAwareCallableAdmissionRejection<'publication>,
> {
    let conservative_root_marking = p6_arm64_conservative_root_marking_context_or_reject(
        top_call_frame_publication,
        machine_stack_conservative_rooting_proof,
        vm_root_gather_plan,
        conservative_root_marking_plan,
    )?;
    validate_p6_arm64_collector_effects_plan(
        conservative_root_marking.conservative_root_marking_plan,
        collector_effects_plan,
    )
    .map_err(|mismatch| {
        conservative_root_marking.collector_effects_mismatch(collector_effects_plan, mismatch)
    })?;
    Ok(P6Arm64CollectorEffectsAdmissionContext {
        conservative_root_marking,
        collector_effects_plan,
    })
}

pub(super) fn p6_arm64_verifier_append_context_or_reject<'proof, 'publication>(
    top_call_frame_publication: &'proof P6Arm64BranchAwareCallableTopCallFramePublicationProof<
        'publication,
    >,
    machine_stack_conservative_rooting_proof: &'proof P6Arm64MachineStackConservativeRootingProof,
    vm_root_gather_plan: &'proof VmRootGatherPlan,
    conservative_root_marking_plan: &'proof P6Arm64SlotVisitorConservativeRootMarkingProof,
    collector_effects_plan: &'proof P6Arm64SlotVisitorCollectorEffectsProof,
    verifier_append_proof: &'proof P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
) -> Result<
    P6Arm64VerifierAppendAdmissionContext<'proof, 'publication>,
    P6Arm64BranchAwareCallableAdmissionRejection<'publication>,
> {
    let collector_effects = p6_arm64_collector_effects_context_or_reject(
        top_call_frame_publication,
        machine_stack_conservative_rooting_proof,
        vm_root_gather_plan,
        conservative_root_marking_plan,
        collector_effects_plan,
    )?;
    let conservative_root_marking = collector_effects.conservative_root_marking;
    let vm_root_gather = conservative_root_marking.vm_root_gather;
    validate_p6_arm64_verifier_append_proof(
        vm_root_gather.conservative_scan_append_receipt,
        conservative_root_marking.conservative_root_marking_plan,
        vm_root_gather.vm_root_gather_plan,
        verifier_append_proof,
    )
    .map_err(|mismatch| {
        collector_effects.verifier_append_mismatch(verifier_append_proof, mismatch)
    })?;
    Ok(P6Arm64VerifierAppendAdmissionContext {
        collector_effects,
        verifier_append_proof,
    })
}

pub(super) fn p6_arm64_jit_stub_trace_context_or_reject<'proof, 'publication>(
    top_call_frame_publication: &'proof P6Arm64BranchAwareCallableTopCallFramePublicationProof<
        'publication,
    >,
    machine_stack_conservative_rooting_proof: &'proof P6Arm64MachineStackConservativeRootingProof,
    vm_root_gather_plan: &'proof VmRootGatherPlan,
    conservative_root_marking_plan: &'proof P6Arm64SlotVisitorConservativeRootMarkingProof,
    collector_effects_plan: &'proof P6Arm64SlotVisitorCollectorEffectsProof,
    verifier_append_proof: &'proof P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
    jit_stub_trace_plan: &'proof P6Arm64JitStubRoutineTraceProof,
) -> Result<
    P6Arm64JitStubTraceAdmissionContext<'proof, 'publication>,
    P6Arm64BranchAwareCallableAdmissionRejection<'publication>,
> {
    let verifier_append = p6_arm64_verifier_append_context_or_reject(
        top_call_frame_publication,
        machine_stack_conservative_rooting_proof,
        vm_root_gather_plan,
        conservative_root_marking_plan,
        collector_effects_plan,
        verifier_append_proof,
    )?;
    let collector_effects = verifier_append.collector_effects;
    validate_p6_arm64_jit_stub_routine_trace_plan(
        collector_effects.collector_effects_plan,
        jit_stub_trace_plan.trace_plan(),
    )
    .map_err(|mismatch| verifier_append.jit_stub_trace_mismatch(jit_stub_trace_plan, mismatch))?;
    Ok(P6Arm64JitStubTraceAdmissionContext {
        machine_stack_conservative_rooting_proof,
        verifier_append,
        jit_stub_trace_plan,
    })
}

pub(super) fn p6_arm64_validate_native_frame_residency_or_reject<'publication>(
    jit_stub_trace: P6Arm64JitStubTraceAdmissionContext<'_, 'publication>,
    native_frame_residency_proof: &P6Arm64VerifiedNativeFrameMachineStackResidencyProof<
        'publication,
    >,
) -> Result<(), P6Arm64BranchAwareCallableAdmissionRejection<'publication>> {
    let vm_root_gather = jit_stub_trace
        .verifier_append
        .collector_effects
        .conservative_root_marking
        .vm_root_gather;
    validate_p6_arm64_verified_native_frame_machine_stack_residency_proof(
        vm_root_gather.top_call_frame_publication,
        jit_stub_trace.machine_stack_conservative_rooting_proof,
        native_frame_residency_proof,
    )
    .map_err(|mismatch| {
        jit_stub_trace.native_frame_residency_mismatch(native_frame_residency_proof, mismatch)
    })
}

pub(super) fn p6_arm64_validate_generated_native_frame_materialization_or_reject<'publication>(
    jit_stub_trace: P6Arm64JitStubTraceAdmissionContext<'_, 'publication>,
    generated_native_frame_materialization_proof:
        &P6Arm64VerifiedGeneratedNativeFrameMaterializationProof<'publication>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64BranchAwareCallableAdmissionRejection<'publication>> {
    p6_arm64_validate_native_frame_residency_or_reject(
        jit_stub_trace,
        generated_native_frame_materialization_proof.native_frame_residency_proof(),
    )?;
    let vm_root_gather = jit_stub_trace
        .verifier_append
        .collector_effects
        .conservative_root_marking
        .vm_root_gather;
    validate_p6_arm64_generated_native_frame_materialization_proof(
        vm_root_gather.top_call_frame_publication,
        generated_native_frame_materialization_proof
            .native_frame_residency_proof()
            .residency_proof(),
        generated_native_frame_materialization_proof.materialization_descriptor(),
        expected_live_local_slots,
    )
    .map_err(|mismatch| {
        P6Arm64BranchAwareCallableAdmissionRejection::Arm64GeneratedNativeFrameMaterializationProofMismatch {
            mismatch,
        }
    })
}

pub(super) fn p6_arm64_validate_jsc_stack_dispatch_request_or_reject<'publication>(
    jit_stub_trace: P6Arm64JitStubTraceAdmissionContext<'_, 'publication>,
    jsc_stack_dispatch_request_proof: &P6Arm64VerifiedJscStackDispatchRequestProof<'publication>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64BranchAwareCallableAdmissionRejection<'publication>> {
    p6_arm64_validate_generated_native_frame_materialization_or_reject(
        jit_stub_trace,
        jsc_stack_dispatch_request_proof.generated_native_frame_materialization_proof(),
        expected_live_local_slots,
    )?;
    let vm_root_gather = jit_stub_trace
        .verifier_append
        .collector_effects
        .conservative_root_marking
        .vm_root_gather;
    validate_p6_arm64_verified_jsc_stack_dispatch_request_proof(
        vm_root_gather.top_call_frame_publication,
        jsc_stack_dispatch_request_proof,
        expected_live_local_slots,
    )
    .map_err(|mismatch| {
        P6Arm64BranchAwareCallableAdmissionRejection::Arm64JscStackDispatchAdmissionAuthorityMismatch {
            mismatch,
        }
    })
}

pub(super) fn p6_arm64_validate_vm_entry_normal_return_restoration_or_reject<'publication>(
    jit_stub_trace: P6Arm64JitStubTraceAdmissionContext<'_, 'publication>,
    vm_entry_normal_return_restoration_proof: &P6Arm64VerifiedVmEntryNormalReturnRestorationProof<
        'publication,
    >,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64BranchAwareCallableAdmissionRejection<'publication>> {
    p6_arm64_validate_jsc_stack_dispatch_request_or_reject(
        jit_stub_trace,
        vm_entry_normal_return_restoration_proof.jsc_stack_dispatch_request_proof(),
        expected_live_local_slots,
    )?;
    let vm_root_gather = jit_stub_trace
        .verifier_append
        .collector_effects
        .conservative_root_marking
        .vm_root_gather;
    validate_p6_arm64_verified_vm_entry_normal_return_restoration_proof(
        vm_root_gather.top_call_frame_publication,
        vm_entry_normal_return_restoration_proof,
        expected_live_local_slots,
    )
    .map_err(|mismatch| {
        P6Arm64BranchAwareCallableAdmissionRejection::Arm64VmEntryNormalReturnRestorationAuthorityMismatch {
            mismatch,
        }
    })
}

pub(super) fn p6_arm64_validate_vm_entry_exception_unwind_restoration_or_reject<'publication>(
    jit_stub_trace: P6Arm64JitStubTraceAdmissionContext<'_, 'publication>,
    vm_entry_exception_unwind_restoration_proof:
        &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64BranchAwareCallableAdmissionRejection<'publication>> {
    p6_arm64_validate_vm_entry_normal_return_restoration_or_reject(
        jit_stub_trace,
        vm_entry_exception_unwind_restoration_proof.vm_entry_normal_return_restoration_proof(),
        expected_live_local_slots,
    )?;
    let vm_root_gather = jit_stub_trace
        .verifier_append
        .collector_effects
        .conservative_root_marking
        .vm_root_gather;
    validate_p6_arm64_verified_vm_entry_exception_unwind_restoration_proof(
        vm_root_gather.top_call_frame_publication,
        vm_entry_exception_unwind_restoration_proof,
        expected_live_local_slots,
    )
    .map_err(|mismatch| {
        P6Arm64BranchAwareCallableAdmissionRejection::Arm64VmEntryExceptionUnwindRestorationAuthorityMismatch {
            mismatch,
        }
    })
}
