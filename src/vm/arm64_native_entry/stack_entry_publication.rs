//! ARM64 stack-local VM top-frame publication proof.
//!
//! C++ JSC map: `LowLevelInterpreter64.asm` `doVMEntry` saves
//! `VM::topCallFrame` / `VM::topEntryFrame` into `VMEntryRecord`, publishes the
//! stack-local `(sp, cfr)` pair before calling generated JavaScript, and
//! restores the saved pair on normal return. `_llint_call_javascript` enters
//! generated code with `sp = CallFrame + CallerFrameAndPCSize` while `cfr`
//! still names the `EntryFrame`.
//!
//! This module is proof-only. It publishes through `VmEntryState` only while an
//! `Arm64NativeEntryStackFrameProof` lifetime is live, and it does not make
//! public ARM64 callable admission succeed.

use core::marker::PhantomData;

use crate::gc::HeapId;

use super::{Arm64NativeEntryFrameAddressSource, Arm64NativeEntryStackFrameProof};
use crate::vm::entry::{
    EntryKind, FrameAddress, VmEntryState, VmStackEntryPublicationError,
    VmStackEntryPublicationExitRecord, VmStackEntryPublicationGuard, VmStackEntryPublicationRecord,
    VmStackEntryPublicationRequest,
};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum Arm64NativeEntryStackPublicationError {
    NonStackLocalFrameSource {
        actual: Arm64NativeEntryFrameAddressSource,
    },
    PreviousTopCallFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    PreviousTopEntryFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    PublishedTopCallFrameMismatch {
        expected: FrameAddress,
        actual: Option<FrameAddress>,
    },
    PublishedTopEntryFrameMismatch {
        expected: FrameAddress,
        actual: Option<FrameAddress>,
    },
    EntryState {
        error: VmStackEntryPublicationError,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct Arm64NativeEntryStackPublicationProof<'frame> {
    pub(in crate::vm) top_call_frame: FrameAddress,
    pub(in crate::vm) top_entry_frame: FrameAddress,
    pub(in crate::vm) vm_entry_record: FrameAddress,
    pub(in crate::vm) previous_top_call_frame: Option<FrameAddress>,
    pub(in crate::vm) previous_top_entry_frame: Option<FrameAddress>,
    pub(in crate::vm) argument_count_excluding_this: usize,
    pub(in crate::vm) padded_argument_count: usize,
    pub(in crate::vm) live_local_count: usize,
    pub(in crate::vm) record: VmStackEntryPublicationRecord,
    _stack_frame: PhantomData<&'frame ()>,
}

#[allow(dead_code)]
pub(in crate::vm) struct Arm64NativeEntryStackPublicationGuard<'vm, 'frame> {
    guard: VmStackEntryPublicationGuard<'vm, 'frame>,
    proof: Arm64NativeEntryStackPublicationProof<'frame>,
}

#[allow(dead_code)]
impl<'vm, 'frame> Arm64NativeEntryStackPublicationGuard<'vm, 'frame> {
    pub(in crate::vm) fn proof(&self) -> Arm64NativeEntryStackPublicationProof<'frame> {
        self.proof
    }

    pub(crate) fn top_call_frame(&self) -> Option<FrameAddress> {
        self.guard.top_call_frame()
    }

    pub(crate) fn top_entry_frame(&self) -> Option<FrameAddress> {
        self.guard.top_entry_frame()
    }

    pub(crate) fn record(&self) -> VmStackEntryPublicationRecord {
        self.guard.record()
    }

    pub(crate) fn normal_return_exit_record(&self) -> VmStackEntryPublicationExitRecord {
        self.guard.normal_return_exit_record()
    }

    pub(crate) fn enter_nested<'nested>(
        &mut self,
        stack_frame: &Arm64NativeEntryStackFrameProof<'nested>,
        kind: EntryKind,
        heap: HeapId,
    ) -> Result<
        Arm64NativeEntryStackPublicationGuard<'_, 'nested>,
        Arm64NativeEntryStackPublicationError,
    > {
        validate_stack_frame_matches_current_top_pair(
            self.guard.top_call_frame(),
            self.guard.top_entry_frame(),
            stack_frame,
        )?;
        let guard = self
            .guard
            .enter_stack_published(entry_request(stack_frame, kind, heap))
            .map_err(|error| Arm64NativeEntryStackPublicationError::EntryState { error })?;
        Ok(guard_from_vm_entry_guard(guard, stack_frame))
    }
}

