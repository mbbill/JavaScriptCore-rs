//! ARM64 JSC-shaped native-entry stack skeleton.
//!
//! C++ JSC map: `LowLevelInterpreter64.asm` `doVMEntry` creates an
//! `EntryFrame`/`VMEntryRecord`, copies a callee `CallFrame` to the machine
//! stack, publishes `VM::topCallFrame`/`VM::topEntryFrame`, calls generated
//! code, and restores the previous pair. `AssemblyHelpers::emitFunctionPrologue`
//! then makes ARM64 `fp/x29` the baseline `CallFrame*`.
//!
//! This Rust module is still proof-only. It deliberately does not call the
//! current ARM64 C-ABI seed and does not make public native admission succeed.
//! The seed takes a Rust register-file pointer in x1; JSC-shaped admission needs
//! a stack `CallFrame*`.

use core::marker::PhantomData;

use super::entry::FrameAddress;

const JSC_REGISTER_BYTES: usize = 8;
const JSC_STACK_ALIGNMENT_BYTES: usize = 16;
const JSC_CALLER_FRAME_AND_PC_WORDS: usize = 2;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64NativeEntryFrameAddressSource {
    StackLocalRustEntryGuard,
    BoxedVmStorage,
    RegisterFileWindow,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Arm64NativeEntryStackFrameError {
    LocalAreaDoesNotCoverLiveLocals {
        live_local_count: usize,
        local_area_words: usize,
    },
    LocalAreaDoesNotPreserveStackAlignment {
        local_area_words: usize,
    },
    CallFrameAddressUnaligned {
        call_frame: usize,
    },
    PostAllocationStackPointerUnaligned {
        post_allocation_sp: usize,
    },
    PostAllocationStackPointerNotBelowCallFrame {
        post_allocation_sp: usize,
        call_frame: usize,
    },
    EntryFrameNotAboveCallFrame {
        entry_frame: usize,
        call_frame: usize,
    },
    VmEntryRecordNotBetweenCallFrameAndEntryFrame {
        vm_entry_record: usize,
        call_frame: usize,
        entry_frame: usize,
    },
    NonStackLocalAddressSource {
        source: Arm64NativeEntryFrameAddressSource,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Arm64NativeEntryStackFrameRequest<const ARGUMENTS_EXCLUDING_THIS: usize> {
    pub(crate) vm: usize,
    pub(crate) context: usize,
    pub(crate) previous_top_call_frame: Option<FrameAddress>,
    pub(crate) previous_top_entry_frame: Option<FrameAddress>,
    pub(crate) code_block: usize,
    pub(crate) callee: u64,
    pub(crate) this_value: u64,
    pub(crate) arguments: [u64; ARGUMENTS_EXCLUDING_THIS],
    pub(crate) live_local_count: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64VmEntryRecord {
    vm: usize,
    context: usize,
    previous_top_call_frame: usize,
    previous_top_entry_frame: usize,
    // C++ stores a platform-dependent callee-save buffer after the top-frame
    // pair. This placeholder preserves the boundary without claiming real
    // register save/restore support.
    callee_save_registers_buffer_placeholder: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64VmEntryFrame {
    // C++ `EntryFrame` is an ABI anchor, not the storage for
    // `VMEntryRecord`; `vmEntryRecord(entryFrame)` points below it.
    anchor: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64StackCallFrame<
    const LOCAL_AREA_WORDS: usize,
    const ARGUMENTS_EXCLUDING_THIS: usize,
> {
    local_area: [u64; LOCAL_AREA_WORDS],
    caller_frame_and_pc: [usize; JSC_CALLER_FRAME_AND_PC_WORDS],
    code_block: usize,
    callee: u64,
    argument_count_including_this: u64,
    this_value: u64,
    arguments: [u64; ARGUMENTS_EXCLUDING_THIS],
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64NativeEntryStackFrame<
    const LOCAL_AREA_WORDS: usize,
    const ARGUMENTS_EXCLUDING_THIS: usize,
> {
    call_frame: Arm64StackCallFrame<LOCAL_AREA_WORDS, ARGUMENTS_EXCLUDING_THIS>,
    vm_entry_record: Arm64VmEntryRecord,
    entry_frame: Arm64VmEntryFrame,
    live_local_count: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Arm64NativeEntryStackFrameProof<'frame> {
    source: Arm64NativeEntryFrameAddressSource,
    entry_frame: FrameAddress,
    vm_entry_record: FrameAddress,
    vm_entry_record_previous_top_call_frame: Option<FrameAddress>,
    vm_entry_record_previous_top_entry_frame: Option<FrameAddress>,
    call_frame: FrameAddress,
    post_allocation_sp: FrameAddress,
    local_area_words: usize,
    live_local_count: usize,
    argument_count_excluding_this: usize,
    _stack_frame: PhantomData<&'frame ()>,
}

#[allow(dead_code)]
pub(crate) fn with_arm64_native_entry_stack_frame<
    const LOCAL_AREA_WORDS: usize,
    const ARGUMENTS_EXCLUDING_THIS: usize,
>(
    request: Arm64NativeEntryStackFrameRequest<ARGUMENTS_EXCLUDING_THIS>,
    body: impl for<'frame> FnOnce(Arm64NativeEntryStackFrameProof<'frame>),
) -> Result<(), Arm64NativeEntryStackFrameError> {
    validate_local_area::<LOCAL_AREA_WORDS>(request.live_local_count)?;
    let frame =
        Arm64NativeEntryStackFrame::<LOCAL_AREA_WORDS, ARGUMENTS_EXCLUDING_THIS>::new(request);
    let proof = frame.proof(Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard)?;
    body(proof);
    Ok(())
}

fn validate_arm64_native_entry_stack_frame_candidate(
    source: Arm64NativeEntryFrameAddressSource,
    entry_frame: FrameAddress,
    vm_entry_record: FrameAddress,
    previous_top_call_frame: Option<FrameAddress>,
    previous_top_entry_frame: Option<FrameAddress>,
    call_frame: FrameAddress,
    post_allocation_sp: FrameAddress,
    local_area_words: usize,
    live_local_count: usize,
    argument_count_excluding_this: usize,
) -> Result<Arm64NativeEntryStackFrameProof<'static>, Arm64NativeEntryStackFrameError> {
    if source != Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard {
        return Err(Arm64NativeEntryStackFrameError::NonStackLocalAddressSource { source });
    }
    validate_local_area_runtime(local_area_words, live_local_count)?;
    validate_addresses(entry_frame, call_frame, post_allocation_sp, vm_entry_record)?;
    Ok(Arm64NativeEntryStackFrameProof {
        source,
        entry_frame,
        vm_entry_record,
        vm_entry_record_previous_top_call_frame: previous_top_call_frame,
        vm_entry_record_previous_top_entry_frame: previous_top_entry_frame,
        call_frame,
        post_allocation_sp,
        local_area_words,
        live_local_count,
        argument_count_excluding_this,
        _stack_frame: PhantomData,
    })
}

impl<const LOCAL_AREA_WORDS: usize, const ARGUMENTS_EXCLUDING_THIS: usize>
    Arm64NativeEntryStackFrame<LOCAL_AREA_WORDS, ARGUMENTS_EXCLUDING_THIS>
{
    fn new(request: Arm64NativeEntryStackFrameRequest<ARGUMENTS_EXCLUDING_THIS>) -> Self {
        Self {
            call_frame: Arm64StackCallFrame {
                local_area: [0; LOCAL_AREA_WORDS],
                caller_frame_and_pc: [0; JSC_CALLER_FRAME_AND_PC_WORDS],
                code_block: request.code_block,
                callee: request.callee,
                argument_count_including_this: (ARGUMENTS_EXCLUDING_THIS as u64).saturating_add(1),
                this_value: request.this_value,
                arguments: request.arguments,
            },
            vm_entry_record: Arm64VmEntryRecord {
                vm: request.vm,
                context: request.context,
                previous_top_call_frame: option_frame_address_to_raw(
                    request.previous_top_call_frame,
                ),
                previous_top_entry_frame: option_frame_address_to_raw(
                    request.previous_top_entry_frame,
                ),
                callee_save_registers_buffer_placeholder: 0,
            },
            entry_frame: Arm64VmEntryFrame { anchor: 0 },
            live_local_count: request.live_local_count,
        }
    }

    fn proof(
        &self,
        source: Arm64NativeEntryFrameAddressSource,
    ) -> Result<Arm64NativeEntryStackFrameProof<'_>, Arm64NativeEntryStackFrameError> {
        validate_arm64_native_entry_stack_frame_candidate(
            source,
            self.entry_frame_address(),
            self.vm_entry_record_address(),
            raw_frame_address_to_option(self.vm_entry_record.previous_top_call_frame),
            raw_frame_address_to_option(self.vm_entry_record.previous_top_entry_frame),
            self.call_frame_address(),
            self.post_allocation_sp(),
            LOCAL_AREA_WORDS,
            self.live_local_count,
            ARGUMENTS_EXCLUDING_THIS,
        )
        .map(|proof| Arm64NativeEntryStackFrameProof {
            _stack_frame: PhantomData,
            ..proof
        })
    }

    fn call_frame_address(&self) -> FrameAddress {
        FrameAddress((&self.call_frame.caller_frame_and_pc as *const [usize; 2]) as usize)
    }

    fn post_allocation_sp(&self) -> FrameAddress {
        FrameAddress((&self.call_frame.local_area as *const [u64; LOCAL_AREA_WORDS]) as usize)
    }

    fn entry_frame_address(&self) -> FrameAddress {
        FrameAddress((&self.entry_frame as *const Arm64VmEntryFrame) as usize)
    }

    fn vm_entry_record_address(&self) -> FrameAddress {
        FrameAddress((&self.vm_entry_record as *const Arm64VmEntryRecord) as usize)
    }
}

const fn option_frame_address_to_raw(address: Option<FrameAddress>) -> usize {
    match address {
        Some(address) => address.0,
        None => 0,
    }
}

const fn raw_frame_address_to_option(address: usize) -> Option<FrameAddress> {
    if address == 0 {
        None
    } else {
        Some(FrameAddress(address))
    }
}

fn validate_local_area<const LOCAL_AREA_WORDS: usize>(
    live_local_count: usize,
) -> Result<(), Arm64NativeEntryStackFrameError> {
    validate_local_area_runtime(LOCAL_AREA_WORDS, live_local_count)
}

fn validate_local_area_runtime(
    local_area_words: usize,
    live_local_count: usize,
) -> Result<(), Arm64NativeEntryStackFrameError> {
    if local_area_words < live_local_count {
        return Err(
            Arm64NativeEntryStackFrameError::LocalAreaDoesNotCoverLiveLocals {
                live_local_count,
                local_area_words,
            },
        );
    }
    if local_area_words.saturating_mul(JSC_REGISTER_BYTES) % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(
            Arm64NativeEntryStackFrameError::LocalAreaDoesNotPreserveStackAlignment {
                local_area_words,
            },
        );
    }
    Ok(())
}

fn validate_addresses(
    entry_frame: FrameAddress,
    call_frame: FrameAddress,
    post_allocation_sp: FrameAddress,
    vm_entry_record: FrameAddress,
) -> Result<(), Arm64NativeEntryStackFrameError> {
    if call_frame.0 % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(Arm64NativeEntryStackFrameError::CallFrameAddressUnaligned {
            call_frame: call_frame.0,
        });
    }
    if post_allocation_sp.0 % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(
            Arm64NativeEntryStackFrameError::PostAllocationStackPointerUnaligned {
                post_allocation_sp: post_allocation_sp.0,
            },
        );
    }
    if post_allocation_sp.0 >= call_frame.0 {
        return Err(
            Arm64NativeEntryStackFrameError::PostAllocationStackPointerNotBelowCallFrame {
                post_allocation_sp: post_allocation_sp.0,
                call_frame: call_frame.0,
            },
        );
    }
    if entry_frame.0 <= call_frame.0 {
        return Err(
            Arm64NativeEntryStackFrameError::EntryFrameNotAboveCallFrame {
                entry_frame: entry_frame.0,
                call_frame: call_frame.0,
            },
        );
    }
    if vm_entry_record.0 <= call_frame.0 || vm_entry_record.0 >= entry_frame.0 {
        return Err(
            Arm64NativeEntryStackFrameError::VmEntryRecordNotBetweenCallFrameAndEntryFrame {
                vm_entry_record: vm_entry_record.0,
                call_frame: call_frame.0,
                entry_frame: entry_frame.0,
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Arm64NativeEntryStackFrameRequest<2> {
        Arm64NativeEntryStackFrameRequest {
            vm: 0x1000,
            context: 0x2000,
            previous_top_call_frame: Some(FrameAddress(0x3000)),
            previous_top_entry_frame: Some(FrameAddress(0x4000)),
            code_block: 0x5000,
            callee: 0x6000,
            this_value: 0x7000,
            arguments: [0x8000, 0x9000],
            live_local_count: 1,
        }
    }

    #[test]
    fn arm64_native_entry_stack_guard_builds_jsc_ordered_frame_proof() {
        with_arm64_native_entry_stack_frame::<2, 2>(request(), |proof| {
            assert_eq!(
                proof.source,
                Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard
            );
            assert_eq!(
                proof.vm_entry_record_previous_top_call_frame,
                Some(FrameAddress(0x3000))
            );
            assert_eq!(
                proof.vm_entry_record_previous_top_entry_frame,
                Some(FrameAddress(0x4000))
            );
            assert_eq!(proof.argument_count_excluding_this, 2);
            assert_eq!(proof.local_area_words, 2);
            assert_eq!(proof.live_local_count, 1);
            assert!(proof.post_allocation_sp.0 < proof.call_frame.0);
            assert!(proof.call_frame.0 < proof.vm_entry_record.0);
            assert!(proof.vm_entry_record.0 < proof.entry_frame.0);
            assert_eq!(proof.call_frame.0 % JSC_STACK_ALIGNMENT_BYTES, 0);
            assert_eq!(proof.post_allocation_sp.0 % JSC_STACK_ALIGNMENT_BYTES, 0);
        })
        .expect("stack-local ARM64 entry proof");
    }

    #[test]
    fn arm64_native_entry_stack_guard_rejects_too_small_local_area() {
        assert_eq!(
            with_arm64_native_entry_stack_frame::<0, 2>(request(), |_| ()),
            Err(
                Arm64NativeEntryStackFrameError::LocalAreaDoesNotCoverLiveLocals {
                    live_local_count: 1,
                    local_area_words: 0,
                },
            )
        );
    }

    #[test]
    fn arm64_native_entry_stack_guard_rejects_unaligned_local_area() {
        assert_eq!(
            with_arm64_native_entry_stack_frame::<1, 2>(request(), |_| ()),
            Err(
                Arm64NativeEntryStackFrameError::LocalAreaDoesNotPreserveStackAlignment {
                    local_area_words: 1,
                },
            )
        );
    }

    #[test]
    fn arm64_native_entry_candidate_rejects_boxed_storage_source() {
        assert_eq!(
            validate_arm64_native_entry_stack_frame_candidate(
                Arm64NativeEntryFrameAddressSource::BoxedVmStorage,
                FrameAddress(0x3000),
                FrameAddress(0x3000),
                None,
                None,
                FrameAddress(0x2000),
                FrameAddress(0x1ff0),
                2,
                1,
                0,
            ),
            Err(
                Arm64NativeEntryStackFrameError::NonStackLocalAddressSource {
                    source: Arm64NativeEntryFrameAddressSource::BoxedVmStorage,
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_candidate_rejects_register_file_window_source() {
        assert_eq!(
            validate_arm64_native_entry_stack_frame_candidate(
                Arm64NativeEntryFrameAddressSource::RegisterFileWindow,
                FrameAddress(0x3000),
                FrameAddress(0x2800),
                None,
                None,
                FrameAddress(0x2000),
                FrameAddress(0x1ff0),
                2,
                1,
                0,
            ),
            Err(
                Arm64NativeEntryStackFrameError::NonStackLocalAddressSource {
                    source: Arm64NativeEntryFrameAddressSource::RegisterFileWindow,
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_candidate_rejects_vm_entry_record_outside_jsc_order() {
        assert_eq!(
            validate_arm64_native_entry_stack_frame_candidate(
                Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard,
                FrameAddress(0x3000),
                FrameAddress(0x3000),
                None,
                None,
                FrameAddress(0x2000),
                FrameAddress(0x1ff0),
                2,
                1,
                0,
            ),
            Err(
                Arm64NativeEntryStackFrameError::VmEntryRecordNotBetweenCallFrameAndEntryFrame {
                    vm_entry_record: 0x3000,
                    call_frame: 0x2000,
                    entry_frame: 0x3000,
                }
            )
        );
    }
}
