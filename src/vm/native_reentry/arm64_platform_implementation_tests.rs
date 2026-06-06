use super::super::arm64_exception_exit_routing::P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof;
use super::super::arm64_platform_implementation::{
    validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_descriptor,
    P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch,
    P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProof,
    P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofError,
    P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch,
};
use super::super::arm64_public_dispatch::P6Arm64PublicJscStackDispatchPlatformRequestField;
use super::frame_materialization_tests::{
    verified_exception_exit_routing_proof_from_public_dispatch_preconditions,
    verified_exception_unwind_restoration_proof_from_normal_return,
    verified_public_dispatch_preconditions_proof_from_exception_unwind,
};
use super::tests;
use super::*;
use crate::jit::{ExecutableAllocationId, MachineCodeOwnership};
use crate::platform::executable_memory_compartment::{
    ExecutableMemoryArm64JscStackCallRequest,
    ExecutableMemoryArm64JscStackDispatchArm64eGatePolicy,
    ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
    ExecutableMemoryArm64JscStackDispatchImplementationKind, ExecutableMemoryCompartment,
    ExecutableMemoryCompartmentRequest,
};

fn with_exception_exit_routing_proof<R>(
    body: impl for<'publication> FnOnce(
        tests::NativeFrameResidencyFixture<'publication>,
        P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'publication>,
    ) -> R,
) -> R {
    tests::with_stack_top_call_frame_publication_stack_call_frame_and_exit(
        |top_call_frame_publication, stack_call_proof, stack_frame_proof, exit_record| {
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
            let normal_return_restoration_proof =
                tests::verified_vm_entry_normal_return_restoration_proof_from_dispatch(
                    &fixture,
                    jsc_stack_dispatch_request_proof,
                    exit_record,
                )
                .expect("VM-entry normal return restoration proof should verify");
            let exception_unwind_restoration_proof =
                verified_exception_unwind_restoration_proof_from_normal_return(
                    normal_return_restoration_proof,
                )
                .expect("VM-entry exception/unwind restoration proof should verify");
            let public_jsc_stack_dispatch_preconditions_proof =
                verified_public_dispatch_preconditions_proof_from_exception_unwind(
                    exception_unwind_restoration_proof,
                )
                .expect("public JSC stack-dispatch preconditions should verify");
            let exception_exit_routing_proof =
                verified_exception_exit_routing_proof_from_public_dispatch_preconditions(
                    public_jsc_stack_dispatch_preconditions_proof,
                )
                .expect("exception-exit routing proof should verify");

            body(fixture, exception_exit_routing_proof)
        },
    )
}

#[cfg(unix)]
fn platform_compartment_for_request(
    request: ExecutableMemoryArm64JscStackCallRequest,
) -> ExecutableMemoryCompartment {
    let byte_len = request
        .entry_offset
        .checked_add(4)
        .expect("test entry range should fit u32");
    let bytes = vec![0; byte_len as usize];
    let mut compartment =
        ExecutableMemoryCompartment::allocate(ExecutableMemoryCompartmentRequest::new(
            ExecutableAllocationId(17),
            MachineCodeOwnership::SharedStub,
            byte_len,
        ))
        .expect("test compartment allocation");
    compartment
        .copy_from_slice(compartment.machine_range(), &bytes)
        .expect("test byte copy");
    compartment.protect_executable().expect("test RX protect");
    compartment
}

#[cfg(all(unix, target_arch = "aarch64"))]
fn current_platform_descriptor(
    request: ExecutableMemoryArm64JscStackCallRequest,
) -> ExecutableMemoryArm64JscStackDispatchImplementationDescriptor {
    platform_compartment_for_request(request)
        .arm64_jsc_stack_dispatch_implementation_descriptor(request)
        .expect("current platform descriptor")
}

#[cfg(unix)]
fn full_platform_descriptor_for_testing(
    request: ExecutableMemoryArm64JscStackCallRequest,
) -> ExecutableMemoryArm64JscStackDispatchImplementationDescriptor {
    platform_compartment_for_request(request)
        .arm64_jsc_stack_dispatch_full_do_vm_entry_descriptor_for_testing(request)
        .expect("test full platform descriptor")
}

