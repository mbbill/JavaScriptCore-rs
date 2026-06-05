//! ARM64 JSC stack-call dispatch request proof.
//!
//! C++ JSC map: `LowLevelInterpreter64.asm` `doVMEntry` publishes a machine
//! stack `CallFrame`, then `_llint_call_javascript` enters generated code with
//! `sp = CallFrame + sizeof(CallerFrameAndPC)` while `cfr/fp` still names the
//! `EntryFrame`. The generated ARM64 prologue pushes that `fp/lr` pair into the
//! callee `CallerFrameAndPC` slots, then makes `fp/x29` the callee `CallFrame*`.
//!
//! This module is still dormant. It derives a platform request only from the
//! already-proven stack-local JSC call frame and keeps the stack-frame lifetime
//! attached. It does not call the current raw ARM64 C ABI seed and does not
//! implement the final doVMEntry platform trampoline.

use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::platform::executable_memory_compartment::{
    ExecutableMemoryArm64JscStackCallRequest,
    ExecutableMemoryArm64JscStackCallRequestValidationError,
};

use super::{
    Arm64NativeEntryFrameAddressSource, Arm64NativeEntryJscStackCallRequestProof,
    Arm64NativeEntryStackFrameProof, FrameAddress, JSC_CALLER_FRAME_AND_PC_WORDS,
    JSC_REGISTER_BYTES,
};