#[allow(dead_code)]
pub(in crate::vm) fn enter_arm64_native_entry_stack_publication<'vm, 'frame>(
    state: &'vm mut VmEntryState,
    stack_frame: &Arm64NativeEntryStackFrameProof<'frame>,
    kind: EntryKind,
    heap: HeapId,
) -> Result<Arm64NativeEntryStackPublicationGuard<'vm, 'frame>, Arm64NativeEntryStackPublicationError>
{
    validate_stack_frame_matches_current_top_pair(
        state.top_frame(),
        state.entry_frame(),
        stack_frame,
    )?;
    let guard = state
        .enter_stack_published(entry_request(stack_frame, kind, heap))
        .map_err(|error| Arm64NativeEntryStackPublicationError::EntryState { error })?;
    Ok(guard_from_vm_entry_guard(guard, stack_frame))
}

fn validate_stack_frame_matches_current_top_pair(
    current_top_call_frame: Option<FrameAddress>,
    current_top_entry_frame: Option<FrameAddress>,
    stack_frame: &Arm64NativeEntryStackFrameProof<'_>,
) -> Result<(), Arm64NativeEntryStackPublicationError> {
    if stack_frame.source != Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard {
        return Err(
            Arm64NativeEntryStackPublicationError::NonStackLocalFrameSource {
                actual: stack_frame.source,
            },
        );
    }
    if stack_frame.vm_entry_record_previous_top_call_frame != current_top_call_frame {
        return Err(
            Arm64NativeEntryStackPublicationError::PreviousTopCallFrameMismatch {
                expected: current_top_call_frame,
                actual: stack_frame.vm_entry_record_previous_top_call_frame,
            },
        );
    }
    if stack_frame.vm_entry_record_previous_top_entry_frame != current_top_entry_frame {
        return Err(
            Arm64NativeEntryStackPublicationError::PreviousTopEntryFrameMismatch {
                expected: current_top_entry_frame,
                actual: stack_frame.vm_entry_record_previous_top_entry_frame,
            },
        );
    }
    Ok(())
}

fn entry_request<'frame>(
    stack_frame: &Arm64NativeEntryStackFrameProof<'frame>,
    kind: EntryKind,
    heap: HeapId,
) -> VmStackEntryPublicationRequest<'frame> {
    VmStackEntryPublicationRequest {
        top_call_frame: stack_frame.call_frame,
        top_entry_frame: stack_frame.entry_frame,
        vm_entry_record: stack_frame.vm_entry_record,
        previous_top_call_frame: stack_frame.vm_entry_record_previous_top_call_frame,
        previous_top_entry_frame: stack_frame.vm_entry_record_previous_top_entry_frame,
        kind,
        heap,
        _stack: PhantomData,
    }
}

