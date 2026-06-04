//! JSC-shaped call-frame header storage for future native publication.
//!
//! This module maps to `interpreter/CallFrame.h`: JSC's `CallFrame*` points at
//! the caller-frame slot of a register-backed frame header. Rust does not yet
//! have that executable register-stack layout. The storage here is therefore a
//! non-executing metadata skeleton that can eventually be the authority for a
//! VM `FrameAddress` once conservative stack roots are wired.

use core::marker::PhantomData;
use std::ptr::NonNull;

use crate::bytecode::BytecodeIndex;
use crate::interpreter::{FrameState, InstalledCallFrame, RegisterWindow};
use crate::runtime::{CallFrameId, CodeBlockId, EntryFrameId, ObjectId, RuntimeValue};

use super::entry::FrameAddress;

/// Stable VM-owned storage for JSC-shaped call-frame header snapshots.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct JscCallFrameStorage {
    records: Vec<Box<JscCallFrameStorageRecord>>,
    next_ordinal: u64,
}

#[allow(dead_code)]
impl JscCallFrameStorage {
    /// Register an installed Rust interpreter frame as future-publishable storage.
    ///
    /// C++ `CallFrame::create(Register*)` returns a raw pointer into the VM
    /// register stack. Rust currently keeps generated native code's first-local
    /// ABI pointer separate from `FrameAddress`, so this method snapshots the
    /// installed frame metadata into boxed storage and returns an authority
    /// handle instead of accepting a raw `usize` or symbolic frame id.
    pub(crate) fn register_installed_frame(
        &mut self,
        frame: &InstalledCallFrame,
    ) -> JscCallFrameStorageHandle {
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let mut record = Box::new(JscCallFrameStorageRecord::from_installed_frame(
            self.next_ordinal,
            frame,
        ));
        let header = NonNull::from(&record.header);
        let handle = JscCallFrameStorageHandle {
            ordinal: record.ordinal,
            frame: frame.id,
            header,
        };
        record.handle = handle;
        self.records.push(record);
        handle
    }

    pub(crate) fn record(
        &self,
        handle: JscCallFrameStorageHandle,
    ) -> Option<&JscCallFrameStorageRecord> {
        self.records
            .iter()
            .map(Box::as_ref)
            .find(|record| record.matches(handle) && record.is_active())
    }

    pub(crate) fn frame_address(&self, handle: JscCallFrameStorageHandle) -> Option<FrameAddress> {
        self.record(handle).map(|record| record.header_address())
    }

    pub(crate) fn published_top_call_frame(
        &self,
        handle: JscCallFrameStorageHandle,
    ) -> Option<VmPublishedTopCallFrame<'_>> {
        self.record(handle)
            .map(VmPublishedTopCallFrame::from_storage_record)
    }

    pub(crate) fn retire(&mut self, handle: JscCallFrameStorageHandle) -> bool {
        let Some(record) = self
            .records
            .iter_mut()
            .map(Box::as_mut)
            .find(|record| record.matches(handle))
        else {
            return false;
        };
        if !record.is_active() {
            return false;
        }
        record.storage_state = JscCallFrameStorageRecordState::Retired;
        true
    }

    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }
}

/// Opaque authority proving that a `FrameAddress` came from registered storage.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JscCallFrameStorageHandle {
    ordinal: u64,
    frame: CallFrameId,
    header: NonNull<JscCallFrameHeaderSnapshot>,
}

#[allow(dead_code)]
impl JscCallFrameStorageHandle {
    pub(crate) fn frame(self) -> CallFrameId {
        self.frame
    }
}

/// Boxed record whose header address stays stable as the storage vector grows.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct JscCallFrameStorageRecord {
    ordinal: u64,
    handle: JscCallFrameStorageHandle,
    storage_state: JscCallFrameStorageRecordState,
    pub(crate) frame: CallFrameId,
    pub(crate) entry: Option<EntryFrameId>,
    pub(crate) header: JscCallFrameHeaderSnapshot,
    pub(crate) register_window: RegisterWindow,
    pub(crate) frame_state: FrameState,
}

#[allow(dead_code)]
impl JscCallFrameStorageRecord {
    fn from_installed_frame(ordinal: u64, frame: &InstalledCallFrame) -> Self {
        Self {
            ordinal,
            handle: JscCallFrameStorageHandle {
                ordinal,
                frame: frame.id,
                header: NonNull::dangling(),
            },
            storage_state: JscCallFrameStorageRecordState::Active,
            frame: frame.id,
            entry: frame.entry,
            header: JscCallFrameHeaderSnapshot::from_installed_frame(frame),
            register_window: frame.register_window,
            frame_state: frame.state,
        }
    }

    fn matches(&self, handle: JscCallFrameStorageHandle) -> bool {
        self.handle == handle
    }

    fn is_active(&self) -> bool {
        self.storage_state == JscCallFrameStorageRecordState::Active
    }

