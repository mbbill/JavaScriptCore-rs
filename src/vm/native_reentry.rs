//! VM native side-exit reentry bridge helpers.
//!
//! C++ JSC maps this responsibility across Baseline JIT operations/thunks
//! (`JITOpcodes.cpp` falsey thunk calls), resume-label metadata
//! (`JIT::fastPathResumePoint` plus `JITCodeMapBuilder`), and
//! `FrameTracers`/`AssemblyHelpers::prepareCallOperation` updating
//! `VM::topCallFrame` for JIT operation rooting. Rust keeps the helper here
//! because `vm::mod` is already oversized; this module only classifies native
//! return payloads and builds opaque executable-memory call requests.

use core::ffi::c_void;
use std::ptr::NonNull;

use crate::interpreter::{ExecutionCompletion, ExecutionError};
use crate::jit::emitter::{
    P10X86_64BaselinePropertyNativeExitReturnPayload,
    P9X86_64BaselineJsCallNativeExitReturnPayload,
    P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
    P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
};
use crate::jit::{
    P14X86_64BaselineLoopBackedgeReturnPayload, P6X86_64BaselineSideExitReturnPayload,
    P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG,
    P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG,
};
use crate::platform::executable_memory_compartment::ExecutableMemoryP6CallRequest;
use crate::runtime::RuntimeValue;
use crate::value::EncodedJsValue;

use super::side_exit::P6CallableSideExitNativeReentryInvocation;
use super::BaselineNativeEntryVmExecution;

#[path = "native_reentry/arm64_admission.rs"]
mod arm64_admission;
#[path = "native_reentry/arm64_exception_unwind.rs"]
mod arm64_exception_unwind;
#[path = "native_reentry/arm64_public_dispatch.rs"]
mod arm64_public_dispatch;
#[path = "native_reentry/arm64_vm_entry_normal_return.rs"]
mod arm64_vm_entry_normal_return;
#[path = "native_reentry/rooting.rs"]
mod rooting;

pub(super) use self::arm64_admission::{
    p6_arm64_public_branch_aware_callable_admission_rejection_for_unemitted_seed_candidate,
    P6Arm64BranchAwareCallableAdmissionRejection,
};

#[cfg(test)]
pub(super) fn p6_x86_64_callable_side_exit_payload_has_reserved_tag(raw_bits: u64) -> bool {
    (raw_bits & 0xff) == u64::from(P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum P6P9P10P14X86_64CallableNativeReturnPayload {
    P6(P6X86_64BaselineSideExitReturnPayload),
    P9(P9X86_64BaselineJsCallNativeExitReturnPayload),
    P10(P10X86_64BaselinePropertyNativeExitReturnPayload),
    P14(P14X86_64BaselineLoopBackedgeReturnPayload),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum P6Arm64EmittedSemanticNativeRawReturn {
    RuntimeValue(RuntimeValue),
    RetainedP6SideExit(P6X86_64BaselineSideExitReturnPayload),
}

pub(super) fn p6_p9_p10_p14_x86_64_callable_native_return_payload(
    raw_bits: u64,
) -> Result<Option<P6P9P10P14X86_64CallableNativeReturnPayload>, ExecutionError> {
    match (raw_bits & 0xff) as u8 {
        P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P6X86_64BaselineSideExitReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P6(
                payload,
            )))
        }
        P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P9X86_64BaselineJsCallNativeExitReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P9(
                payload,
            )))
        }
        P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P10X86_64BaselinePropertyNativeExitReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P10(
                payload,
            )))
        }
        P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG => {
            let payload = P14X86_64BaselineLoopBackedgeReturnPayload::decode(raw_bits)
                .ok_or(ExecutionError::BaselineGeneratedExecutionRejected)?;
            Ok(Some(P6P9P10P14X86_64CallableNativeReturnPayload::P14(
                payload,
            )))
        }
        _ => Ok(None),
    }
}

pub(super) fn p6_arm64_emitted_semantic_native_raw_return(
    raw_bits: u64,
) -> Result<P6Arm64EmittedSemanticNativeRawReturn, ExecutionError> {
    match p6_p9_p10_p14_x86_64_callable_native_return_payload(raw_bits)? {
        Some(P6P9P10P14X86_64CallableNativeReturnPayload::P6(payload)) => Ok(
            P6Arm64EmittedSemanticNativeRawReturn::RetainedP6SideExit(payload),
        ),
        Some(
            P6P9P10P14X86_64CallableNativeReturnPayload::P9(_)
            | P6P9P10P14X86_64CallableNativeReturnPayload::P10(_)
            | P6P9P10P14X86_64CallableNativeReturnPayload::P14(_),
        ) => Err(ExecutionError::BaselineGeneratedExecutionRejected),
        None => Ok(P6Arm64EmittedSemanticNativeRawReturn::RuntimeValue(
            RuntimeValue::from_encoded(EncodedJsValue(raw_bits)),
        )),
    }
}

pub(super) fn p6_arm64_reject_side_exit_reentry_execution(
    execution: BaselineNativeEntryVmExecution,
) -> BaselineNativeEntryVmExecution {
    match execution {
        BaselineNativeEntryVmExecution::P6SideExitReentry(_) => {
            BaselineNativeEntryVmExecution::Native(ExecutionCompletion::Failed(
                ExecutionError::BaselineGeneratedExecutionRejected,
            ))
        }
        execution => execution,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P6NativeSideExitReentryCallBridge {
    reentry: P6CallableSideExitNativeReentryInvocation,
}

impl P6NativeSideExitReentryCallBridge {
    pub(super) const fn new(reentry: P6CallableSideExitNativeReentryInvocation) -> Self {
        Self { reentry }
    }

    pub(super) const fn entry_offset(self) -> u32 {
        self.reentry.entry_offset
    }

    pub(super) const fn call_request(
        self,
        vm: NonNull<c_void>,
        frame_base: NonNull<c_void>,
        callee_value_bits: u64,
        ic_store_base: NonNull<c_void>,
    ) -> ExecutableMemoryP6CallRequest {
        // C++ JSC reenters by branching to a linked native label while
        // `prepareCallOperation`/FrameTracers keep `VM::topCallFrame` coherent
        // for stack walking and rooting. Rust diverges here intentionally:
        // the VM has already synchronized/cleaned the fallback roots, and this
        // bridge carries only opaque pointers plus an allocation-relative label.
        // It owns no roots and grants no public backend authority.
        ExecutableMemoryP6CallRequest::new(
            self.entry_offset(),
            vm,
            frame_base,
            callee_value_bits,
            ic_store_base,
        )
    }
}