const JSC_CALLER_FRAME_AND_PC_BYTES: usize = JSC_CALLER_FRAME_AND_PC_WORDS * JSC_REGISTER_BYTES;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64NativeEntryJscStackDispatchProofField {
    FrameSource,
    CallFrame,
    EntryFrame,
    VmEntryRecord,
    PostAllocationSp,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64NativeEntryJscStackDispatchRequestError {
    NonStackLocalFrameSource {
        actual: Arm64NativeEntryFrameAddressSource,
    },
    StackFrameProofMismatch {
        field: Arm64NativeEntryJscStackDispatchProofField,
    },
    EntryStackPointerOverflow {
        call_frame: FrameAddress,
    },
    NullCallFrame {
        call_frame: FrameAddress,
    },
    NullEntryStackPointer {
        entry_sp: FrameAddress,
    },
    NullEntryFrame {
        entry_frame: FrameAddress,
    },
    PlatformRequestInvalid {
        reason: ExecutableMemoryArm64JscStackCallRequestValidationError,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64NativeEntryJscStackDispatchRequestProof<'frame> {
    pub(crate) entry_offset: u32,
    pub(crate) call_frame: FrameAddress,
    pub(crate) entry_frame: FrameAddress,
    pub(crate) entry_sp: FrameAddress,
    pub(crate) platform_request: ExecutableMemoryArm64JscStackCallRequest,
    _stack_frame: PhantomData<&'frame ()>,
}

#[allow(dead_code)]
pub(crate) fn prove_arm64_native_entry_jsc_stack_dispatch_request<'frame>(
    stack_call_proof: &Arm64NativeEntryJscStackCallRequestProof,
    stack_frame_proof: &Arm64NativeEntryStackFrameProof<'frame>,
    entry_offset: u32,
) -> Result<
    Arm64NativeEntryJscStackDispatchRequestProof<'frame>,
    Arm64NativeEntryJscStackDispatchRequestError,
> {
    validate_stack_local_source(stack_call_proof.frame_source)?;
    validate_stack_local_source(stack_frame_proof.source)?;
    validate_stack_frame_lifetime_match(stack_call_proof, stack_frame_proof)?;

    let entry_sp = FrameAddress(
        stack_call_proof
            .call_frame
            .0
            .checked_add(JSC_CALLER_FRAME_AND_PC_BYTES)
            .ok_or(
                Arm64NativeEntryJscStackDispatchRequestError::EntryStackPointerOverflow {
                    call_frame: stack_call_proof.call_frame,
                },
            )?,
    );
    let call_frame = non_null_frame_address(stack_call_proof.call_frame).ok_or(
        Arm64NativeEntryJscStackDispatchRequestError::NullCallFrame {
            call_frame: stack_call_proof.call_frame,
        },
    )?;
    let entry_sp_ptr = non_null_frame_address(entry_sp)
        .ok_or(Arm64NativeEntryJscStackDispatchRequestError::NullEntryStackPointer { entry_sp })?;
    let entry_frame = non_null_frame_address(stack_call_proof.entry_frame).ok_or(
        Arm64NativeEntryJscStackDispatchRequestError::NullEntryFrame {
            entry_frame: stack_call_proof.entry_frame,
        },
    )?;
    let platform_request = ExecutableMemoryArm64JscStackCallRequest::new(
        entry_offset,
        entry_sp_ptr,
        call_frame,
        entry_frame,
    );
    platform_request.validate().map_err(|reason| {
        Arm64NativeEntryJscStackDispatchRequestError::PlatformRequestInvalid { reason }
    })?;

    Ok(Arm64NativeEntryJscStackDispatchRequestProof {
        entry_offset,
        call_frame: stack_call_proof.call_frame,
        entry_frame: stack_call_proof.entry_frame,
        entry_sp,
        platform_request,
        _stack_frame: PhantomData,
    })
}

fn validate_stack_local_source(
    source: Arm64NativeEntryFrameAddressSource,
) -> Result<(), Arm64NativeEntryJscStackDispatchRequestError> {
    if source != Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard {
        return Err(
            Arm64NativeEntryJscStackDispatchRequestError::NonStackLocalFrameSource {
                actual: source,
            },
        );
    }
    Ok(())
}

fn validate_stack_frame_lifetime_match(
    stack_call_proof: &Arm64NativeEntryJscStackCallRequestProof,
    stack_frame_proof: &Arm64NativeEntryStackFrameProof<'_>,
) -> Result<(), Arm64NativeEntryJscStackDispatchRequestError> {
    for (matches, field) in [
        (
            stack_call_proof.frame_source == stack_frame_proof.source,
            Arm64NativeEntryJscStackDispatchProofField::FrameSource,
        ),
        (
            stack_call_proof.call_frame == stack_frame_proof.call_frame,
            Arm64NativeEntryJscStackDispatchProofField::CallFrame,
        ),
        (
            stack_call_proof.entry_frame == stack_frame_proof.entry_frame,
            Arm64NativeEntryJscStackDispatchProofField::EntryFrame,
        ),
        (
            stack_call_proof.vm_entry_record == stack_frame_proof.vm_entry_record,
            Arm64NativeEntryJscStackDispatchProofField::VmEntryRecord,
        ),
        (
            stack_call_proof.post_allocation_sp == stack_frame_proof.post_allocation_sp,
            Arm64NativeEntryJscStackDispatchProofField::PostAllocationSp,
        ),
    ] {
        if !matches {
            return Err(
                Arm64NativeEntryJscStackDispatchRequestError::StackFrameProofMismatch { field },
            );
        }
    }
    Ok(())
}

fn non_null_frame_address(address: FrameAddress) -> Option<NonNull<c_void>> {
    NonNull::new(address.0 as *mut c_void)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;
    use crate::jit::{
        BaselineNativeEntryToken, BaselineNativeEntryTokenKind, EntryAbi, Entrypoint,
        EntrypointKind, ExecutableAllocationId, ExecutableAllocationLifecycle,
        ExecutableMemoryProtection, ExecutableMutationAuthority, JitCodeId, MachineCodeHandle,
        MachineCodeOwnership, MachineCodeRange,
    };
    use crate::runtime::{CallFrameId, CodeBlockId, EntryFrameId, NativeCodeId};

    fn stack_frame_proof<'frame>() -> Arm64NativeEntryStackFrameProof<'frame> {
        Arm64NativeEntryStackFrameProof {
            source: Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard,
            entry_frame: FrameAddress(0x3000),
            vm_entry_record: FrameAddress(0x2800),
            vm_entry_record_previous_top_call_frame: Some(FrameAddress(0x1000)),
            vm_entry_record_previous_top_entry_frame: Some(FrameAddress(0x1800)),
            call_frame: FrameAddress(0x2000),
            post_allocation_sp: FrameAddress(0x1ff0),
            local_area_words: 2,
            live_local_count: 1,
            argument_count_excluding_this: 2,
            padded_argument_count: 5,
            undefined_fill_count: 2,
            _stack_frame: PhantomData,
        }
    }

    fn stack_call_proof() -> Arm64NativeEntryJscStackCallRequestProof {
        Arm64NativeEntryJscStackCallRequestProof {
            owner: CodeBlockId(CellId(20)),
            code_block: CodeBlockId(CellId(20)),
            active_entry_frame: EntryFrameId(1),
            active_top_call_frame: CallFrameId(2),
            selected_token: token(),
            frame_source: Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard,
            call_frame: FrameAddress(0x2000),
            entry_frame: FrameAddress(0x3000),
            vm_entry_record: FrameAddress(0x2800),
            vm_entry_record_previous_top_call_frame: Some(FrameAddress(0x1000)),
            vm_entry_record_previous_top_entry_frame: Some(FrameAddress(0x1800)),
            post_allocation_sp: FrameAddress(0x1ff0),
            local_area_words: 2,
            live_local_count: 1,
            frame_word_count: 10,
            frame_size_bytes: 80,
            argument_count_including_this: 3,
            argument_count_excluding_this: 2,
            padded_argument_count: 5,
            undefined_fill_count: 2,
        }
    }

    fn token() -> BaselineNativeEntryToken {
        let owner = CodeBlockId(CellId(20));
        let allocation = ExecutableAllocationId(7);
        BaselineNativeEntryToken {
            owner,
            artifact_id: JitCodeId(8),
            native_symbol: NativeCodeId(9),
            machine_code: MachineCodeHandle {
                allocation,
                owner: MachineCodeOwnership::CodeBlock(owner),
                range: MachineCodeRange {
                    allocation,
                    start_offset: 0,
                    size_bytes: 64,
                },
                symbol: Some(NativeCodeId(9)),
                protection: ExecutableMemoryProtection::Executable,
                lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::LinkBuffer,
            },
            entrypoint: Entrypoint {
                kind: EntrypointKind::GeneratedCode,
                abi: EntryAbi::GeneratedCode,
                code: Some(JitCodeId(8)),
                boundary: None,
            },
            kind: BaselineNativeEntryTokenKind::Normal,
        }
    }

    #[test]
    fn arm64_native_entry_jsc_stack_dispatch_request_derives_entry_sp_from_call_frame() {
        let stack_frame = stack_frame_proof();
        let stack_call = stack_call_proof();

        let dispatch =
            prove_arm64_native_entry_jsc_stack_dispatch_request(&stack_call, &stack_frame, 24)
                .expect("JSC stack dispatch request");

        assert_eq!(dispatch.entry_offset, 24);
        assert_eq!(dispatch.call_frame, FrameAddress(0x2000));
        assert_eq!(dispatch.entry_frame, FrameAddress(0x3000));
        assert_eq!(dispatch.entry_sp, FrameAddress(0x2010));
        assert_eq!(dispatch.platform_request.entry_offset, 24);
        assert_eq!(
            dispatch.platform_request.call_frame.as_ptr() as usize,
            0x2000
        );
        assert_eq!(
            dispatch.platform_request.entry_frame.as_ptr() as usize,
            0x3000
        );
        assert_eq!(dispatch.platform_request.entry_sp.as_ptr() as usize, 0x2010);
        assert_eq!(dispatch.platform_request.validate(), Ok(()));
    }

    #[test]
    fn arm64_native_entry_jsc_stack_dispatch_request_rejects_register_window_source() {
        let mut stack_call = stack_call_proof();
        stack_call.frame_source = Arm64NativeEntryFrameAddressSource::RegisterFileWindow;

        assert_eq!(
            prove_arm64_native_entry_jsc_stack_dispatch_request(
                &stack_call,
                &stack_frame_proof(),
                0,
            ),
            Err(
                Arm64NativeEntryJscStackDispatchRequestError::NonStackLocalFrameSource {
                    actual: Arm64NativeEntryFrameAddressSource::RegisterFileWindow,
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_jsc_stack_dispatch_request_reattaches_stack_lifetime_token() {
        let stack_call = stack_call_proof();
        let mut mismatched_frame = stack_frame_proof();
        mismatched_frame.call_frame = FrameAddress(0x2100);

        assert_eq!(
            prove_arm64_native_entry_jsc_stack_dispatch_request(&stack_call, &mismatched_frame, 0),
            Err(
                Arm64NativeEntryJscStackDispatchRequestError::StackFrameProofMismatch {
                    field: Arm64NativeEntryJscStackDispatchProofField::CallFrame,
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_jsc_stack_dispatch_request_rejects_entry_sp_overflow() {
        let stack_frame = Arm64NativeEntryStackFrameProof {
            call_frame: FrameAddress(usize::MAX - 8),
            ..stack_frame_proof()
        };
        let stack_call = Arm64NativeEntryJscStackCallRequestProof {
            call_frame: FrameAddress(usize::MAX - 8),
            ..stack_call_proof()
        };

        assert_eq!(
            prove_arm64_native_entry_jsc_stack_dispatch_request(&stack_call, &stack_frame, 0),
            Err(
                Arm64NativeEntryJscStackDispatchRequestError::EntryStackPointerOverflow {
                    call_frame: FrameAddress(usize::MAX - 8),
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_jsc_stack_dispatch_request_rejects_null_entry_frame() {
        let stack_frame = Arm64NativeEntryStackFrameProof {
            entry_frame: FrameAddress(0),
            ..stack_frame_proof()
        };
        let stack_call = Arm64NativeEntryJscStackCallRequestProof {
            entry_frame: FrameAddress(0),
            ..stack_call_proof()
        };

        assert_eq!(
            prove_arm64_native_entry_jsc_stack_dispatch_request(&stack_call, &stack_frame, 0),
            Err(
                Arm64NativeEntryJscStackDispatchRequestError::NullEntryFrame {
                    entry_frame: FrameAddress(0),
                }
            )
        );
    }
}