fn platform_request_from_routing_proof(
    routing_proof: &P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'_>,
) -> ExecutableMemoryArm64JscStackCallRequest {
    routing_proof
        .public_jsc_stack_dispatch_preconditions_proof()
        .platform_envelope()
        .platform_request
}

#[cfg(unix)]
fn assert_platform_descriptor_linkage_error(
    mutate: impl FnOnce(
        ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
    ) -> ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
    expected: P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch,
) {
    with_exception_exit_routing_proof(|fixture, routing_proof| {
        let descriptor = full_platform_descriptor_for_testing(platform_request_from_routing_proof(
            &routing_proof,
        ));
        assert_eq!(
            validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_descriptor(
                &fixture.top_call_frame_publication,
                &routing_proof,
                mutate(descriptor),
                1,
            ),
            Err(
                P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch::Linkage(
                    expected,
                )
            )
        );
    });
}

#[cfg(all(unix, target_arch = "aarch64"))]
#[test]
fn arm64_platform_implementation_rejects_current_normal_return_only_descriptor() {
    with_exception_exit_routing_proof(|_, routing_proof| {
        let descriptor =
            current_platform_descriptor(platform_request_from_routing_proof(&routing_proof));

        assert_eq!(
            P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProof::from_platform_implementation_descriptor(
                routing_proof,
                descriptor,
            ),
            Err(P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofError::Linkage(
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::ImplementationKindUnsupported {
                    actual:
                        ExecutableMemoryArm64JscStackDispatchImplementationKind::NormalReturnOnlyPrivateTrampoline,
                },
            ))
        );
    });
}

#[cfg(unix)]
#[test]
fn arm64_platform_implementation_rejects_platform_request_drift() {
    with_exception_exit_routing_proof(|fixture, routing_proof| {
        let request = platform_request_from_routing_proof(&routing_proof);
        let descriptor = full_platform_descriptor_for_testing(request);
        let drifted_request = ExecutableMemoryArm64JscStackCallRequest {
            entry_offset: request.entry_offset + 1,
            ..request
        };

        assert_eq!(
            validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_descriptor(
                &fixture.top_call_frame_publication,
                &routing_proof,
                descriptor.with_platform_request_for_testing(drifted_request),
                1,
            ),
            Err(P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch::Linkage(
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::PlatformRequestMismatch {
                    field: P6Arm64PublicJscStackDispatchPlatformRequestField::EntryOffset,
                    expected: request.entry_offset as usize,
                    actual: drifted_request.entry_offset as usize,
                },
            ))
        );
    });
}

#[cfg(unix)]
#[test]
fn arm64_platform_implementation_rejects_missing_platform_behavior_flags() {
    with_exception_exit_routing_proof(|fixture, routing_proof| {
        let descriptor = full_platform_descriptor_for_testing(platform_request_from_routing_proof(
            &routing_proof,
        ));
        for (descriptor, expected) in [
            (
                descriptor.with_supports_normal_return_for_testing(false),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::NormalReturnUnsupported,
            ),
            (
                descriptor.with_supports_caught_exception_exit_for_testing(false),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::CaughtExceptionExitUnsupported,
            ),
            (
                descriptor.with_supports_uncaught_exception_exit_for_testing(false),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::UncaughtExceptionExitUnsupported,
            ),
            (
                descriptor.with_constructs_vm_entry_record_frame_for_testing(false),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmEntryRecordFrameNotConstructed,
            ),
            (
                descriptor.with_publishes_vm_top_frame_pair_for_testing(false),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmTopFramePairNotPublished,
            ),
            (
                descriptor.with_restores_vm_top_frame_pair_on_normal_return_for_testing(false),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmTopFramePairNormalReturnNotRestored,
            ),
            (
                descriptor.with_copies_callee_saves_to_vm_entry_record_on_exception_for_testing(
                    false,
                ),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmEntryCalleeSavesNotCopiedForException,
            ),
            (
                descriptor.with_restores_callee_saves_from_vm_entry_record_on_exception_for_testing(
                    false,
                ),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmEntryCalleeSavesNotRestoredForException,
            ),
            (
                descriptor
                    .with_routes_caught_exception_via_target_machine_pc_for_throw_for_testing(
                        false,
                    ),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::CaughtExceptionExitDoesNotUseTargetMachinePcForThrow,
            ),
            (
                descriptor
                    .with_routes_uncaught_exception_via_target_machine_pc_for_throw_for_testing(
                        false,
                    ),
                P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::UncaughtExceptionExitDoesNotUseTargetMachinePcForThrow,
            ),
        ] {
            assert_eq!(
                validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_descriptor(
                    &fixture.top_call_frame_publication,
                    &routing_proof,
                    descriptor,
                    1,
                ),
                Err(P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch::Linkage(
                    expected,
                ))
            );
        }
    });
}

