//! ARM64 public native-entry admission proof support.
//!
//! C++ JSC maps this responsibility across `LowLevelInterpreter64.asm`
//! `doVMEntry` / `_llint_call_javascript`, `FrameTracers` and
//! `AssemblyHelpers::prepareCallOperation` top-frame publication,
//! conservative-root append through `SlotVisitor::append(ConservativeRoots)`,
//! JIT stub tracing, and ARM64 generated-frame materialization proofs. This
//! module boundary is extraction-only Rust maintainability work required by the
//! oversized-file guardrail; it preserves the proof-only rejection behavior and
//! does not add public ARM64 admission authority.

use std::convert::Infallible;

use crate::bytecode::{BytecodeIndex, CodeBlock, CoreOpcode};
use crate::gc::HeapConservativeScanAppendReceipt;
use crate::jit::arm64_baseline::Arm64BaselineGeneratedNativeFrameMaterializationMismatch;
use crate::jit::{
    BaselineNativeEntryCallableKind, MachineCodeRange, P6X86_64BaselineSelectedSideExitReason,
    P6X86_64BaselineTerminalPolicy,
};

use super::super::side_exit::{
    p6_jump_if_false_truthiness_side_exit_resume_shape, P6X86_64CallableSideExitReturnSite,
};
use super::super::vm_roots::VmRootGatherPlan;
use super::rooting::{
    validate_p6_arm64_collector_effects_plan, validate_p6_arm64_conservative_root_marking_plan,
    validate_p6_arm64_generated_native_frame_materialization_proof,
    validate_p6_arm64_jit_stub_routine_trace_plan,
    validate_p6_arm64_machine_stack_conservative_rooting_proof,
    validate_p6_arm64_verified_native_frame_machine_stack_residency_proof,
    validate_p6_arm64_verifier_append_proof, validate_p6_arm64_vm_root_gather_plan,
    P6Arm64BranchAwareCallableFallbackRootingProof,
    P6Arm64BranchAwareCallableTopCallFramePublicationProof, P6Arm64CollectorEffectsProofMismatch,
    P6Arm64ConservativeRootMarkingProofMismatch, P6Arm64JitStubRoutineTraceProof,
    P6Arm64JitStubRoutineTraceProofMismatch, P6Arm64MachineStackConservativeRootingProof,
    P6Arm64MachineStackConservativeRootingProofMismatch,
    P6Arm64NativeFrameMachineStackResidencyProofMismatch, P6Arm64SlotVisitorCollectorEffectsProof,
    P6Arm64SlotVisitorConservativeRootMarkingProof,
    P6Arm64VerifiedGeneratedNativeFrameMaterializationProof,
    P6Arm64VerifiedNativeFrameMachineStackResidencyProof, P6Arm64VerifierAppendProofMismatch,
    P6Arm64VerifierSlotVisitorConservativeRootAppendProof, P6Arm64VmRootGatherProofMismatch,
};

#[cfg(test)]
use super::rooting::expected_p6_arm64_collector_effect_action;
#[cfg(test)]
use crate::jit::P6X86_64BaselineSideExitReturnPayload;
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64BranchAwareCallableExitCounts {
    pub(in crate::vm) runtime_helper_native_exits: usize,
    pub(in crate::vm) js_call_native_exits: usize,
    pub(in crate::vm) property_native_exits: usize,
    pub(in crate::vm) loop_backedge_native_exits: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64BranchAwareCallableMetadataProof {
    pub(in crate::vm) readiness_matches_descriptor: bool,
    pub(in crate::vm) readiness_matches_bytecode_snapshot: bool,
    pub(in crate::vm) materialization_matches_install: bool,
    pub(in crate::vm) retained_table_matches_materialization: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(in crate::vm) struct P6Arm64BranchAwareCallableSideExitProof<'a> {
    pub(in crate::vm) site: &'a P6X86_64CallableSideExitReturnSite,
    pub(in crate::vm) code_block: &'a CodeBlock,
    pub(in crate::vm) opcode: Option<CoreOpcode>,
    pub(in crate::vm) target_bytecode_index: BytecodeIndex,
    pub(in crate::vm) fallthrough_bytecode_index: BytecodeIndex,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(in crate::vm) struct P6Arm64BranchAwareCallableAdmissionProofRequest<'a, 'publication> {
    pub(in crate::vm) callable_kind: BaselineNativeEntryCallableKind,
    pub(in crate::vm) terminal_policy: Option<P6X86_64BaselineTerminalPolicy>,
    pub(in crate::vm) descriptor_machine_range: Option<MachineCodeRange>,
    pub(in crate::vm) side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
    pub(in crate::vm) exit_counts: P6Arm64BranchAwareCallableExitCounts,
    pub(in crate::vm) metadata: P6Arm64BranchAwareCallableMetadataProof,
    pub(in crate::vm) fallback_rooting_proof:
        P6Arm64BranchAwareCallableFallbackRootingProof<'publication>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64BranchAwareCallableAdmissionRejection<'publication> {
    MissingBranchAwareSemanticEmission,
    CallableKindNotArm64 {
        actual: BaselineNativeEntryCallableKind,
    },
    MissingTerminalPolicy,
    NonBranchAwareTerminalPolicy {
        actual: P6X86_64BaselineTerminalPolicy,
    },
    MissingDescriptorRange,
    DescriptorRangeInvalid {
        range: MachineCodeRange,
    },
    MissingSideExitPayloadStub,
    UnexpectedSideExit {
        side_exit_index: u32,
        reason: P6X86_64BaselineSelectedSideExitReason,
        opcode: Option<CoreOpcode>,
    },
    MissingNativeReentryTarget {
        side_exit_index: u32,
        resume_bytecode_index: BytecodeIndex,
    },
    NativeReentryTargetOutsideDescriptorRange {
        side_exit_index: u32,
        resume_bytecode_index: BytecodeIndex,
        resume_entry_offset: u32,
        range: MachineCodeRange,
    },
    RuntimeHelperNativeExitPresent {
        count: usize,
    },
    JsCallNativeExitPresent {
        count: usize,
    },
    PropertyNativeExitPresent {
        count: usize,
    },
    LoopBackedgeNativeExitPresent {
        count: usize,
    },
    ReadinessDescriptorMismatch,
    ReadinessBytecodeSnapshotMismatch,
    MaterializationInstallMismatch,
    RetainedTableMaterializationMismatch,
    MissingTopCallFramePublicationProof,
    MissingMachineStackAndConservativeRootingProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
    },
    MachineStackAndConservativeRootingProofMismatch {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        mismatch: P6Arm64MachineStackConservativeRootingProofMismatch,
    },
    MissingVmRootGatherProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
    },
    VmRootGatherProofMismatch {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        mismatch: P6Arm64VmRootGatherProofMismatch,
    },
    MissingRealSlotVisitorConservativeRootMarkingProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
    },
    ConservativeRootMarkingProofMismatch {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        mismatch: P6Arm64ConservativeRootMarkingProofMismatch,
    },
    MissingRealCollectorMarkStackCellStateAndContainerProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
    },
    CollectorEffectsProofMismatch {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        mismatch: P6Arm64CollectorEffectsProofMismatch,
    },
    MissingVerifierSlotVisitorAppendOrAbsenceProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
    },
    VerifierAppendProofMismatch {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        mismatch: P6Arm64VerifierAppendProofMismatch,
    },
    MissingJitStubRoutineTraceProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
    },
    JitStubRoutineTraceProofMismatch {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        mismatch: P6Arm64JitStubRoutineTraceProofMismatch,
    },
    MissingNativeFrameMachineStackResidencyProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
    },
    NativeFrameMachineStackResidencyProofMismatch {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        native_frame_residency_proof:
            P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'publication>,
        mismatch: P6Arm64NativeFrameMachineStackResidencyProofMismatch,
    },
    MissingArm64GeneratedNativeFrameMaterializationProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        native_frame_residency_proof:
            P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'publication>,
    },
    Arm64GeneratedNativeFrameMaterializationProofMismatch {
        mismatch: Arm64BaselineGeneratedNativeFrameMaterializationMismatch,
    },
    MissingArm64JscStackDispatchAdmissionAuthority {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        generated_native_frame_materialization_proof:
            P6Arm64VerifiedGeneratedNativeFrameMaterializationProof<'publication>,
    },
}

pub(in crate::vm) const fn p6_arm64_public_branch_aware_callable_admission_rejection_for_unemitted_seed_candidate(
) -> P6Arm64BranchAwareCallableAdmissionRejection<'static> {
    P6Arm64BranchAwareCallableAdmissionRejection::MissingBranchAwareSemanticEmission
}