    pub(crate) fn header_address(&self) -> FrameAddress {
        FrameAddress((&self.header as *const JscCallFrameHeaderSnapshot) as usize)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JscCallFrameStorageRecordState {
    Active,
    Retired,
}

/// Storage-derived proof for publishing a native top call frame.
///
/// C++ publishes a `CallFrame*` directly from a live register-backed frame.
/// Rust's wrapper is lifetime-tied to this VM-owned storage record so
/// `VmEntryState` no longer accepts an arbitrary `FrameAddress` for native
/// call-frame publication. This is still not a conservative-root proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmPublishedTopCallFrame<'storage> {
    frame: CallFrameId,
    entry_frame: Option<EntryFrameId>,
    address: FrameAddress,
    storage_ordinal: u64,
    _storage: PhantomData<&'storage JscCallFrameStorageRecord>,
}

#[allow(dead_code)]
impl<'storage> VmPublishedTopCallFrame<'storage> {
    fn from_storage_record(record: &'storage JscCallFrameStorageRecord) -> Self {
        Self {
            frame: record.frame,
            entry_frame: record.entry,
            address: record.header_address(),
            storage_ordinal: record.ordinal,
            _storage: PhantomData,
        }
    }

    pub(crate) fn frame(self) -> CallFrameId {
        self.frame
    }

    pub(crate) fn entry_frame(self) -> Option<EntryFrameId> {
        self.entry_frame
    }

    pub(crate) fn address(self) -> FrameAddress {
        self.address
    }
}

/// Header metadata ordered after JSC `CallFrameSlot`.
///
/// JSC stores these fields directly in the VM register stack: caller frame,
/// return address, code block, callee, then argument count. Rust keeps frame and
/// entry ids outside this addressed header because today's interpreter stack
/// names frames symbolically; those ids are metadata only and must not be
/// widened into a production `FrameAddress`.
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JscCallFrameHeaderSnapshot {
    pub(crate) caller: JscCallerFrame,
    pub(crate) return_address: Option<BytecodeIndex>,
    pub(crate) code_block: Option<CodeBlockId>,
    pub(crate) callee: Option<ObjectId>,
    pub(crate) callee_value: Option<RuntimeValue>,
    pub(crate) argument_count_including_this: u32,
    pub(crate) call_site: Option<BytecodeIndex>,
}

impl JscCallFrameHeaderSnapshot {
    fn from_installed_frame(frame: &InstalledCallFrame) -> Self {
        Self {
            caller: JscCallerFrame::from_installed_frame(frame),
            return_address: frame.return_address,
            code_block: frame.code_block,
            callee: frame.callee,
            callee_value: frame.callee_value,
            argument_count_including_this: frame.argument_count_including_this,
            call_site: frame.bytecode_index,
        }
    }
}

/// Rust model of JSC's overloaded callerFrame-or-entryFrame header slot.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JscCallerFrame {
    None,
    Entry(EntryFrameId),
    Call(CallFrameId),
}