#[cfg(unix)]
#[test]
fn arm64_platform_implementation_rejects_arm64e_gate_claim() {
    assert_platform_descriptor_linkage_error(
        |descriptor| {
            descriptor.with_arm64e_gate_policy_for_testing(
                ExecutableMemoryArm64JscStackDispatchArm64eGatePolicy::Arm64eGateModeled,
            )
        },
        P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::Arm64eGateClaimed {
            policy: ExecutableMemoryArm64JscStackDispatchArm64eGatePolicy::Arm64eGateModeled,
        },
    );
}

#[cfg(all(unix, target_arch = "aarch64"))]
#[test]
fn public_arm64_admission_rejects_current_platform_descriptor_as_mismatch() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    with_exception_exit_routing_proof(|fixture, routing_proof| {
        let descriptor =
            current_platform_descriptor(platform_request_from_routing_proof(&routing_proof));
        let mut request = tests::valid_request(&side_exits);
        request.fallback_rooting_proof = P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceExceptionExitRoutingAndPlatformImplementationDescriptor {
            top_call_frame_publication: fixture.top_call_frame_publication,
            machine_stack_conservative_rooting_proof: fixture
                .machine_stack_conservative_rooting_proof
                .clone(),
            vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
            collector_effects_plan: fixture.collector_effects_plan.clone(),
            verifier_append_proof: fixture.verifier_append_proof.clone(),
            jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
            public_jsc_stack_dispatch_exception_exit_routing_proof: routing_proof,
            platform_implementation_descriptor: descriptor,
        };

        assert_eq!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(P6Arm64BranchAwareCallableAdmissionRejection::Arm64PublicJscStackDispatchPlatformImplementationMismatch {
                mismatch:
                    P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch::Linkage(
                        P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::ImplementationKindUnsupported {
                            actual:
                                ExecutableMemoryArm64JscStackDispatchImplementationKind::NormalReturnOnlyPrivateTrampoline,
                        },
                    ),
            })
        );
    });
}

#[cfg(unix)]
#[test]
fn public_arm64_admission_keeps_platform_authority_missing_after_full_test_descriptor() {
    let code_block = tests::jump_if_false_code_block(4);
    let site = tests::jump_if_false_site();
    let side_exits = [tests::branch_aware_side_exit_proof(&code_block, &site)];
    with_exception_exit_routing_proof(|fixture, routing_proof| {
        let descriptor = full_platform_descriptor_for_testing(platform_request_from_routing_proof(
            &routing_proof,
        ));
        let mut request = tests::valid_request(&side_exits);
        request.fallback_rooting_proof = P6Arm64BranchAwareCallableFallbackRootingProof::TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceExceptionExitRoutingAndPlatformImplementationDescriptor {
            top_call_frame_publication: fixture.top_call_frame_publication,
            machine_stack_conservative_rooting_proof: fixture
                .machine_stack_conservative_rooting_proof
                .clone(),
            vm_root_gather_plan: fixture.vm_root_gather_plan.clone(),
            conservative_root_marking_plan: fixture.conservative_root_marking_plan.clone(),
            collector_effects_plan: fixture.collector_effects_plan.clone(),
            verifier_append_proof: fixture.verifier_append_proof.clone(),
            jit_stub_trace_plan: fixture.jit_stub_trace_plan.clone(),
            public_jsc_stack_dispatch_exception_exit_routing_proof: routing_proof,
            platform_implementation_descriptor: descriptor,
        };

        assert!(matches!(
            p6_arm64_public_branch_aware_callable_admission_proof(&request),
            Err(P6Arm64BranchAwareCallableAdmissionRejection::MissingArm64PublicJscStackDispatchPlatformImplementationAuthority {
                ..
            })
        ));
    });
}