fn p6_arm64_conservative_scan_append_receipt_or_reject<'proof, 'publication>(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<
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

#[allow(dead_code)]
pub(in crate::vm) fn p6_arm64_public_branch_aware_callable_admission_proof<'publication>(
    request: &P6Arm64BranchAwareCallableAdmissionProofRequest<'_, 'publication>,
) -> Result<Infallible, P6Arm64BranchAwareCallableAdmissionRejection<'publication>> {
    if request.callable_kind != BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::CallableKindNotArm64 {
                actual: request.callable_kind,
            },
        );
    }

    match request.terminal_policy {
        Some(
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs,
        ) => {}
        Some(actual) => {
            return Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NonBranchAwareTerminalPolicy {
                    actual,
                },
            );
        }
        None => return Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingTerminalPolicy),
    }

    let Some(range) = request.descriptor_machine_range else {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingDescriptorRange);
    };
    if range.size_bytes == 0 || range.end_offset().is_none() {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::DescriptorRangeInvalid { range });
    }

    if request.side_exits.is_empty() {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingSideExitPayloadStub);
    }
    for proof in request.side_exits {
        validate_p6_arm64_branch_aware_callable_side_exit_proof(*proof, range)?;
    }
    // The metadata proof above is the owner/code-block identity evidence for
    // the side-exit records; with that established, JIT.cpp frame allocation is
    // derived from the compiled CodeBlock's unlinked frame shape.
    let generated_frame_shape = request.side_exits[0].code_block.unlinked().frame();
    let expected_live_local_slots = generated_frame_shape.num_callee_locals.max(
        generated_frame_shape
            .num_vars
            .saturating_add(generated_frame_shape.num_temporaries),
    ) as usize;

    let exit_counts = request.exit_counts;
    if exit_counts.runtime_helper_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::RuntimeHelperNativeExitPresent {
                count: exit_counts.runtime_helper_native_exits,
            },
        );
    }
    if exit_counts.js_call_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::JsCallNativeExitPresent {
                count: exit_counts.js_call_native_exits,
            },
        );
    }
    if exit_counts.property_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::PropertyNativeExitPresent {
                count: exit_counts.property_native_exits,
            },
        );
    }
    if exit_counts.loop_backedge_native_exits != 0 {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::LoopBackedgeNativeExitPresent {
                count: exit_counts.loop_backedge_native_exits,
            },
        );
    }

    let metadata = request.metadata;
    if !metadata.readiness_matches_descriptor {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::ReadinessDescriptorMismatch);
    }
    if !metadata.readiness_matches_bytecode_snapshot {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::ReadinessBytecodeSnapshotMismatch,
        );
    }
    if !metadata.materialization_matches_install {
        return Err(P6Arm64BranchAwareCallableAdmissionRejection::MaterializationInstallMismatch);
    }
    if !metadata.retained_table_matches_materialization {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::RetainedTableMaterializationMismatch,
        );
    }

    // C++ JSC publishes an actual CallFrame* into VM::topCallFrame, prepares
    // JIT stub routines, gathers conservative stack/VM roots, appends
    // ConservativeRoots under RootMarkReason::ConservativeScan, optionally
    // appends the same roots to VerifierSlotVisitor, then traces
    // may-be-executing JIT stubs under RootMarkReason::JITStubRoutines. Rust
    // intentionally diverges here: the top-call-frame, VM-root gather, GC
    // marking, collector-effect, verifier append, JIT-stub trace, and
    // machine-stack residency plans are evidence rather than real scratch
    // buffers, CheckpointOSRExitSideState storage, generated stack/register
    // frame materialization, MarkedBlock / PreciseAllocation bits, JSCell
    // header storage, collector-stack storage, verifier mark maps, verifier
    // stack traces, verifier drain, or `markRequiredObjects` traversal. Public
    // ARM64 admission therefore remains rejected until generated native frames
    // are materialized in a C++-scannable layout.
    match &request.fallback_rooting_proof {
        P6Arm64BranchAwareCallableFallbackRootingProof::MissingTopCallFramePublication => {
            Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingTopCallFramePublicationProof)
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithoutConservativeScanAppend(
            top_call_frame_publication,
        ) => Err(
            P6Arm64BranchAwareCallableAdmissionRejection::MissingMachineStackAndConservativeRootingProof {
                top_call_frame_publication: *top_call_frame_publication,
            },
        ),
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithMachineStackConservativeRootingProof {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingVmRootGatherProof {
                top_call_frame_publication: *top_call_frame_publication,
                conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
            })
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherPlan {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            match validate_p6_arm64_vm_root_gather_plan(
                conservative_scan_append_receipt,
                vm_root_gather_plan,
            ) {
            Ok(()) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingRealSlotVisitorConservativeRootMarkingProof {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                },
            ),
            Err(mismatch) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    mismatch,
                },
            ),
            }
        },
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceMachineStackResidencyAndGeneratedNativeFrameMaterializationProof {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
            verifier_append_proof,
            jit_stub_trace_plan,
            generated_native_frame_materialization_proof,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            match validate_p6_arm64_vm_root_gather_plan(
                conservative_scan_append_receipt,
                vm_root_gather_plan,
            ) {
            Ok(()) => match validate_p6_arm64_conservative_root_marking_plan(
                conservative_scan_append_receipt,
                conservative_root_marking_plan,
            ) {
                Ok(()) => match validate_p6_arm64_collector_effects_plan(
                    conservative_root_marking_plan,
                    collector_effects_plan,
                ) {
                    Ok(()) => match validate_p6_arm64_verifier_append_proof(
                        conservative_scan_append_receipt,
                        conservative_root_marking_plan,
                        vm_root_gather_plan,
                        verifier_append_proof,
                    ) {
                        Ok(()) => match validate_p6_arm64_jit_stub_routine_trace_plan(
                            collector_effects_plan,
                            jit_stub_trace_plan.trace_plan(),
                        ) {
                            Ok(()) => match validate_p6_arm64_verified_native_frame_machine_stack_residency_proof(
                                top_call_frame_publication,
                                machine_stack_conservative_rooting_proof,
                                generated_native_frame_materialization_proof
                                    .native_frame_residency_proof(),
                            ) {
                                Ok(()) => match validate_p6_arm64_generated_native_frame_materialization_proof(
                                    top_call_frame_publication,
                                    generated_native_frame_materialization_proof
                                        .native_frame_residency_proof()
                                        .residency_proof(),
                                    generated_native_frame_materialization_proof
                                        .materialization_descriptor(),
                                    expected_live_local_slots,
                                ) {
                                    Ok(()) => Err(
                                        P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64JscStackDispatchAdmissionAuthority {
                                            top_call_frame_publication: *top_call_frame_publication,
                                            conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                            vm_root_gather_plan: vm_root_gather_plan.clone(),
                                            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                            collector_effects_plan: collector_effects_plan.clone(),
                                            verifier_append_proof: verifier_append_proof.clone(),
                                            jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                            generated_native_frame_materialization_proof:
                                                generated_native_frame_materialization_proof.clone(),
                                        },
                                    ),
                                    Err(mismatch) => Err(
                                        P6Arm64BranchAwareCallableAdmissionRejection::Arm64GeneratedNativeFrameMaterializationProofMismatch {
                                            mismatch,
                                        },
                                    ),
                                },
                                Err(mismatch) => Err(
                                    P6Arm64BranchAwareCallableAdmissionRejection::NativeFrameMachineStackResidencyProofMismatch {
                                        top_call_frame_publication: *top_call_frame_publication,
                                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                        collector_effects_plan: collector_effects_plan.clone(),
                                        verifier_append_proof: verifier_append_proof.clone(),
                                        jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                        native_frame_residency_proof:
                                            generated_native_frame_materialization_proof
                                                .native_frame_residency_proof()
                                                .clone(),
                                        mismatch,
                                    },
                                ),
                            },
                            Err(mismatch) => Err(
                                P6Arm64BranchAwareCallableAdmissionRejection::JitStubRoutineTraceProofMismatch {
                                    top_call_frame_publication: *top_call_frame_publication,
                                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                    collector_effects_plan: collector_effects_plan.clone(),
                                    verifier_append_proof: verifier_append_proof.clone(),
                                    jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                    mismatch,
                                },
                            ),
                        },
                        Err(mismatch) => Err(
                            P6Arm64BranchAwareCallableAdmissionRejection::VerifierAppendProofMismatch {
                                top_call_frame_publication: *top_call_frame_publication,
                                conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                vm_root_gather_plan: vm_root_gather_plan.clone(),
                                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                collector_effects_plan: collector_effects_plan.clone(),
                                verifier_append_proof: verifier_append_proof.clone(),
                                mismatch,
                            },
                        ),
                    },
                    Err(mismatch) => Err(
                        P6Arm64BranchAwareCallableAdmissionRejection::CollectorEffectsProofMismatch {
                            top_call_frame_publication: *top_call_frame_publication,
                            conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                            vm_root_gather_plan: vm_root_gather_plan.clone(),
                            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                            collector_effects_plan: collector_effects_plan.clone(),
                            mismatch,
                        },
                    ),
                },
                Err(mismatch) => Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
                        top_call_frame_publication: *top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                        mismatch,
                    },
                ),
            },
            Err(mismatch) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    mismatch,
                },
            ),
        }
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherAndConservativeRootMarkingPlan {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            match validate_p6_arm64_vm_root_gather_plan(
                conservative_scan_append_receipt,
                vm_root_gather_plan,
            ) {
            Ok(()) => match validate_p6_arm64_conservative_root_marking_plan(
                conservative_scan_append_receipt,
                conservative_root_marking_plan,
            ) {
                Ok(()) => Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::MissingRealCollectorMarkStackCellStateAndContainerProof {
                        top_call_frame_publication: *top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                    },
                ),
                Err(mismatch) => Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
                        top_call_frame_publication: *top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                        mismatch,
                    },
                ),
            },
            Err(mismatch) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    mismatch,
                },
            ),
        }
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherAndCollectorEffectsPlan {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            match validate_p6_arm64_vm_root_gather_plan(
                conservative_scan_append_receipt,
                vm_root_gather_plan,
            ) {
            Ok(()) => match validate_p6_arm64_conservative_root_marking_plan(
                conservative_scan_append_receipt,
                conservative_root_marking_plan,
            ) {
                Ok(()) => match validate_p6_arm64_collector_effects_plan(
                    conservative_root_marking_plan,
                    collector_effects_plan,
                ) {
                    Ok(()) => Err(
                        P6Arm64BranchAwareCallableAdmissionRejection::MissingVerifierSlotVisitorAppendOrAbsenceProof {
                            top_call_frame_publication: *top_call_frame_publication,
                            conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                            vm_root_gather_plan: vm_root_gather_plan.clone(),
                            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                            collector_effects_plan: collector_effects_plan.clone(),
                        },
                    ),
                    Err(mismatch) => Err(
                        P6Arm64BranchAwareCallableAdmissionRejection::CollectorEffectsProofMismatch {
                            top_call_frame_publication: *top_call_frame_publication,
                            conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                            vm_root_gather_plan: vm_root_gather_plan.clone(),
                            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                            collector_effects_plan: collector_effects_plan.clone(),
                            mismatch,
                        },
                    ),
                },
                Err(mismatch) => Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
                        top_call_frame_publication: *top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                        mismatch,
                    },
                ),
            },
            Err(mismatch) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    mismatch,
                },
            ),
        }
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsAndVerifierAppendProof {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
            verifier_append_proof,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            match validate_p6_arm64_vm_root_gather_plan(
                conservative_scan_append_receipt,
                vm_root_gather_plan,
            ) {
            Ok(()) => match validate_p6_arm64_conservative_root_marking_plan(
                conservative_scan_append_receipt,
                conservative_root_marking_plan,
            ) {
                Ok(()) => match validate_p6_arm64_collector_effects_plan(
                    conservative_root_marking_plan,
                    collector_effects_plan,
                ) {
                    Ok(()) => match validate_p6_arm64_verifier_append_proof(
                        conservative_scan_append_receipt,
                        conservative_root_marking_plan,
                        vm_root_gather_plan,
                        verifier_append_proof,
                    ) {
                        Ok(()) => Err(
                            P6Arm64BranchAwareCallableAdmissionRejection::MissingJitStubRoutineTraceProof {
                                top_call_frame_publication: *top_call_frame_publication,
                                conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                vm_root_gather_plan: vm_root_gather_plan.clone(),
                                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                collector_effects_plan: collector_effects_plan.clone(),
                                verifier_append_proof: verifier_append_proof.clone(),
                            },
                        ),
                        Err(mismatch) => Err(
                            P6Arm64BranchAwareCallableAdmissionRejection::VerifierAppendProofMismatch {
                                top_call_frame_publication: *top_call_frame_publication,
                                conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                vm_root_gather_plan: vm_root_gather_plan.clone(),
                                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                collector_effects_plan: collector_effects_plan.clone(),
                                verifier_append_proof: verifier_append_proof.clone(),
                                mismatch,
                            },
                        ),
                    },
                    Err(mismatch) => Err(
                        P6Arm64BranchAwareCallableAdmissionRejection::CollectorEffectsProofMismatch {
                            top_call_frame_publication: *top_call_frame_publication,
                            conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                            vm_root_gather_plan: vm_root_gather_plan.clone(),
                            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                            collector_effects_plan: collector_effects_plan.clone(),
                            mismatch,
                        },
                    ),
                },
                Err(mismatch) => Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
                        top_call_frame_publication: *top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                        mismatch,
                    },
                ),
            },
            Err(mismatch) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    mismatch,
                },
            ),
        }
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierAppendAndJitStubTracePlan {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
            verifier_append_proof,
            jit_stub_trace_plan,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            match validate_p6_arm64_vm_root_gather_plan(
                conservative_scan_append_receipt,
                vm_root_gather_plan,
            ) {
            Ok(()) => match validate_p6_arm64_conservative_root_marking_plan(
                conservative_scan_append_receipt,
                conservative_root_marking_plan,
            ) {
                Ok(()) => match validate_p6_arm64_collector_effects_plan(
                    conservative_root_marking_plan,
                    collector_effects_plan,
                ) {
                    Ok(()) => match validate_p6_arm64_verifier_append_proof(
                        conservative_scan_append_receipt,
                        conservative_root_marking_plan,
                        vm_root_gather_plan,
                        verifier_append_proof,
                    ) {
                        Ok(()) => match validate_p6_arm64_jit_stub_routine_trace_plan(
                            collector_effects_plan,
                            jit_stub_trace_plan.trace_plan(),
                        ) {
                            Ok(()) => Err(
                                P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeFrameMachineStackResidencyProof {
                                    top_call_frame_publication: *top_call_frame_publication,
                                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                    collector_effects_plan: collector_effects_plan.clone(),
                                    verifier_append_proof: verifier_append_proof.clone(),
                                    jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                },
                            ),
                            Err(mismatch) => Err(
                                P6Arm64BranchAwareCallableAdmissionRejection::JitStubRoutineTraceProofMismatch {
                                    top_call_frame_publication: *top_call_frame_publication,
                                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                    collector_effects_plan: collector_effects_plan.clone(),
                                    verifier_append_proof: verifier_append_proof.clone(),
                                    jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                    mismatch,
                                },
                            ),
                        },
                        Err(mismatch) => Err(
                            P6Arm64BranchAwareCallableAdmissionRejection::VerifierAppendProofMismatch {
                                top_call_frame_publication: *top_call_frame_publication,
                                conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                vm_root_gather_plan: vm_root_gather_plan.clone(),
                                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                collector_effects_plan: collector_effects_plan.clone(),
                                verifier_append_proof: verifier_append_proof.clone(),
                                mismatch,
                            },
                        ),
                    },
                    Err(mismatch) => Err(
                        P6Arm64BranchAwareCallableAdmissionRejection::CollectorEffectsProofMismatch {
                            top_call_frame_publication: *top_call_frame_publication,
                            conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                            vm_root_gather_plan: vm_root_gather_plan.clone(),
                            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                            collector_effects_plan: collector_effects_plan.clone(),
                            mismatch,
                        },
                    ),
                },
                Err(mismatch) => Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
                        top_call_frame_publication: *top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                        mismatch,
                    },
                ),
            },
            Err(mismatch) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    mismatch,
                },
            ),
        }
        }
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceAndMachineStackResidencyProof {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
            verifier_append_proof,
            jit_stub_trace_plan,
            native_frame_residency_proof,
        } => {
            let conservative_scan_append_receipt =
                p6_arm64_conservative_scan_append_receipt_or_reject(
                    top_call_frame_publication,
                    machine_stack_conservative_rooting_proof,
                )?;
            match validate_p6_arm64_vm_root_gather_plan(
                conservative_scan_append_receipt,
                vm_root_gather_plan,
            ) {
            Ok(()) => match validate_p6_arm64_conservative_root_marking_plan(
                conservative_scan_append_receipt,
                conservative_root_marking_plan,
            ) {
                Ok(()) => match validate_p6_arm64_collector_effects_plan(
                    conservative_root_marking_plan,
                    collector_effects_plan,
                ) {
                    Ok(()) => match validate_p6_arm64_verifier_append_proof(
                        conservative_scan_append_receipt,
                        conservative_root_marking_plan,
                        vm_root_gather_plan,
                        verifier_append_proof,
                    ) {
                        Ok(()) => match validate_p6_arm64_jit_stub_routine_trace_plan(
                            collector_effects_plan,
                            jit_stub_trace_plan.trace_plan(),
                        ) {
                            Ok(()) => match validate_p6_arm64_verified_native_frame_machine_stack_residency_proof(
                                top_call_frame_publication,
                                machine_stack_conservative_rooting_proof,
                                native_frame_residency_proof,
                            ) {
                                Ok(()) => Err(
                                    P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64GeneratedNativeFrameMaterializationProof {
                                        top_call_frame_publication: *top_call_frame_publication,
                                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                        collector_effects_plan: collector_effects_plan.clone(),
                                        verifier_append_proof: verifier_append_proof.clone(),
                                        jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                        native_frame_residency_proof: native_frame_residency_proof.clone(),
                                    },
                                ),
                                Err(mismatch) => Err(
                                    P6Arm64BranchAwareCallableAdmissionRejection::NativeFrameMachineStackResidencyProofMismatch {
                                        top_call_frame_publication: *top_call_frame_publication,
                                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                        collector_effects_plan: collector_effects_plan.clone(),
                                        verifier_append_proof: verifier_append_proof.clone(),
                                        jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                        native_frame_residency_proof: native_frame_residency_proof.clone(),
                                        mismatch,
                                    },
                                ),
                            },
                            Err(mismatch) => Err(
                                P6Arm64BranchAwareCallableAdmissionRejection::JitStubRoutineTraceProofMismatch {
                                    top_call_frame_publication: *top_call_frame_publication,
                                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                                    conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                    collector_effects_plan: collector_effects_plan.clone(),
                                    verifier_append_proof: verifier_append_proof.clone(),
                                    jit_stub_trace_plan: jit_stub_trace_plan.clone(),
                                    mismatch,
                                },
                            ),
                        },
                        Err(mismatch) => Err(
                            P6Arm64BranchAwareCallableAdmissionRejection::VerifierAppendProofMismatch {
                                top_call_frame_publication: *top_call_frame_publication,
                                conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                                vm_root_gather_plan: vm_root_gather_plan.clone(),
                                conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                                collector_effects_plan: collector_effects_plan.clone(),
                                verifier_append_proof: verifier_append_proof.clone(),
                                mismatch,
                            },
                        ),
                    },
                    Err(mismatch) => Err(
                        P6Arm64BranchAwareCallableAdmissionRejection::CollectorEffectsProofMismatch {
                            top_call_frame_publication: *top_call_frame_publication,
                            conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                            vm_root_gather_plan: vm_root_gather_plan.clone(),
                            conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                            collector_effects_plan: collector_effects_plan.clone(),
                            mismatch,
                        },
                    ),
                },
                Err(mismatch) => Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::ConservativeRootMarkingProofMismatch {
                        top_call_frame_publication: *top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: vm_root_gather_plan.clone(),
                        conservative_root_marking_plan: conservative_root_marking_plan.clone(),
                        mismatch,
                    },
                ),
            },
            Err(mismatch) => Err(
                P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                    top_call_frame_publication: *top_call_frame_publication,
                    conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                    vm_root_gather_plan: vm_root_gather_plan.clone(),
                    mismatch,
                },
            ),
        }
        }
    }
}