impl JscCallerFrame {
    fn from_installed_frame(frame: &InstalledCallFrame) -> Self {
        if let Some(caller) = frame.caller {
            Self::Call(caller)
        } else if let Some(entry) = frame.entry {
            Self::Entry(entry)
        } else {
            Self::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::register::CallFrameSlotLayout;
    use crate::gc::CellId;

    #[test]
    fn installed_frame_metadata_populates_jsc_header_snapshot() {
        let frame = installed_frame(
            CallFrameId(7),
            Some(EntryFrameId(3)),
            Some(CallFrameId(6)),
            128,
        );
        let mut storage = JscCallFrameStorage::default();

        let handle = storage.register_installed_frame(&frame);
        let record = storage.record(handle).expect("registered frame record");

        assert_eq!(handle.frame(), frame.id);
        assert_eq!(record.frame, frame.id);
        assert_eq!(record.entry, frame.entry);
        assert_eq!(record.storage_state, JscCallFrameStorageRecordState::Active);
        assert_eq!(record.header.caller, JscCallerFrame::Call(CallFrameId(6)));
        assert_eq!(record.header.return_address, frame.return_address);
        assert_eq!(record.header.code_block, frame.code_block);
        assert_eq!(record.header.callee, frame.callee);
        assert_eq!(record.header.callee_value, frame.callee_value);
        assert_eq!(
            record.header.argument_count_including_this,
            frame.argument_count_including_this
        );
        assert_eq!(record.header.call_site, frame.bytecode_index);
        assert_eq!(record.register_window, frame.register_window);
        assert_eq!(record.frame_state, frame.state);
    }

    #[test]
    fn caller_slot_uses_entry_frame_without_rust_caller() {
        let frame = installed_frame(CallFrameId(8), Some(EntryFrameId(4)), None, 256);
        let mut storage = JscCallFrameStorage::default();

        let handle = storage.register_installed_frame(&frame);
        let record = storage.record(handle).expect("registered frame record");

        assert_eq!(record.header.caller, JscCallerFrame::Entry(EntryFrameId(4)));
    }

    #[test]
    fn active_handle_yields_storage_derived_publication_proof() {
        let frame = installed_frame(CallFrameId(9), Some(EntryFrameId(4)), None, 512);
        let mut storage = JscCallFrameStorage::default();
        let handle = storage.register_installed_frame(&frame);

        let published = storage
            .published_top_call_frame(handle)
            .expect("published top call-frame proof");

        assert_eq!(published.frame(), frame.id);
        assert_eq!(published.entry_frame(), frame.entry);
        assert_eq!(published.address(), storage.frame_address(handle).unwrap());
    }

    #[test]
    fn retired_handle_rejects_record_address_and_publication_proof() {
        let frame = installed_frame(CallFrameId(10), Some(EntryFrameId(4)), None, 1024);
        let mut storage = JscCallFrameStorage::default();
        let handle = storage.register_installed_frame(&frame);
        assert!(storage.frame_address(handle).is_some());
        assert!(storage.published_top_call_frame(handle).is_some());

        assert!(storage.retire(handle));
        assert_eq!(storage.record(handle), None);
        assert_eq!(storage.frame_address(handle), None);
        assert_eq!(storage.published_top_call_frame(handle), None);
        assert!(!storage.retire(handle));
    }

    #[test]
    fn registered_header_address_stays_stable_across_storage_growth() {
        let first = installed_frame(CallFrameId(1), Some(EntryFrameId(1)), None, 64);
        let mut storage = JscCallFrameStorage::default();
        let first_handle = storage.register_installed_frame(&first);
        let first_address = storage
            .frame_address(first_handle)
            .expect("first frame address");

        for index in 2..128 {
            let frame = installed_frame(
                CallFrameId(index),
                Some(EntryFrameId(1)),
                Some(CallFrameId(index - 1)),
                64 + index as usize * 8,
            );
            storage.register_installed_frame(&frame);
        }

        assert_eq!(storage.len(), 127);
        assert_eq!(storage.frame_address(first_handle), Some(first_address));
    }

    #[test]
    fn storage_api_rejects_fabricated_handle_and_keeps_raw_values_separate() {
        let frame = installed_frame(CallFrameId(11), Some(EntryFrameId(5)), None, 4096);
        let mut storage = JscCallFrameStorage::default();
        let handle = storage.register_installed_frame(&frame);
        let stored_address = storage.frame_address(handle).expect("stored address");

        // `FrameAddress(pub usize)` is still broadly constructible outside this
        // storage module. The guardrail here is narrower: this storage API does
        // not accept raw native ABI bases or symbolic ids as handles for its
        // future production address authority.
        assert_ne!(stored_address, FrameAddress(frame.register_window.base));
        assert_ne!(stored_address, FrameAddress(frame.id.0 as usize));

        let raw_base_handle = JscCallFrameStorageHandle {
            ordinal: frame.register_window.base as u64,
            frame: CallFrameId(frame.register_window.base as u32),
            header: NonNull::dangling(),
        };
        let symbolic_id_handle = JscCallFrameStorageHandle {
            ordinal: frame.id.0 as u64,
            frame: frame.id,
            header: NonNull::dangling(),
        };
        let fabricated = JscCallFrameStorageHandle {
            ordinal: handle.ordinal.saturating_add(1),
            frame: frame.id,
            header: NonNull::dangling(),
        };
        assert_eq!(storage.frame_address(raw_base_handle), None);
        assert_eq!(storage.published_top_call_frame(raw_base_handle), None);
        assert_eq!(storage.frame_address(symbolic_id_handle), None);
        assert_eq!(storage.published_top_call_frame(symbolic_id_handle), None);
        assert_eq!(storage.frame_address(fabricated), None);
        assert_eq!(storage.published_top_call_frame(fabricated), None);
    }

    fn installed_frame(
        id: CallFrameId,
        entry: Option<EntryFrameId>,
        caller: Option<CallFrameId>,
        base: usize,
    ) -> InstalledCallFrame {
        InstalledCallFrame {
            id,
            entry,
            caller,
            code_block: Some(CodeBlockId(CellId(100 + id.0))),
            callee: Some(ObjectId(CellId(200 + id.0))),
            callee_value: Some(RuntimeValue::from_i32(id.0 as i32)),
            lexical_scope: None,
            bytecode_index: Some(BytecodeIndex::from_offset(3 + id.0)),
            return_address: Some(BytecodeIndex::from_offset(30 + id.0)),
            return_continuation: None,
            argument_count_including_this: 3,
            register_window: RegisterWindow {
                owner: id,
                base,
                local_count: 4,
                argument_base: base + 4,
                argument_count: 3,
                this_offset: CallFrameSlotLayout::JSC_RUST.this_argument_offset,
            },
            state: FrameState::Executing,
        }
    }
}