fn guard_from_vm_entry_guard<'vm, 'frame>(
    guard: VmStackEntryPublicationGuard<'vm, 'frame>,
    stack_frame: &Arm64NativeEntryStackFrameProof<'frame>,
) -> Arm64NativeEntryStackPublicationGuard<'vm, 'frame> {
    debug_assert_eq!(guard.top_call_frame(), Some(stack_frame.call_frame));
    debug_assert_eq!(guard.top_entry_frame(), Some(stack_frame.entry_frame));

    let record = guard.record();
    Arm64NativeEntryStackPublicationGuard {
        guard,
        proof: Arm64NativeEntryStackPublicationProof {
            top_call_frame: stack_frame.call_frame,
            top_entry_frame: stack_frame.entry_frame,
            vm_entry_record: stack_frame.vm_entry_record,
            previous_top_call_frame: stack_frame.vm_entry_record_previous_top_call_frame,
            previous_top_entry_frame: stack_frame.vm_entry_record_previous_top_entry_frame,
            argument_count_excluding_this: stack_frame.argument_count_excluding_this,
            padded_argument_count: stack_frame.padded_argument_count,
            live_local_count: stack_frame.live_local_count,
            record,
            _stack_frame: PhantomData,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        validate_arm64_native_entry_stack_frame_candidate, with_arm64_native_entry_stack_frame,
        Arm64NativeEntryStackFrameRequest, JSC_ARM64_VM_CALLEE_SAVE_BUFFER_BYTES,
        JSC_ARM64_VM_CALLEE_SAVE_REGISTER_COUNT,
    };
    use super::*;

    fn request(
        previous_top_call_frame: Option<FrameAddress>,
        previous_top_entry_frame: Option<FrameAddress>,
    ) -> Arm64NativeEntryStackFrameRequest<2> {
        Arm64NativeEntryStackFrameRequest {
            vm: 0x1000,
            context: 0x2000,
            previous_top_call_frame,
            previous_top_entry_frame,
            code_block: 0x5000,
            callee: 0x6000,
            this_value: 0x7000,
            arguments: [0x8000, 0x9000],
            live_local_count: 1,
        }
    }

    fn forged_stack_frame(
        source: Arm64NativeEntryFrameAddressSource,
        previous_top_call_frame: Option<FrameAddress>,
        previous_top_entry_frame: Option<FrameAddress>,
    ) -> Arm64NativeEntryStackFrameProof<'static> {
        Arm64NativeEntryStackFrameProof {
            source,
            entry_frame: FrameAddress(0x3000),
            vm_entry_record: FrameAddress(0x2800),
            vm_entry_record_previous_top_call_frame: previous_top_call_frame,
            vm_entry_record_previous_top_entry_frame: previous_top_entry_frame,
            vm_entry_record_callee_save_buffer: FrameAddress(0x2820),
            vm_entry_record_callee_save_register_count: JSC_ARM64_VM_CALLEE_SAVE_REGISTER_COUNT,
            vm_entry_record_callee_save_buffer_bytes: JSC_ARM64_VM_CALLEE_SAVE_BUFFER_BYTES,
            call_frame: FrameAddress(0x2000),
            post_allocation_sp: FrameAddress(0x1ff0),
            local_area_words: 2,
            live_local_count: 1,
            argument_count_excluding_this: 2,
            padded_argument_count: 3,
            undefined_fill_count: 0,
            _stack_frame: PhantomData,
        }
    }

    fn assert_default_state(state: &VmEntryState) {
        assert_eq!(state.entry_depth(), 0);
        assert_eq!(state.top_frame(), None);
        assert_eq!(state.entry_frame(), None);
        assert!(state.stack_entry_publications().is_empty());
        assert!(state.stack_entry_publication_exits().is_empty());
    }

    #[test]
    fn arm64_stack_entry_publication_publishes_top_pair_from_stack_proof_and_restores() {
        let mut state = VmEntryState::default();
        with_arm64_native_entry_stack_frame::<2, 2>(request(None, None), |stack_frame| {
            let call_frame = stack_frame.call_frame;
            let entry_frame = stack_frame.entry_frame;
            {
                let guard = enter_arm64_native_entry_stack_publication(
                    &mut state,
                    &stack_frame,
                    EntryKind::Script,
                    HeapId(30),
                )
                .expect("stack-local top-frame publication");

                assert_eq!(guard.top_call_frame(), Some(call_frame));
                assert_eq!(guard.top_entry_frame(), Some(entry_frame));
                assert_eq!(guard.record().previous_top_call_frame, None);
                assert_eq!(guard.record().previous_top_entry_frame, None);
                assert_eq!(guard.record().top_call_frame, call_frame);
                assert_eq!(guard.record().top_entry_frame, entry_frame);
                assert_eq!(guard.record().vm_entry_record, stack_frame.vm_entry_record);
                assert_eq!(guard.proof().top_call_frame, call_frame);
                assert_eq!(guard.proof().top_entry_frame, entry_frame);
                assert_eq!(guard.proof().vm_entry_record, stack_frame.vm_entry_record);
            }

            assert_eq!(state.entry_depth(), 0);
            assert_eq!(state.top_frame(), None);
            assert_eq!(state.entry_frame(), None);
            assert_eq!(state.stack_entry_publications().len(), 1);
            assert_eq!(state.stack_entry_publication_exits().len(), 1);
            assert_eq!(
                state.stack_entry_publication_exits()[0].restored_top_call_frame,
                None
            );
            assert_eq!(
                state.stack_entry_publication_exits()[0].restored_top_entry_frame,
                None
            );
        })
        .expect("stack-local ARM64 frame proof");
    }

    #[test]
    fn arm64_stack_entry_publication_rejects_previous_top_call_frame_mismatch_without_mutation() {
        let mut state = VmEntryState::default();
        with_arm64_native_entry_stack_frame::<2, 2>(
            request(Some(FrameAddress(0x3000)), None),
            |stack_frame| {
                let error = enter_arm64_native_entry_stack_publication(
                    &mut state,
                    &stack_frame,
                    EntryKind::Script,
                    HeapId(31),
                )
                .err()
                .expect("previous top-call-frame rejection");

                assert_eq!(
                    error,
                    Arm64NativeEntryStackPublicationError::PreviousTopCallFrameMismatch {
                        expected: None,
                        actual: Some(FrameAddress(0x3000)),
                    }
                );
                assert_default_state(&state);
            },
        )
        .expect("stack-local ARM64 frame proof");
    }

    #[test]
    fn arm64_stack_entry_publication_rejects_previous_top_entry_frame_mismatch_without_mutation() {
        let mut state = VmEntryState::default();
        with_arm64_native_entry_stack_frame::<2, 2>(
            request(None, Some(FrameAddress(0x4000))),
            |stack_frame| {
                let error = enter_arm64_native_entry_stack_publication(
                    &mut state,
                    &stack_frame,
                    EntryKind::Script,
                    HeapId(32),
                )
                .err()
                .expect("previous top-entry-frame rejection");

                assert_eq!(
                    error,
                    Arm64NativeEntryStackPublicationError::PreviousTopEntryFrameMismatch {
                        expected: None,
                        actual: Some(FrameAddress(0x4000)),
                    }
                );
                assert_default_state(&state);
            },
        )
        .expect("stack-local ARM64 frame proof");
    }

    #[test]
    fn arm64_stack_entry_publication_rejects_non_stack_local_source_without_mutation() {
        let mut state = VmEntryState::default();
        let stack_frame = forged_stack_frame(
            Arm64NativeEntryFrameAddressSource::RegisterFileWindow,
            None,
            None,
        );

        let error = enter_arm64_native_entry_stack_publication(
            &mut state,
            &stack_frame,
            EntryKind::Script,
            HeapId(33),
        )
        .err()
        .expect("source rejection");

        assert_eq!(
            error,
            Arm64NativeEntryStackPublicationError::NonStackLocalFrameSource {
                actual: Arm64NativeEntryFrameAddressSource::RegisterFileWindow,
            }
        );
        assert_default_state(&state);
    }

    #[test]
    fn arm64_stack_entry_publication_nests_and_restores_inner_then_outer_pair() {
        let mut state = VmEntryState::default();
        with_arm64_native_entry_stack_frame::<2, 2>(request(None, None), |outer_frame| {
            let outer_call_frame = outer_frame.call_frame;
            let outer_entry_frame = outer_frame.entry_frame;
            let mut outer = enter_arm64_native_entry_stack_publication(
                &mut state,
                &outer_frame,
                EntryKind::Script,
                HeapId(34),
            )
            .expect("outer stack-local publication");

            let inner_frame = validate_arm64_native_entry_stack_frame_candidate(
                Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard,
                FrameAddress(0x5000),
                FrameAddress(0x4800),
                Some(outer_call_frame),
                Some(outer_entry_frame),
                FrameAddress(0x4820),
                JSC_ARM64_VM_CALLEE_SAVE_REGISTER_COUNT,
                JSC_ARM64_VM_CALLEE_SAVE_BUFFER_BYTES,
                FrameAddress(0x4000),
                FrameAddress(0x3ff0),
                2,
                1,
                2,
                3,
            )
            .expect("validated inner stack-local frame proof");
            {
                let inner = outer
                    .enter_nested(&inner_frame, EntryKind::HostCall, HeapId(35))
                    .expect("inner stack-local publication");
                assert_eq!(inner.top_call_frame(), Some(inner_frame.call_frame));
                assert_eq!(inner.top_entry_frame(), Some(inner_frame.entry_frame));
                assert_eq!(
                    inner.proof().previous_top_call_frame,
                    Some(outer_call_frame)
                );
                assert_eq!(
                    inner.proof().previous_top_entry_frame,
                    Some(outer_entry_frame)
                );
            }

            assert_eq!(outer.top_call_frame(), Some(outer_call_frame));
            assert_eq!(outer.top_entry_frame(), Some(outer_entry_frame));
        })
        .expect("outer stack-local ARM64 frame proof");

        assert_eq!(state.entry_depth(), 0);
        assert_eq!(state.top_frame(), None);
        assert_eq!(state.entry_frame(), None);
        assert_eq!(state.stack_entry_publications().len(), 2);
        assert_eq!(state.stack_entry_publication_exits().len(), 2);
        assert_eq!(
            state.stack_entry_publication_exits()[0].restored_top_call_frame,
            Some(state.stack_entry_publications()[0].top_call_frame)
        );
        assert_eq!(
            state.stack_entry_publication_exits()[0].restored_top_entry_frame,
            Some(state.stack_entry_publications()[0].top_entry_frame)
        );
        assert_eq!(
            state.stack_entry_publication_exits()[1].restored_top_call_frame,
            None
        );
        assert_eq!(
            state.stack_entry_publication_exits()[1].restored_top_entry_frame,
            None
        );
    }
}