#[allow(dead_code)]
fn validate_p6_arm64_branch_aware_callable_side_exit_proof<'publication>(
    proof: P6Arm64BranchAwareCallableSideExitProof<'_>,
    range: MachineCodeRange,
) -> Result<(), P6Arm64BranchAwareCallableAdmissionRejection<'publication>> {
    if proof.site.reason != P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        || proof.opcode != Some(CoreOpcode::JumpIfFalse)
    {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                side_exit_index: proof.site.side_exit_index,
                reason: proof.site.reason,
                opcode: proof.opcode,
            },
        );
    }

    let Some(shape) =
        p6_jump_if_false_truthiness_side_exit_resume_shape(proof.code_block, proof.site)
    else {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                side_exit_index: proof.site.side_exit_index,
                reason: proof.site.reason,
                opcode: proof.opcode,
            },
        );
    };
    if proof.target_bytecode_index != shape.taken_target.resume_bytecode_index {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                side_exit_index: proof.site.side_exit_index,
                resume_bytecode_index: proof.target_bytecode_index,
            },
        );
    }
    if proof.fallthrough_bytecode_index != shape.fallthrough_target.resume_bytecode_index {
        return Err(
            P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                side_exit_index: proof.site.side_exit_index,
                resume_bytecode_index: proof.fallthrough_bytecode_index,
            },
        );
    }

    for target in [shape.taken_target, shape.fallthrough_target] {
        if !p6_arm64_image_entry_offset_points_inside_descriptor_range(
            target.resume_entry_offset,
            range,
        ) {
            return Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NativeReentryTargetOutsideDescriptorRange {
                    side_exit_index: proof.site.side_exit_index,
                    resume_bytecode_index: target.resume_bytecode_index,
                    resume_entry_offset: target.resume_entry_offset,
                    range,
                },
            );
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn p6_arm64_image_entry_offset_points_inside_descriptor_range(
    image_entry_offset: u32,
    range: MachineCodeRange,
) -> bool {
    let Some(end_offset) = range.end_offset() else {
        return false;
    };
    let Some(allocation_relative_entry_offset) = range.start_offset.checked_add(image_entry_offset)
    else {
        return false;
    };
    image_entry_offset < range.size_bytes
        && allocation_relative_entry_offset >= range.start_offset
        && allocation_relative_entry_offset < end_offset
}

#[cfg(test)]
#[path = "frame_materialization_tests.rs"]
mod frame_materialization_tests;

#[cfg(test)]
#[path = "arm64_native_frame_residency_admission_tests.rs"]
mod arm64_native_frame_residency_admission_tests;

#[cfg(test)]
#[path = "arm64_rooting_admission_tests.rs"]
mod arm64_rooting_admission_tests;

#[cfg(test)]
#[path = "arm64_marking_admission_tests.rs"]
mod arm64_marking_admission_tests;

#[cfg(test)]
#[path = "arm64_collector_effects_admission_tests.rs"]
mod arm64_collector_effects_admission_tests;

#[cfg(test)]
#[path = "arm64_verifier_admission_tests.rs"]
mod arm64_verifier_admission_tests;

#[cfg(test)]
#[path = "arm64_jit_stub_admission_tests.rs"]
mod arm64_jit_stub_admission_tests;

#[cfg(test)]
pub(super) mod tests {
    use super::super::rooting::{
        validate_p6_arm64_native_frame_machine_stack_residency_proof,
        P6Arm64JitStubRoutineConservativeScanHookProof, P6Arm64JitStubRoutineTraceProof,
        P6Arm64MachineStackConservativeRootingProof, P6Arm64NativeFrameMachineStackResidencyProof,
        P6Arm64NativeFrameMachineStackSpanKind, P6Arm64NativeFrameMachineStackSpanRecord,
        P6Arm64NativeRootSlotKind, P6Arm64NativeRootSlotRecord,
        P6Arm64SlotVisitorConservativeRootMarkingProof,
        P6Arm64VerifiedGeneratedNativeFrameMaterializationProof,
        P6Arm64VerifiedGeneratedNativeFrameMaterializationProofError,
        P6Arm64VerifiedNativeFrameMachineStackResidencyProof,
        P6Arm64VerifiedNativeFrameMachineStackResidencyProofError,
        P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
    };
    use super::*;
    use std::convert::Infallible;

    use crate::bytecode::{
        CodeBlockEntrypoints, CodeBlockLifecycleState, CodeKind, InterpreterEntrySlot, LinkContext,
        Operand, OperandWidth, PackedInstructionStream, RegisterFrameShape, TypedInstruction,
        UnlinkedCodeBlock, VirtualRegister,
    };
    use crate::gc::{
        AllocationMode, CellMetadata, ConservativeRootCell, ConservativeRootSpan,
        ConservativeRoots, GcConductor, GcPhase, Heap, HeapAllocationRequest,
        HeapConservativeScanAppendReceipt, HeapEpoch, HeapId, HeapStateDescriptor,
        JscMachineStackMarker, MutatorState,
    };
    use crate::jit::code::BaselineEntryArtifact;
    use crate::jit::{
        CodeFinalizationAuthority, CodeLiveness, CodeOrigin, CodeOriginKind, CodeOwnership,
        CodeRetentionPolicy, EntryAbi, Entrypoint, EntrypointKind, ExecutableAllocationId,
        ExecutableAllocationLifecycle, ExecutableMemoryProtection, ExecutableMutationAuthority,
        GcAwareJitStubRoutineDescriptor, JitCodeArtifact, JitCodeId,
        JitStubRoutineCandidateAddress, JitStubRoutineSetDescriptor, JitType, MachineCodeHandle,
        MachineCodeOwnership, P6BaselineNativeReentryTargetRecord,
    };
    use crate::runtime::{
        ArityCheckMode, CallFrameId, CellId, CodeBlockId, CodeSpecializationKind, EntryFrameId,
        NativeCodeId, RuntimeValue,
    };

    use super::super::super::arm64_native_entry::{
        enter_arm64_native_entry_stack_publication,
        prove_arm64_native_entry_do_vm_entry_stack_layout,
        prove_arm64_native_entry_jsc_stack_call_request,
        prove_arm64_native_entry_launch_descriptor, with_arm64_native_entry_padded_stack_frame,
        with_arm64_native_entry_stack_frame, Arm64NativeEntryJscStackCallRequestProof,
        Arm64NativeEntryLaunchProofRequest, Arm64NativeEntryStackFrameRequest,
        Arm64NativeEntryStackPublicationError,
    };
    use super::super::super::entry::{
        EntryKind, FrameAddress, VmEntryCallFrameMetadata, VmEntryLaunchArgumentValue,
        VmEntryLaunchDescriptor, VmEntryLaunchScope, VmEntryState,
    };
    use super::super::super::vm_roots::{
        VmRootGatherDescriptor, VmRootGatherError, VmRootSource, VmScratchBufferCandidateSlot,
        VmScratchBufferDescriptor, VmScratchBufferId, ENCODED_JS_VALUE_BYTES,
    };
    use super::super::super::{
        BaselineEntryGateOutcome, BaselineEntryGateRecord, BaselineNativeEntryExecutionPolicy,
        BaselineNativeEntryReadinessOutcome, BaselineNativeEntryReadinessRecord,
    };

    fn bci(offset: u32) -> BytecodeIndex {
        BytecodeIndex::from_offset(offset)
    }

    fn range() -> MachineCodeRange {
        MachineCodeRange {
            allocation: ExecutableAllocationId(1),
            start_offset: 128,
            size_bytes: 64,
        }
    }

    fn target(
        resume_bytecode_index: BytecodeIndex,
        resume_entry_offset: u32,
    ) -> P6BaselineNativeReentryTargetRecord {
        P6BaselineNativeReentryTargetRecord {
            resume_bytecode_index,
            resume_entry_offset,
        }
    }

    fn local(index: u32) -> VirtualRegister {
        VirtualRegister::local(index)
    }

    fn typed_core_instruction_with_operands(
        offset: u32,
        opcode: CoreOpcode,
        operands: Vec<Operand>,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: opcode.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(bci(offset)),
        }
    }

    fn code_block_from_instructions(instructions: Vec<TypedInstruction>) -> CodeBlock {
        CodeBlock::from_unlinked(
            UnlinkedCodeBlock::new(
                CodeKind::Program,
                PackedInstructionStream::from_typed_placeholder(instructions),
            )
            .with_frame(RegisterFrameShape {
                num_parameters_including_this: 1,
                num_vars: 1,
                num_callee_locals: 0,
                num_temporaries: 0,
                special: Default::default(),
            }),
            LinkContext::default(),
        )
        .with_entrypoints(CodeBlockEntrypoints {
            interpreter: Some(InterpreterEntrySlot(0)),
            ..CodeBlockEntrypoints::default()
        })
        .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    pub(super) fn jump_if_false_code_block(taken_target: u32) -> CodeBlock {
        code_block_from_instructions(vec![
            typed_core_instruction_with_operands(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            typed_core_instruction_with_operands(
                1,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(bci(taken_target)),
                ],
            ),
            typed_core_instruction_with_operands(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(local(0))],
            ),
            typed_core_instruction_with_operands(
                3,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(1)],
            ),
            typed_core_instruction_with_operands(
                4,
                CoreOpcode::Return,
                vec![Operand::Register(local(0))],
            ),
        ])
    }

    fn terminal_jump_if_false_code_block() -> CodeBlock {
        code_block_from_instructions(vec![
            typed_core_instruction_with_operands(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            typed_core_instruction_with_operands(
                1,
                CoreOpcode::JumpIfFalse,
                vec![Operand::Register(local(0)), Operand::BytecodeIndex(bci(0))],
            ),
        ])
    }

    pub(super) fn jump_if_false_site() -> P6X86_64CallableSideExitReturnSite {
        P6X86_64CallableSideExitReturnSite {
            bytecode_index: bci(1),
            reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            side_exit_index: 0,
            resume_bytecode_index: None,
            resume_entry_offset: None,
            native_reentry_targets: vec![target(bci(4), 12), target(bci(2), 28)],
            encoded_payload: P6X86_64BaselineSideExitReturnPayload::encode(0),
        }
    }

    pub(super) fn branch_aware_side_exit_proof<'a>(
        code_block: &'a CodeBlock,
        site: &'a P6X86_64CallableSideExitReturnSite,
    ) -> P6Arm64BranchAwareCallableSideExitProof<'a> {
        branch_aware_side_exit_proof_with_labels(
            code_block,
            site,
            Some(CoreOpcode::JumpIfFalse),
            bci(4),
            bci(2),
        )
    }

    fn branch_aware_side_exit_proof_with_labels<'a>(
        code_block: &'a CodeBlock,
        site: &'a P6X86_64CallableSideExitReturnSite,
        opcode: Option<CoreOpcode>,
        target_bytecode_index: BytecodeIndex,
        fallthrough_bytecode_index: BytecodeIndex,
    ) -> P6Arm64BranchAwareCallableSideExitProof<'a> {
        P6Arm64BranchAwareCallableSideExitProof {
            site,
            code_block,
            opcode,
            target_bytecode_index,
            fallthrough_bytecode_index,
        }
    }

    fn valid_metadata() -> P6Arm64BranchAwareCallableMetadataProof {
        P6Arm64BranchAwareCallableMetadataProof {
            readiness_matches_descriptor: true,
            readiness_matches_bytecode_snapshot: true,
            materialization_matches_install: true,
            retained_table_matches_materialization: true,
        }
    }

    pub(super) fn valid_request<'a, 'publication>(
        side_exits: &'a [P6Arm64BranchAwareCallableSideExitProof<'a>],
    ) -> P6Arm64BranchAwareCallableAdmissionProofRequest<'a, 'publication> {
        P6Arm64BranchAwareCallableAdmissionProofRequest {
            callable_kind: BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
            terminal_policy: Some(
                P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs,
            ),
            descriptor_machine_range: Some(range()),
            side_exits,
            exit_counts: P6Arm64BranchAwareCallableExitCounts::default(),
            metadata: valid_metadata(),
            fallback_rooting_proof:
                P6Arm64BranchAwareCallableFallbackRootingProof::MissingTopCallFramePublication,
        }
    }

    fn stack_frame_request(
        previous_top_call_frame: Option<FrameAddress>,
        previous_top_entry_frame: Option<FrameAddress>,
    ) -> Arm64NativeEntryStackFrameRequest<0> {
        Arm64NativeEntryStackFrameRequest {
            vm: 0x1000,
            context: 0x2000,
            previous_top_call_frame,
            previous_top_entry_frame,
            code_block: 0x5000,
            callee: 0x6000,
            this_value: 0x7000,
            arguments: [],
            live_local_count: 1,
        }
    }

    fn padded_stack_frame_request() -> Arm64NativeEntryStackFrameRequest<2> {
        Arm64NativeEntryStackFrameRequest {
            vm: 0x1000,
            context: 0x2000,
            previous_top_call_frame: None,
            previous_top_entry_frame: None,
            code_block: 0x5000,
            callee: 0x6000,
            this_value: 0x7000,
            arguments: [0x8000, 0x9000],
            live_local_count: 1,
        }
    }

    fn launch_descriptor_for_stack_frame(
        argument_count_excluding_this: u32,
        padded_argument_count_excluding_this: u32,
    ) -> VmEntryLaunchDescriptor {
        let owner = CodeBlockId(CellId(20));
        let artifact = baseline_entry_artifact(owner, 20);
        let descriptor = artifact
            .validate_native_entry_descriptor()
            .expect("native entry descriptor");
        let readiness = BaselineNativeEntryReadinessRecord {
            ordinal: 7,
            owner,
            materialization_ordinal: 1,
            install_ordinal: 2,
            artifact_id: Some(artifact.id),
            native_code: Some(artifact.native_code),
            machine_code: Some(artifact.machine_code),
            machine_range: Some(artifact.machine_code.range),
            bytecode_snapshot: None,
            body_capability: None,
            execution_policy: BaselineNativeEntryExecutionPolicy::Enabled,
            descriptor: Some(descriptor),
            callable: None,
            outcome: BaselineNativeEntryReadinessOutcome::Ready,
        };
        let gate = BaselineEntryGateRecord {
            owner,
            requested_tier: JitType::Baseline,
            native_artifact: Some(artifact),
            native_entry_readiness_ordinal: Some(readiness.ordinal),
            generated_artifact: None,
            outcome: BaselineEntryGateOutcome::NativeEntryReady,
        };
        VmEntryLaunchDescriptor::baseline_native_entry(
            launch_scope(owner),
            launch_call_frame(
                owner,
                argument_count_excluding_this,
                padded_argument_count_excluding_this,
            ),
            gate,
            &readiness,
        )
        .expect("valid ARM64 launch descriptor")
    }

    fn launch_scope(owner: CodeBlockId) -> VmEntryLaunchScope {
        VmEntryLaunchScope {
            owner,
            entry_code_block: Some(owner),
            active_entry_frame: Some(EntryFrameId(1)),
            previous_entry_frame: None,
            saved_top_call_frame: None,
            active_top_call_frame: Some(CallFrameId(2)),
        }
    }

    fn launch_call_frame(
        owner: CodeBlockId,
        argument_count_excluding_this: u32,
        padded_argument_count_excluding_this: u32,
    ) -> VmEntryCallFrameMetadata {
        VmEntryCallFrameMetadata {
            frame: CallFrameId(2),
            entry_frame: Some(EntryFrameId(1)),
            caller_frame: Some(CallFrameId(1)),
            code_block: Some(owner),
            callee: None,
            callee_value: None,
            context: None,
            global_object: None,
            entry_value: VmEntryLaunchArgumentValue::This(RuntimeValue::from_i32(41)),
            argument_count_including_this: argument_count_excluding_this
                .checked_add(1)
                .expect("argument count including this"),
            provided_argument_count: argument_count_excluding_this,
            padded_argument_count: padded_argument_count_excluding_this
                .checked_add(1)
                .expect("padded argument count including this"),
            specialization: CodeSpecializationKind::Call,
            arity_mode: ArityCheckMode::AlreadyChecked,
        }
    }

    fn baseline_entry_artifact(owner: CodeBlockId, id: u64) -> BaselineEntryArtifact {
        baseline_artifact(owner, id)
            .validate_baseline_entry_artifact(owner)
            .expect("baseline entry artifact")
    }

    fn baseline_artifact(owner: CodeBlockId, id: u64) -> JitCodeArtifact {
        let code = JitCodeId(id);
        let native_code = NativeCodeId(id as u32 + 100);
        let allocation = ExecutableAllocationId(id + 200);
        JitCodeArtifact {
            id: code,
            tier: JitType::Baseline,
            origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: Some(owner),
                executable: None,
                bytecode_index: Some(0),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: Some(native_code),
            machine_code: Some(MachineCodeHandle {
                allocation,
                owner: MachineCodeOwnership::CodeBlock(owner),
                range: MachineCodeRange {
                    allocation,
                    start_offset: 0,
                    size_bytes: 64,
                },
                symbol: Some(native_code),
                protection: ExecutableMemoryProtection::Executable,
                lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::LinkBuffer,
            }),
            entrypoint: Entrypoint {
                kind: EntrypointKind::GeneratedCode,
                abi: EntryAbi::GeneratedCode,
                code: Some(code),
                boundary: None,
            },
            patchpoints: Vec::new(),
            dependencies: Vec::new(),
            byproducts: Vec::new(),
            disassembly: None,
            liveness: CodeLiveness::Live,
            finalization_authority: CodeFinalizationAuthority::MainThread,
        }
    }

    fn do_vm_entry_layout_for_stack_frame(
        argument_count_excluding_this: u32,
        padded_argument_count_excluding_this: u32,
    ) -> super::super::super::arm64_native_entry::Arm64NativeEntryDoVmEntryLayoutProof {
        let descriptor = launch_descriptor_for_stack_frame(
            argument_count_excluding_this,
            padded_argument_count_excluding_this,
        );
        let launch_proof =
            prove_arm64_native_entry_launch_descriptor(Arm64NativeEntryLaunchProofRequest {
                launch_descriptor: &descriptor,
                callable_kind: BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
                callable_token: descriptor.native_entry.normal_entry,
            })
            .expect("ARM64 launch proof");
        prove_arm64_native_entry_do_vm_entry_stack_layout(launch_proof)
            .expect("doVMEntry stack layout proof")
    }

    fn with_stack_top_call_frame_publication_from_request<
        const LOCAL_AREA_WORDS: usize,
        const ARGUMENTS_EXCLUDING_THIS: usize,
        const PADDED_ARGUMENTS_EXCLUDING_THIS: usize,
        R,
    >(
        request: Arm64NativeEntryStackFrameRequest<ARGUMENTS_EXCLUDING_THIS>,
        body: impl for<'publication> FnOnce(
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        ) -> R,
    ) -> R {
        with_stack_top_call_frame_publication_and_stack_call_from_request::<
            LOCAL_AREA_WORDS,
            ARGUMENTS_EXCLUDING_THIS,
            PADDED_ARGUMENTS_EXCLUDING_THIS,
            R,
        >(request, |top_call_frame_publication, _stack_call_proof| {
            body(top_call_frame_publication)
        })
    }

    pub(super) fn with_stack_top_call_frame_publication_and_stack_call_from_request<
        const LOCAL_AREA_WORDS: usize,
        const ARGUMENTS_EXCLUDING_THIS: usize,
        const PADDED_ARGUMENTS_EXCLUDING_THIS: usize,
        R,
    >(
        request: Arm64NativeEntryStackFrameRequest<ARGUMENTS_EXCLUDING_THIS>,
        body: impl for<'publication> FnOnce(
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
            Arm64NativeEntryJscStackCallRequestProof,
        ) -> R,
    ) -> R {
        let layout_proof = do_vm_entry_layout_for_stack_frame(
            u32::try_from(ARGUMENTS_EXCLUDING_THIS).expect("argument count"),
            u32::try_from(PADDED_ARGUMENTS_EXCLUDING_THIS).expect("padded argument count"),
        );
        let mut result = None;
        with_arm64_native_entry_padded_stack_frame::<
            LOCAL_AREA_WORDS,
            ARGUMENTS_EXCLUDING_THIS,
            PADDED_ARGUMENTS_EXCLUDING_THIS,
        >(request, |stack_frame| {
            let stack_call_proof =
                prove_arm64_native_entry_jsc_stack_call_request(layout_proof, &stack_frame)
                    .expect("JSC stack-call request proof");
            let mut state = VmEntryState::default();
            let guard = enter_arm64_native_entry_stack_publication(
                &mut state,
                &stack_frame,
                EntryKind::Script,
                HeapId::default(),
            )
            .expect("stack-local VM top-frame publication");
            let top_call_frame_publication =
                P6Arm64BranchAwareCallableTopCallFramePublicationProof::from_stack_publication_guard(
                    &guard,
                );
            assert_eq!(
                top_call_frame_publication.publication.published_top_frame,
                guard.top_call_frame().expect("published stack CallFrame")
            );
            assert_eq!(
                top_call_frame_publication.publication.current_entry_frame,
                guard.top_entry_frame().expect("published stack EntryFrame")
            );
            assert_eq!(
                top_call_frame_publication
                    .publication
                    .vm_entry_previous_top_call_frame,
                None
            );
            assert_eq!(
                top_call_frame_publication
                    .publication
                    .vm_entry_previous_top_entry_frame,
                None
            );
            result = Some(body(top_call_frame_publication, stack_call_proof));
            drop(guard);
            assert_eq!(state.top_frame(), None);
            assert_eq!(state.entry_frame(), None);
        })
        .expect("stack frame fixture");
        result.expect("stack publication fixture body")
    }

    fn with_stack_top_call_frame_publication<R>(
        body: impl for<'publication> FnOnce(
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        ) -> R,
    ) -> R {
        with_stack_top_call_frame_publication_from_request::<2, 0, 0, R>(
            stack_frame_request(None, None),
            body,
        )
    }

    pub(super) fn with_stack_top_call_frame_publication_and_stack_call<R>(
        body: impl for<'publication> FnOnce(
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
            Arm64NativeEntryJscStackCallRequestProof,
        ) -> R,
    ) -> R {
        with_stack_top_call_frame_publication_and_stack_call_from_request::<2, 0, 0, R>(
            stack_frame_request(None, None),
            body,
        )
    }

    pub(super) fn with_padded_stack_top_call_frame_publication<R>(
        body: impl for<'publication> FnOnce(
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        ) -> R,
    ) -> R {
        with_stack_top_call_frame_publication_from_request::<2, 2, 4, R>(
            padded_stack_frame_request(),
            body,
        )
    }

    pub(super) fn heap_with_conservative_scan_append_receipt(
    ) -> (Heap, HeapConservativeScanAppendReceipt) {
        let mut heap = Heap::new();
        let cell = heap
            .allocate_record(HeapAllocationRequest {
                heap: heap.id(),
                subspace: "object",
                metadata: CellMetadata::default(),
                byte_size: 64,
                mode: AllocationMode::Normal,
                may_trigger_collection: false,
            })
            .map(|response| response.cell)
            .expect("test allocation");
        let payload = 0x5000;
        heap.bind_cell_payload(cell, payload)
            .expect("bind conservative-root payload");
        heap.publish_cell(cell)
            .expect("publish conservative root cell");

        let mut roots = ConservativeRoots::new();
        roots.add_validated_cell(
            heap.validate_conservative_root_candidate_exact_payload(payload)
                .expect("validated conservative root"),
        );
        heap.ingest_conservative_roots(roots)
            .expect("ingest conservative roots");
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Collecting,
            GcConductor::Mutator,
        );

        let visitor = heap.slot_visitor_descriptor("native-reentry-conservative-scan-test");
        let receipt = heap
            .append_conservative_roots_to_slot_visitor_descriptor(&visitor)
            .expect("heap conservative-scan append receipt");
        assert_eq!(receipt.conservative_root_count, 1);
        assert_eq!(receipt.appended_record_count, 1);
        (heap, receipt)
    }

    pub(super) fn conservative_root_marking_and_collector_effects_proof() -> (
        HeapConservativeScanAppendReceipt,
        P6Arm64SlotVisitorConservativeRootMarkingProof,
        P6Arm64SlotVisitorCollectorEffectsProof,
    ) {
        let (mut heap, receipt) = heap_with_conservative_scan_append_receipt();
        let marking_proof =
            P6Arm64SlotVisitorConservativeRootMarkingProof::from_conservative_scan_append_receipt(
                &receipt, &mut heap,
            )
            .expect("slot visitor conservative-root marking plan");
        let marking_plan = marking_proof.marking_plan();
        let collector_effects_proof =
            P6Arm64SlotVisitorCollectorEffectsProof::from_conservative_root_marking_proof(
                &marking_proof,
                &mut heap,
            )
            .expect("slot visitor collector-effects proof");
        let collector_effects_plan = collector_effects_proof.collector_effects_plan();

        assert_eq!(collector_effects_plan.heap, marking_plan.heap);
        assert_eq!(
            collector_effects_plan.marking_epoch,
            marking_plan.marking_epoch
        );
        assert_eq!(
            collector_effects_plan.records[0].marking_record,
            marking_plan.records[0]
        );
        (receipt, marking_proof, collector_effects_proof)
    }

    fn jit_stub_routine(
        id: u64,
        start_offset: u32,
        size_bytes: u32,
        immutable: bool,
        required_object_edges: Vec<CellId>,
    ) -> GcAwareJitStubRoutineDescriptor {
        GcAwareJitStubRoutineDescriptor {
            id: JitCodeId(id),
            code: JitCodeId(10_000 + id),
            range: MachineCodeRange {
                allocation: ExecutableAllocationId(17),
                start_offset,
                size_bytes,
            },
            liveness: CodeLiveness::Live,
            retention: CodeRetentionPolicy::SharedStubRegistry,
            is_code_immutable: immutable,
            may_be_executing: false,
            required_object_edges,
        }
    }

    pub(super) fn jit_stub_set_descriptor() -> JitStubRoutineSetDescriptor {
        JitStubRoutineSetDescriptor::new(vec![
            jit_stub_routine(1, 320, 24, false, vec![CellId(91), CellId(92)]),
            jit_stub_routine(2, 420, 24, false, vec![CellId(93)]),
            jit_stub_routine(9, 260, 16, true, vec![CellId(94)]),
        ])
    }

    pub(super) fn jit_stub_candidate_for_first_mutable_routine() -> JitStubRoutineCandidateAddress {
        JitStubRoutineCandidateAddress {
            allocation: ExecutableAllocationId(17),
            offset: 328,
        }
    }

    pub(super) fn jit_stub_scan_hook_proof() -> P6Arm64JitStubRoutineConservativeScanHookProof {
        let set = jit_stub_set_descriptor();
        P6Arm64JitStubRoutineConservativeScanHookProof::from_prepared_set_and_conservative_scan_hook_candidates(
            &set,
            [jit_stub_candidate_for_first_mutable_routine()],
        )
        .expect("JIT stub conservative-scan hook proof")
    }

    pub(super) fn jit_stub_trace_proof(
        collector_effects_proof: &P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: &P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
    ) -> P6Arm64JitStubRoutineTraceProof {
        let scan_hook_proof = jit_stub_scan_hook_proof();
        let trace_proof =
            P6Arm64JitStubRoutineTraceProof::from_collector_effects_verifier_and_scan_hook_proofs(
                collector_effects_proof,
                verifier_append_proof,
                &scan_hook_proof,
            )
            .expect("trace marked JIT stub routine proof");
        let trace_plan = trace_proof.trace_plan();
        assert_eq!(trace_plan.traced_routine_count, 1);
        assert_eq!(trace_plan.required_edge_count, 2);
        assert_eq!(trace_plan.records[0].routine, JitCodeId(1));
        trace_proof
    }

    pub(super) fn vm_root_gather_proof(
        receipt: &HeapConservativeScanAppendReceipt,
    ) -> VmRootGatherPlan {
        let root = receipt.append_plan.records[0].root;
        VmRootGatherDescriptor {
            heap: receipt.heap,
            marking_epoch: receipt.epoch,
            world_stopped: true,
            jit_enabled: true,
            scratch_buffers: vec![VmScratchBufferDescriptor {
                id: VmScratchBufferId(1),
                data_begin: 0x8000,
                byte_length: 4 * ENCODED_JS_VALUE_BYTES,
                active_length: ENCODED_JS_VALUE_BYTES,
                candidate_slots: vec![VmScratchBufferCandidateSlot {
                    offset: 0,
                    candidate_address: root.candidate_address,
                }],
            }],
            checkpoint_side_states: Vec::new(),
            validated_cells: vec![root],
        }
        .gather_vm_roots()
        .expect("VM root gather proof")
    }

    pub(super) fn verifier_append_proof(
        collector_effects_proof: &P6Arm64SlotVisitorCollectorEffectsProof,
    ) -> P6Arm64VerifierSlotVisitorConservativeRootAppendProof {
        P6Arm64VerifierSlotVisitorConservativeRootAppendProof::no_verifier_slot_visitor_from_collector_effects_proof(
            collector_effects_proof,
        )
    }

    fn align_down_to_word(address: usize) -> usize {
        let mask = core::mem::size_of::<usize>() - 1;
        address & !mask
    }

    fn align_up_to_word(address: usize) -> usize {
        let mask = core::mem::size_of::<usize>() - 1;
        (address + mask) & !mask
    }

    fn stack_span_covering_words(first: usize, second: usize) -> ConservativeRootSpan {
        let begin = align_down_to_word(first.min(second));
        let end = align_up_to_word(first.max(second) + core::mem::size_of::<usize>());
        ConservativeRootSpan { begin, end }
    }

    fn machine_stack_roots_for_residency(
        register_span: ConservativeRootSpan,
        stack_span: ConservativeRootSpan,
        root: ConservativeRootCell,
    ) -> ConservativeRoots {
        let mut roots = ConservativeRoots::new();
        roots.add_span(register_span);
        roots.add_span(stack_span);
        roots.add_validated_cell(root);
        roots
    }

    pub(super) fn machine_stack_conservative_rooting_proof_from_marker(
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
        receipt: &HeapConservativeScanAppendReceipt,
    ) -> P6Arm64MachineStackConservativeRootingProof {
        let top_frame_address = top_call_frame_publication.publication.published_top_frame.0;
        let slot_address = top_frame_address + core::mem::size_of::<usize>();
        let root = receipt.append_plan.records[0].root;
        let stack_span = stack_span_covering_words(top_frame_address, slot_address);
        let marker = JscMachineStackMarker::new();
        let heap_state = HeapStateDescriptor {
            phase: receipt.phase,
            mutator_state: receipt.mutator_state,
            conductor: GcConductor::Mutator,
            ..HeapStateDescriptor::default()
        };

        marker
            .with_synthetic_current_thread_conservative_roots_for_testing(
                receipt.heap,
                receipt.epoch,
                heap_state,
                stack_span,
                |machine_stack_proof| {
                    let mut machine_stack_roots = ConservativeRoots::new();
                    for span in machine_stack_proof.spans() {
                        machine_stack_roots.add_span(span.span);
                    }
                    machine_stack_roots.add_validated_cell(root);

                    P6Arm64MachineStackConservativeRootingProof::from_machine_stack_proof(
                        &top_call_frame_publication,
                        &machine_stack_proof,
                        machine_stack_roots,
                        receipt.clone(),
                        P6Arm64NativeFrameMachineStackSpanKind::Stack,
                    )
                },
            )
            .expect("synthetic current-thread machine-stack proof")
    }

    pub(super) fn native_frame_machine_stack_residency_proof_from_marker(
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
        receipt: &HeapConservativeScanAppendReceipt,
    ) -> P6Arm64NativeFrameMachineStackResidencyProof {
        let top_frame_address = top_call_frame_publication.publication.published_top_frame.0;
        let slot_address = top_frame_address + core::mem::size_of::<usize>();
        let root = receipt.append_plan.records[0].root;
        let stack_span = stack_span_covering_words(top_frame_address, slot_address);
        let marker = JscMachineStackMarker::new();
        let heap_state = HeapStateDescriptor {
            phase: receipt.phase,
            mutator_state: receipt.mutator_state,
            conductor: GcConductor::Mutator,
            ..HeapStateDescriptor::default()
        };

        marker
            .with_synthetic_current_thread_conservative_roots_for_testing(
                receipt.heap,
                receipt.epoch,
                heap_state,
                stack_span,
                |machine_stack_proof| {
                    let mut machine_stack_roots = ConservativeRoots::new();
                    for span in machine_stack_proof.spans() {
                        machine_stack_roots.add_span(span.span);
                    }
                    machine_stack_roots.add_validated_cell(root);

                    P6Arm64NativeFrameMachineStackResidencyProof::from_machine_stack_proof(
                        &top_call_frame_publication,
                        &machine_stack_proof,
                        machine_stack_roots,
                        P6Arm64NativeFrameMachineStackSpanKind::Stack,
                        vec![P6Arm64NativeRootSlotRecord {
                            kind: P6Arm64NativeRootSlotKind::ThisValue,
                            slot_address,
                            encoded_payload: root.candidate_address,
                            expected_root: root,
                            containing_span: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                        }],
                    )
                },
            )
            .expect("synthetic current-thread machine-stack proof")
    }

    pub(super) fn native_frame_root_slot_records_for_publication(
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
        receipt: &HeapConservativeScanAppendReceipt,
    ) -> Vec<P6Arm64NativeRootSlotRecord> {
        let top_frame_address = top_call_frame_publication.publication.published_top_frame.0;
        let slot_address = top_frame_address + core::mem::size_of::<usize>();
        let root = receipt.append_plan.records[0].root;
        vec![P6Arm64NativeRootSlotRecord {
            kind: P6Arm64NativeRootSlotKind::ThisValue,
            slot_address,
            encoded_payload: root.candidate_address,
            expected_root: root,
            containing_span: P6Arm64NativeFrameMachineStackSpanKind::Stack,
        }]
    }

    pub(super) fn verified_native_frame_machine_stack_residency_proof_from_rooting<'publication>(
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<
            'publication,
        >,
        machine_stack_conservative_rooting_proof: &P6Arm64MachineStackConservativeRootingProof,
        receipt: &HeapConservativeScanAppendReceipt,
    ) -> Result<
        P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'publication>,
        P6Arm64VerifiedNativeFrameMachineStackResidencyProofError,
    > {
        P6Arm64VerifiedNativeFrameMachineStackResidencyProof::from_machine_stack_conservative_rooting_proof(
            &top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            native_frame_root_slot_records_for_publication(top_call_frame_publication, receipt),
        )
    }

    pub(super) fn full_machine_stack_residency_fallback<'publication>(
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<
            'publication,
        >,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        native_frame_residency_proof: P6Arm64VerifiedNativeFrameMachineStackResidencyProof<
            'publication,
        >,
    ) -> P6Arm64BranchAwareCallableFallbackRootingProof<'publication> {
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceAndMachineStackResidencyProof {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
            verifier_append_proof,
            jit_stub_trace_plan,
            native_frame_residency_proof,
        }
    }

    pub(super) fn verified_generated_native_frame_materialization_proof_from_stack_call<
        'publication,
    >(
        fixture: &NativeFrameResidencyFixture<'publication>,
        stack_call_proof: &Arm64NativeEntryJscStackCallRequestProof,
        expected_live_local_slots: usize,
    ) -> Result<
        P6Arm64VerifiedGeneratedNativeFrameMaterializationProof<'publication>,
        P6Arm64VerifiedGeneratedNativeFrameMaterializationProofError,
    > {
        P6Arm64VerifiedGeneratedNativeFrameMaterializationProof::from_jsc_stack_call_request_proof(
            &fixture.top_call_frame_publication,
            &fixture.machine_stack_conservative_rooting_proof,
            fixture.native_frame_residency_proof.clone(),
            stack_call_proof,
            expected_live_local_slots,
        )
    }

    pub(super) fn full_generated_native_frame_materialization_fallback<'publication>(
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<
            'publication,
        >,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        generated_native_frame_materialization_proof:
            P6Arm64VerifiedGeneratedNativeFrameMaterializationProof<'publication>,
    ) -> P6Arm64BranchAwareCallableFallbackRootingProof<'publication> {
        P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceMachineStackResidencyAndGeneratedNativeFrameMaterializationProof {
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
            verifier_append_proof,
            jit_stub_trace_plan,
            generated_native_frame_materialization_proof,
        }
    }

    #[derive(Clone)]
    pub(super) struct NativeFrameResidencyFixture<'publication> {
        pub(super) top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        pub(super) conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        pub(super) machine_stack_conservative_rooting_proof:
            P6Arm64MachineStackConservativeRootingProof,
        pub(super) vm_root_gather_plan: VmRootGatherPlan,
        pub(super) conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        pub(super) collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        pub(super) verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        pub(super) jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        pub(super) native_frame_residency_descriptor: P6Arm64NativeFrameMachineStackResidencyProof,
        pub(super) native_frame_residency_proof:
            P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'publication>,
    }

    impl<'publication> NativeFrameResidencyFixture<'publication> {
        pub(super) fn fallback(
            &self,
        ) -> P6Arm64BranchAwareCallableFallbackRootingProof<'publication> {
            full_machine_stack_residency_fallback(
                self.top_call_frame_publication,
                self.machine_stack_conservative_rooting_proof.clone(),
                self.vm_root_gather_plan.clone(),
                self.conservative_root_marking_plan.clone(),
                self.collector_effects_plan.clone(),
                self.verifier_append_proof.clone(),
                self.jit_stub_trace_plan.clone(),
                self.native_frame_residency_proof.clone(),
            )
        }
    }

    pub(super) fn native_frame_residency_fixture_for_publication<'publication>(
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof<
            'publication,
        >,
    ) -> NativeFrameResidencyFixture<'publication> {
        let (
            conservative_scan_append_receipt,
            conservative_root_marking_plan,
            collector_effects_plan,
        ) = conservative_root_marking_and_collector_effects_proof();
        let vm_root_gather_plan = vm_root_gather_proof(&conservative_scan_append_receipt);
        let verifier_append_proof = verifier_append_proof(&collector_effects_plan);
        let jit_stub_trace_plan =
            jit_stub_trace_proof(&collector_effects_plan, &verifier_append_proof);
        let machine_stack_conservative_rooting_proof =
            machine_stack_conservative_rooting_proof_from_marker(
                top_call_frame_publication,
                &conservative_scan_append_receipt,
            );
        let native_frame_residency_proof = native_frame_machine_stack_residency_proof_from_marker(
            top_call_frame_publication,
            &conservative_scan_append_receipt,
        );
        let verified_native_frame_residency_proof =
            verified_native_frame_machine_stack_residency_proof_from_rooting(
                top_call_frame_publication,
                &machine_stack_conservative_rooting_proof,
                &conservative_scan_append_receipt,
            )
            .expect("fixture should verify native frame machine-stack residency");

        NativeFrameResidencyFixture {
            top_call_frame_publication,
            conservative_scan_append_receipt,
            machine_stack_conservative_rooting_proof,
            vm_root_gather_plan,
            conservative_root_marking_plan,
            collector_effects_plan,
            verifier_append_proof,
            jit_stub_trace_plan,
            native_frame_residency_descriptor: native_frame_residency_proof,
            native_frame_residency_proof: verified_native_frame_residency_proof,
        }
    }

    pub(super) fn with_native_frame_residency_fixture<R>(
        body: impl for<'publication> FnOnce(NativeFrameResidencyFixture<'publication>) -> R,
    ) -> R {
        with_stack_top_call_frame_publication(|top_call_frame_publication| {
            body(native_frame_residency_fixture_for_publication(
                top_call_frame_publication,
            ))
        })
    }

    fn assert_native_frame_residency_descriptor_rejection(
        fixture: NativeFrameResidencyFixture<'_>,
        expected_mismatch: P6Arm64NativeFrameMachineStackResidencyProofMismatch,
    ) {
        assert_eq!(
            validate_p6_arm64_native_frame_machine_stack_residency_proof(
                &fixture.top_call_frame_publication,
                &fixture.conservative_scan_append_receipt,
                &fixture.native_frame_residency_descriptor,
            ),
            Err(expected_mismatch)
        );
    }

    fn admission_for_site(
        code_block: &CodeBlock,
        site: &P6X86_64CallableSideExitReturnSite,
    ) -> Result<Infallible, P6Arm64BranchAwareCallableAdmissionRejection<'static>> {
        admission_for_site_with_labels(
            code_block,
            site,
            Some(CoreOpcode::JumpIfFalse),
            bci(4),
            bci(2),
        )
    }

    fn admission_for_site_with_labels(
        code_block: &CodeBlock,
        site: &P6X86_64CallableSideExitReturnSite,
        opcode: Option<CoreOpcode>,
        target_bytecode_index: BytecodeIndex,
        fallthrough_bytecode_index: BytecodeIndex,
    ) -> Result<Infallible, P6Arm64BranchAwareCallableAdmissionRejection<'static>> {
        let side_exits = [branch_aware_side_exit_proof_with_labels(
            code_block,
            site,
            opcode,
            target_bytecode_index,
            fallthrough_bytecode_index,
        )];
        let request = valid_request(&side_exits);
        p6_arm64_public_branch_aware_callable_admission_proof(&request)
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_missing_top_call_frame_publication_proof() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let request = valid_request(&side_exits);

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingTopCallFramePublicationProof)
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_stale_stack_previous_top_before_mutation() {
        let previous_top_call_frame = Some(FrameAddress(0x1230));
        with_arm64_native_entry_stack_frame::<2, 0>(
            stack_frame_request(previous_top_call_frame, None),
            |stack_frame| {
                let mut state = VmEntryState::default();
                let error = match enter_arm64_native_entry_stack_publication(
                    &mut state,
                    &stack_frame,
                    EntryKind::Script,
                    HeapId::default(),
                ) {
                    Ok(_) => panic!("stale VMEntryRecord previous top-call frame must reject"),
                    Err(error) => error,
                };
                assert_eq!(
                    error,
                    Arm64NativeEntryStackPublicationError::PreviousTopCallFrameMismatch {
                        expected: None,
                        actual: previous_top_call_frame,
                    }
                );
                assert!(state.stack_entry_publications().is_empty());
                assert!(state.stack_entry_publication_exits().is_empty());
            },
        )
        .expect("stack frame fixture");
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_missing_conservative_scan_append_after_publication(
    ) {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        with_stack_top_call_frame_publication(|top_call_frame_publication| {
            let mut request = valid_request(&side_exits);
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
    fn public_arm64_branch_aware_admission_rejects_stack_top_frame_without_machine_stack_residency()
    {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        with_native_frame_residency_fixture(|fixture| {
            let mut request = valid_request(&side_exits);

            request.fallback_rooting_proof =
                P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierAppendAndJitStubTracePlan {
                    top_call_frame_publication: fixture.top_call_frame_publication,
                    machine_stack_conservative_rooting_proof: fixture
                        .machine_stack_conservative_rooting_proof
                        .clone(),
                    vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
                    conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
                    collector_effects_plan: fixture.collector_effects_plan.clone(),
                    verifier_append_proof: fixture.verifier_append_proof.clone(),
                    jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
                };

            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeFrameMachineStackResidencyProof {
                        top_call_frame_publication: fixture.top_call_frame_publication,
                        conservative_scan_append_receipt: fixture.conservative_scan_append_receipt,
                        vm_root_gather_plan: fixture.vm_root_gather_plan,
                        conservative_root_marking_plan: fixture.conservative_root_marking_plan,
                        collector_effects_plan: fixture.collector_effects_plan,
                        verifier_append_proof: fixture.verifier_append_proof,
                        jit_stub_trace_plan: fixture.jit_stub_trace_plan,
                    }
                )
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_top_frame_outside_scanned_machine_stack_spans() {
        with_native_frame_residency_fixture(|mut fixture| {
            let register_span = fixture
                .native_frame_residency_descriptor
                .machine_stack_spans[0]
                .span;
            let stack_span = ConservativeRootSpan {
                begin: 0x2000,
                end: 0x2100,
            };
            let root = fixture.native_frame_residency_descriptor.slot_records[0].expected_root;
            fixture
                .native_frame_residency_descriptor
                .machine_stack_spans[1]
                .span = stack_span;
            fixture
                .native_frame_residency_descriptor
                .machine_stack_roots =
                machine_stack_roots_for_residency(register_span, stack_span, root);

            assert_native_frame_residency_descriptor_rejection(
                fixture.clone(),
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::TopCallFrameOutsideScannedSpans {
                    address: fixture
                        .top_call_frame_publication
                        .publication
                        .published_top_frame,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_extra_machine_stack_residency_spans() {
        with_native_frame_residency_fixture(|mut fixture| {
            let extra_span = ConservativeRootSpan {
                begin: 0x3000,
                end: 0x3100,
            };
            fixture
                .native_frame_residency_descriptor
                .machine_stack_spans
                .push(P6Arm64NativeFrameMachineStackSpanRecord {
                    kind: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                    span: extra_span,
                });
            fixture
                .native_frame_residency_descriptor
                .machine_stack_roots
                .add_span(extra_span);

            assert_native_frame_residency_descriptor_rejection(
                fixture,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::CurrentThreadSpanOrderMismatch {
                    observed: vec![
                        P6Arm64NativeFrameMachineStackSpanKind::RegisterState,
                        P6Arm64NativeFrameMachineStackSpanKind::Stack,
                        P6Arm64NativeFrameMachineStackSpanKind::Stack,
                    ],
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_top_frame_in_register_state_span() {
        with_native_frame_residency_fixture(|mut fixture| {
            let top_frame_address = fixture
                .top_call_frame_publication
                .publication
                .published_top_frame
                .0;
            let register_span = stack_span_covering_words(
                top_frame_address,
                top_frame_address + core::mem::size_of::<usize>(),
            );
            let stack_span = fixture
                .native_frame_residency_descriptor
                .machine_stack_spans[1]
                .span;
            let root = fixture.native_frame_residency_descriptor.slot_records[0].expected_root;
            fixture
                .native_frame_residency_descriptor
                .machine_stack_spans[0]
                .span = register_span;
            fixture
                .native_frame_residency_descriptor
                .machine_stack_roots =
                machine_stack_roots_for_residency(register_span, stack_span, root);

            assert_native_frame_residency_descriptor_rejection(
                fixture,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::TopCallFrameContainingSpanMismatch {
                    expected: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                    actual: P6Arm64NativeFrameMachineStackSpanKind::RegisterState,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_mismatched_machine_stack_root_spans() {
        with_native_frame_residency_fixture(|mut fixture| {
            let extra_span = ConservativeRootSpan {
                begin: 0x4000,
                end: 0x4100,
            };
            fixture
                .native_frame_residency_descriptor
                .machine_stack_roots
                .add_span(extra_span);
            let expected = fixture
                .native_frame_residency_descriptor
                .machine_stack_spans
                .iter()
                .map(|record| record.span)
                .collect();
            let actual = fixture
                .native_frame_residency_descriptor
                .machine_stack_roots
                .spans()
                .to_vec();

            assert_native_frame_residency_descriptor_rejection(
                fixture,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::MachineStackRootSpanMismatch {
                    expected,
                    actual,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_slot_address_outside_scanned_spans() {
        with_native_frame_residency_fixture(|mut fixture| {
            let slot_address = fixture
                .native_frame_residency_descriptor
                .machine_stack_spans[1]
                .span
                .end
                + core::mem::size_of::<usize>();
            fixture.native_frame_residency_descriptor.slot_records[0].slot_address = slot_address;

            assert_native_frame_residency_descriptor_rejection(
                fixture,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotAddressOutsideScannedSpans {
                    order: 0,
                    slot_address,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_unaligned_native_root_slot_address() {
        with_native_frame_residency_fixture(|mut fixture| {
            let slot_address = fixture
                .native_frame_residency_descriptor
                .machine_stack_spans[1]
                .span
                .begin
                + 1;
            fixture.native_frame_residency_descriptor.slot_records[0].slot_address = slot_address;

            assert_native_frame_residency_descriptor_rejection(
                fixture,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotAddressUnaligned {
                    order: 0,
                    slot_address,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_slot_payload_absent_from_machine_stack_roots() {
        with_native_frame_residency_fixture(|mut fixture| {
            let root = fixture.native_frame_residency_descriptor.slot_records[0].expected_root;
            let mut roots = ConservativeRoots::new();
            for span in &fixture
                .native_frame_residency_descriptor
                .machine_stack_spans
            {
                roots.add_span(span.span);
            }
            fixture
                .native_frame_residency_descriptor
                .machine_stack_roots = roots;

            assert_native_frame_residency_descriptor_rejection(
                fixture,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotRootAbsentFromMachineStackRoots {
                    order: 0,
                    root,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_slot_payload_absent_from_append_receipt() {
        with_native_frame_residency_fixture(|mut fixture| {
            let fake_root = ConservativeRootCell {
                candidate_address: 0x9_0000,
                cell: CellId(0xdead),
            };
            fixture.native_frame_residency_descriptor.slot_records[0].encoded_payload =
                fake_root.candidate_address;
            fixture.native_frame_residency_descriptor.slot_records[0].expected_root = fake_root;
            let mut roots = ConservativeRoots::new();
            for span in &fixture
                .native_frame_residency_descriptor
                .machine_stack_spans
            {
                roots.add_span(span.span);
            }
            roots.add_validated_cell(fake_root);
            fixture
                .native_frame_residency_descriptor
                .machine_stack_roots = roots;

            assert_native_frame_residency_descriptor_rejection(
                fixture,
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotRootAbsentFromConservativeScanAppendReceipt {
                    order: 0,
                    root: fake_root,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_stale_machine_stack_residency_state() {
        with_native_frame_residency_fixture(|mut heap_mismatch| {
            heap_mismatch.native_frame_residency_descriptor.heap =
                HeapId(heap_mismatch.conservative_scan_append_receipt.heap.0 + 1);
            assert_native_frame_residency_descriptor_rejection(
                heap_mismatch.clone(),
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::HeapMismatch {
                    receipt: heap_mismatch.conservative_scan_append_receipt.heap,
                    machine_stack: heap_mismatch.native_frame_residency_descriptor.heap,
                },
            );
        });

        with_native_frame_residency_fixture(|mut epoch_mismatch| {
            epoch_mismatch
                .native_frame_residency_descriptor
                .marking_epoch =
                HeapEpoch(epoch_mismatch.conservative_scan_append_receipt.epoch.0 + 1);
            assert_native_frame_residency_descriptor_rejection(
                epoch_mismatch.clone(),
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::MarkingEpochMismatch {
                    receipt: epoch_mismatch.conservative_scan_append_receipt.epoch,
                    machine_stack: epoch_mismatch
                        .native_frame_residency_descriptor
                        .marking_epoch,
                },
            );
        });

        with_native_frame_residency_fixture(|mut phase_mismatch| {
            phase_mismatch.native_frame_residency_descriptor.phase = GcPhase::NotRunning;
            assert_native_frame_residency_descriptor_rejection(
                phase_mismatch.clone(),
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::PhaseMismatch {
                    receipt: phase_mismatch.conservative_scan_append_receipt.phase,
                    machine_stack: GcPhase::NotRunning,
                },
            );
        });

        with_native_frame_residency_fixture(|mut mutator_mismatch| {
            mutator_mismatch
                .native_frame_residency_descriptor
                .mutator_state = MutatorState::Running;
            assert_native_frame_residency_descriptor_rejection(
                mutator_mismatch.clone(),
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::MutatorStateMismatch {
                    receipt: mutator_mismatch
                        .conservative_scan_append_receipt
                        .mutator_state,
                    machine_stack: MutatorState::Running,
                },
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_inconsistent_vm_root_gather_proof() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        with_stack_top_call_frame_publication(|top_call_frame_publication| {
            let mut request = valid_request(&side_exits);
            let (conservative_scan_append_receipt, _, _) =
                conservative_root_marking_and_collector_effects_proof();
            let valid_vm_root_gather_plan = vm_root_gather_proof(&conservative_scan_append_receipt);
            let machine_stack_conservative_rooting_proof =
                machine_stack_conservative_rooting_proof_from_marker(
                    top_call_frame_publication,
                    &conservative_scan_append_receipt,
                );

            let mut heap_mismatch_plan = valid_vm_root_gather_plan.clone();
            heap_mismatch_plan.heap = HeapId(conservative_scan_append_receipt.heap.0 + 1);
            request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
                vm_root_gather_plan: heap_mismatch_plan.clone(),
            };
            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                        top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: heap_mismatch_plan,
                        mismatch: P6Arm64VmRootGatherProofMismatch::HeapMismatch {
                            receipt: conservative_scan_append_receipt.heap,
                            vm_roots: HeapId(conservative_scan_append_receipt.heap.0 + 1),
                        },
                    }
                )
            );

            let mut epoch_mismatch_plan = valid_vm_root_gather_plan.clone();
            epoch_mismatch_plan.marking_epoch =
                HeapEpoch(conservative_scan_append_receipt.epoch.0 + 1);
            request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof: machine_stack_conservative_rooting_proof.clone(),
                vm_root_gather_plan: epoch_mismatch_plan.clone(),
            };
            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                        top_call_frame_publication,
                        conservative_scan_append_receipt: conservative_scan_append_receipt.clone(),
                        vm_root_gather_plan: epoch_mismatch_plan,
                        mismatch: P6Arm64VmRootGatherProofMismatch::MarkingEpochMismatch {
                            receipt: conservative_scan_append_receipt.epoch,
                            vm_roots: HeapEpoch(conservative_scan_append_receipt.epoch.0 + 1),
                        },
                    }
                )
            );

            let mut source_mismatch_plan = valid_vm_root_gather_plan;
            source_mismatch_plan.scratch_buffer_records[0].source =
                VmRootSource::CheckpointOsrExitSideState;
            request.fallback_rooting_proof =
            P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherPlan {
                top_call_frame_publication,
                machine_stack_conservative_rooting_proof,
                vm_root_gather_plan: source_mismatch_plan.clone(),
            };
            assert_eq!(
                p6_arm64_public_branch_aware_callable_admission_proof(&request),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::VmRootGatherProofMismatch {
                        top_call_frame_publication,
                        conservative_scan_append_receipt,
                        vm_root_gather_plan: source_mismatch_plan,
                        mismatch: P6Arm64VmRootGatherProofMismatch::GatherPlanMismatch(
                            VmRootGatherError::ScratchBufferSourceMismatch {
                                order: 0,
                                actual: VmRootSource::CheckpointOsrExitSideState,
                            },
                        ),
                    }
                )
            );
        });
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_x86_callable_kind() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let mut request = valid_request(&side_exits);
        request.callable_kind = BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry;

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::CallableKindNotArm64 {
                    actual: BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry,
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_non_branch_aware_terminal_policy() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let mut request = valid_request(&side_exits);
        request.terminal_policy = Some(
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs,
        );

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NonBranchAwareTerminalPolicy {
                    actual: P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs,
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_requires_target_and_fallthrough_reentry_ranges() {
        let code_block = jump_if_false_code_block(4);
        let mut site = jump_if_false_site();
        site.native_reentry_targets = vec![target(bci(4), 12), target(bci(2), 64)];
        let side_exits = [branch_aware_side_exit_proof(&code_block, &site)];
        let request = valid_request(&side_exits);

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::NativeReentryTargetOutsideDescriptorRange {
                    side_exit_index: 0,
                    resume_bytecode_index: bci(2),
                    resume_entry_offset: 64,
                    range: range(),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_legacy_single_target_shape() {
        let code_block = jump_if_false_code_block(4);
        let mut site = jump_if_false_site();
        site.resume_bytecode_index = Some(bci(2));
        site.resume_entry_offset = Some(28);

        assert_eq!(
            admission_for_site(&code_block, &site),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_missing_extra_or_duplicate_reentry_labels() {
        let code_block = jump_if_false_code_block(4);

        for native_reentry_targets in [
            vec![target(bci(4), 12)],
            vec![target(bci(4), 12), target(bci(2), 28), target(bci(8), 36)],
            vec![target(bci(4), 12), target(bci(4), 36)],
            vec![target(bci(4), 12), target(bci(8), 36)],
        ] {
            let mut site = jump_if_false_site();
            site.native_reentry_targets = native_reentry_targets;

            assert_eq!(
                admission_for_site(&code_block, &site),
                Err(
                    P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                        side_exit_index: 0,
                        reason:
                            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                        opcode: Some(CoreOpcode::JumpIfFalse),
                    }
                )
            );
        }
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_degenerate_or_invalid_decoded_labels() {
        let degenerate_code_block = jump_if_false_code_block(2);
        let mut degenerate_site = jump_if_false_site();
        degenerate_site.native_reentry_targets = vec![target(bci(2), 28), target(bci(2), 36)];

        assert_eq!(
            admission_for_site(&degenerate_code_block, &degenerate_site),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );

        let invalid_target_code_block = jump_if_false_code_block(99);
        let mut invalid_target_site = jump_if_false_site();
        invalid_target_site.native_reentry_targets = vec![target(bci(99), 12), target(bci(2), 28)];

        assert_eq!(
            admission_for_site_with_labels(
                &invalid_target_code_block,
                &invalid_target_site,
                Some(CoreOpcode::JumpIfFalse),
                bci(99),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );

        let terminal_code_block = terminal_jump_if_false_code_block();
        let mut missing_fallthrough_site = jump_if_false_site();
        missing_fallthrough_site.native_reentry_targets =
            vec![target(bci(0), 12), target(bci(2), 28)];

        assert_eq!(
            admission_for_site_with_labels(
                &terminal_code_block,
                &missing_fallthrough_site,
                Some(CoreOpcode::JumpIfFalse),
                bci(0),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_wrong_reason_or_opcode() {
        let code_block = jump_if_false_code_block(4);
        let mut wrong_reason = jump_if_false_site();
        wrong_reason.reason = P6X86_64BaselineSelectedSideExitReason::NonInt32Operand;

        assert_eq!(
            admission_for_site(&code_block, &wrong_reason),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
                    opcode: Some(CoreOpcode::JumpIfFalse),
                }
            )
        );

        let site = jump_if_false_site();
        assert_eq!(
            admission_for_site_with_labels(
                &code_block,
                &site,
                Some(CoreOpcode::AddInt32),
                bci(4),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::UnexpectedSideExit {
                    side_exit_index: 0,
                    reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                    opcode: Some(CoreOpcode::AddInt32),
                }
            )
        );
    }

    #[test]
    fn public_arm64_branch_aware_admission_rejects_proof_label_mismatches() {
        let code_block = jump_if_false_code_block(4);
        let site = jump_if_false_site();

        assert_eq!(
            admission_for_site_with_labels(
                &code_block,
                &site,
                Some(CoreOpcode::JumpIfFalse),
                bci(8),
                bci(2),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                    side_exit_index: 0,
                    resume_bytecode_index: bci(8),
                }
            )
        );

        assert_eq!(
            admission_for_site_with_labels(
                &code_block,
                &site,
                Some(CoreOpcode::JumpIfFalse),
                bci(4),
                bci(8),
            ),
            Err(
                P6Arm64BranchAwareCallableAdmissionRejection::MissingNativeReentryTarget {
                    side_exit_index: 0,
                    resume_bytecode_index: bci(8),
                }
            )
        );
    }
}
