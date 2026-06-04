//! JSC-shaped VM entry-frame storage for future top-entry-frame publication.
//!
//! This module maps to `interpreter/EntryFrame.h` and
//! `interpreter/VMEntryRecord.h`: JSC's `EntryFrame*` identifies a
//! vmEntryToJavaScript stack frame, and its `VMEntryRecord` preserves the
//! previous `VM::topCallFrame` / `VM::topEntryFrame` pair for restore.

use core::marker::PhantomData;
use std::ptr::NonNull;

use crate::runtime::{CallFrameId, EntryFrameId};

use super::entry::FrameAddress;

/// Stable VM-owned storage for JSC-shaped entry-frame snapshots.
///
/// C++ stores `VMEntryRecord` in the native vmEntryToJavaScript frame after
/// callee-save registers. Rust does not yet have that platform entry frame or
/// callee-save stack layout, so these boxed, non-executing records are the
/// VM-owned authority for future `topEntryFrame` publication. They are not a
/// machine-stack or conservative-root proof.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct JscEntryFrameStorage {
    records: Vec<Box<JscEntryFrameStorageRecord>>,
    next_ordinal: u64,
}

#[allow(dead_code)]
impl JscEntryFrameStorage {
    /// Register an interpreter entry frame as future-publishable storage.
    ///
    /// The caller provides both symbolic Rust frame ids and the JSC-shaped
    /// previous top-frame address pair. The returned handle is the only way to
    /// recover a storage-derived `FrameAddress` or publication proof.
    pub(crate) fn register_entry_frame(
        &mut self,
        registration: JscEntryFrameRegistration,
    ) -> JscEntryFrameStorageHandle {
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let mut record = Box::new(JscEntryFrameStorageRecord::from_registration(
            self.next_ordinal,
            registration,
        ));
        let entry_frame = NonNull::from(&record.entry_frame);
        let handle = JscEntryFrameStorageHandle {
            ordinal: record.ordinal,
            entry: registration.entry,
            entry_frame,
        };
        record.handle = handle;
        self.records.push(record);
        handle
    }

    pub(crate) fn record(
        &self,
        handle: JscEntryFrameStorageHandle,
    ) -> Option<&JscEntryFrameStorageRecord> {
        self.records
            .iter()
            .map(Box::as_ref)
            .find(|record| record.matches(handle) && record.is_active())
    }

    pub(crate) fn entry_frame_address(
        &self,
        handle: JscEntryFrameStorageHandle,
    ) -> Option<FrameAddress> {
        self.record(handle)
            .map(JscEntryFrameStorageRecord::entry_frame_address)
    }

    pub(crate) fn published_entry_frame(
        &self,
        handle: JscEntryFrameStorageHandle,
    ) -> Option<VmPublishedEntryFrame<'_>> {
        self.record(handle)
            .map(VmPublishedEntryFrame::from_storage_record)
    }

    pub(crate) fn retire(&mut self, handle: JscEntryFrameStorageHandle) -> bool {
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
        record.storage_state = JscEntryFrameStorageRecordState::Retired;
        true
    }

    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }
}

/// Values captured while setting up a VM-entry frame.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JscEntryFrameRegistration {
    pub(crate) entry: EntryFrameId,
    pub(crate) previous_entry_frame: Option<EntryFrameId>,
    pub(crate) saved_top_call_frame: Option<CallFrameId>,
    pub(crate) previous_top_call_frame: Option<FrameAddress>,
    pub(crate) previous_top_entry_frame: Option<FrameAddress>,
}

/// Opaque authority proving that an entry-frame address came from VM storage.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JscEntryFrameStorageHandle {
    ordinal: u64,
    entry: EntryFrameId,
    entry_frame: NonNull<JscEntryFrameSnapshot>,
}

#[allow(dead_code)]
impl JscEntryFrameStorageHandle {
    pub(crate) fn entry(self) -> EntryFrameId {
        self.entry
    }
}

/// Boxed record whose entry-frame address stays stable as the storage vector grows.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct JscEntryFrameStorageRecord {
    ordinal: u64,
    handle: JscEntryFrameStorageHandle,
    storage_state: JscEntryFrameStorageRecordState,
    pub(crate) entry: EntryFrameId,
    pub(crate) previous_entry_frame: Option<EntryFrameId>,
    pub(crate) saved_top_call_frame: Option<CallFrameId>,
    pub(crate) entry_frame: JscEntryFrameSnapshot,
    pub(crate) vm_entry_record: JscVmEntryRecordSnapshot,
}

#[allow(dead_code)]
impl JscEntryFrameStorageRecord {
    fn from_registration(ordinal: u64, registration: JscEntryFrameRegistration) -> Self {
        Self {
            ordinal,
            handle: JscEntryFrameStorageHandle {
                ordinal,
                entry: registration.entry,
                entry_frame: NonNull::dangling(),
            },
            storage_state: JscEntryFrameStorageRecordState::Active,
            entry: registration.entry,
            previous_entry_frame: registration.previous_entry_frame,
            saved_top_call_frame: registration.saved_top_call_frame,
            entry_frame: JscEntryFrameSnapshot::default(),
            vm_entry_record: JscVmEntryRecordSnapshot::new(
                registration.previous_top_call_frame,
                registration.previous_top_entry_frame,
            ),
        }
    }

    fn matches(&self, handle: JscEntryFrameStorageHandle) -> bool {
        self.handle == handle
    }

    fn is_active(&self) -> bool {
        self.storage_state == JscEntryFrameStorageRecordState::Active
    }

    pub(crate) fn entry_frame_address(&self) -> FrameAddress {
        FrameAddress((&self.entry_frame as *const JscEntryFrameSnapshot) as usize)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JscEntryFrameStorageRecordState {
    Active,
    Retired,
}

/// Rust stand-in for JSC's `EntryFrame*` anchor.
///
/// JSC's `EntryFrame` type is an ABI marker for a native stack frame. Rust keeps
/// symbolic `EntryFrameId` metadata in `JscEntryFrameStorageRecord` instead of
/// inside this addressed snapshot so ids cannot be widened into production
/// `FrameAddress` values.
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct JscEntryFrameSnapshot {
    abi_anchor: usize,
}

/// Snapshot of the JSC `VMEntryRecord` fields needed for top-frame restore.
///
/// JSC also stores `VM*` and context cells in this native stack record. Rust
/// already owns these records from `Vm`, and current entry metadata has no
/// platform context slot, so this snapshot is intentionally limited to the
/// adjacent nullable top-frame pair that LLInt saves and restores.
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JscVmEntryRecordSnapshot {
    previous_top_call_frame: usize,
    previous_top_entry_frame: usize,
}

#[allow(dead_code)]
impl JscVmEntryRecordSnapshot {
    fn new(
        previous_top_call_frame: Option<FrameAddress>,
        previous_top_entry_frame: Option<FrameAddress>,
    ) -> Self {
        Self {
            previous_top_call_frame: option_frame_address_to_raw(previous_top_call_frame),
            previous_top_entry_frame: option_frame_address_to_raw(previous_top_entry_frame),
        }
    }

    pub(crate) fn previous_top_call_frame(self) -> Option<FrameAddress> {
        raw_frame_address_to_option(self.previous_top_call_frame)
    }

    pub(crate) fn previous_top_entry_frame(self) -> Option<FrameAddress> {
        raw_frame_address_to_option(self.previous_top_entry_frame)
    }
}

/// Storage-derived proof for publishing a VM top entry frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmPublishedEntryFrame<'storage> {
    entry: EntryFrameId,
    previous_entry_frame: Option<EntryFrameId>,
    saved_top_call_frame: Option<CallFrameId>,
    address: FrameAddress,
    previous_top_call_frame: Option<FrameAddress>,
    previous_top_entry_frame: Option<FrameAddress>,
    storage_ordinal: u64,
    _storage: PhantomData<&'storage JscEntryFrameStorageRecord>,
}

#[allow(dead_code)]
impl<'storage> VmPublishedEntryFrame<'storage> {
    fn from_storage_record(record: &'storage JscEntryFrameStorageRecord) -> Self {
        Self {
            entry: record.entry,
            previous_entry_frame: record.previous_entry_frame,
            saved_top_call_frame: record.saved_top_call_frame,
            address: record.entry_frame_address(),
            previous_top_call_frame: record.vm_entry_record.previous_top_call_frame(),
            previous_top_entry_frame: record.vm_entry_record.previous_top_entry_frame(),
            storage_ordinal: record.ordinal,
            _storage: PhantomData,
        }
    }

    pub(crate) fn entry(self) -> EntryFrameId {
        self.entry
    }

    pub(crate) fn previous_entry_frame(self) -> Option<EntryFrameId> {
        self.previous_entry_frame
    }

    pub(crate) fn saved_top_call_frame(self) -> Option<CallFrameId> {
        self.saved_top_call_frame
    }

    pub(crate) fn address(self) -> FrameAddress {
        self.address
    }

    pub(crate) fn previous_top_call_frame(self) -> Option<FrameAddress> {
        self.previous_top_call_frame
    }

    pub(crate) fn previous_top_entry_frame(self) -> Option<FrameAddress> {
        self.previous_top_entry_frame
    }
}

fn option_frame_address_to_raw(address: Option<FrameAddress>) -> usize {
    address.map_or(0, |address| address.0)
}

fn raw_frame_address_to_option(address: usize) -> Option<FrameAddress> {
    if address == 0 {
        None
    } else {
        Some(FrameAddress(address))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_snapshots_symbolic_entry_and_previous_top_pair() {
        let registration = entry_registration(
            EntryFrameId(7),
            Some(EntryFrameId(6)),
            Some(CallFrameId(11)),
            Some(FrameAddress(0x1000)),
            Some(FrameAddress(0x2000)),
        );
        let mut storage = JscEntryFrameStorage::default();

        let handle = storage.register_entry_frame(registration);
        let record = storage.record(handle).expect("registered entry record");

        assert_eq!(handle.entry(), registration.entry);
        assert_eq!(record.entry, registration.entry);
        assert_eq!(
            record.previous_entry_frame,
            registration.previous_entry_frame
        );
        assert_eq!(
            record.saved_top_call_frame,
            registration.saved_top_call_frame
        );
        assert_eq!(
            record.vm_entry_record.previous_top_call_frame(),
            registration.previous_top_call_frame
        );
        assert_eq!(
            record.vm_entry_record.previous_top_entry_frame(),
            registration.previous_top_entry_frame
        );
    }

    #[test]
    fn active_handle_yields_storage_derived_entry_frame_proof() {
        let registration = entry_registration(
            EntryFrameId(8),
            Some(EntryFrameId(5)),
            Some(CallFrameId(12)),
            Some(FrameAddress(0x3000)),
            Some(FrameAddress(0x4000)),
        );
        let mut storage = JscEntryFrameStorage::default();
        let handle = storage.register_entry_frame(registration);

        let published = storage
            .published_entry_frame(handle)
            .expect("published entry-frame proof");

        assert_eq!(published.entry(), registration.entry);
        assert_eq!(
            published.previous_entry_frame(),
            registration.previous_entry_frame
        );
        assert_eq!(
            published.saved_top_call_frame(),
            registration.saved_top_call_frame
        );
        assert_eq!(
            published.address(),
            storage.entry_frame_address(handle).unwrap()
        );
        assert_eq!(
            published.previous_top_call_frame(),
            registration.previous_top_call_frame
        );
        assert_eq!(
            published.previous_top_entry_frame(),
            registration.previous_top_entry_frame
        );
    }

    #[test]
    fn retired_handle_rejects_record_address_and_publication_proof() {
        let registration = entry_registration(
            EntryFrameId(9),
            Some(EntryFrameId(4)),
            Some(CallFrameId(13)),
            Some(FrameAddress(0x5000)),
            Some(FrameAddress(0x6000)),
        );
        let mut storage = JscEntryFrameStorage::default();
        let handle = storage.register_entry_frame(registration);
        assert!(storage.entry_frame_address(handle).is_some());
        assert!(storage.published_entry_frame(handle).is_some());

        assert!(storage.retire(handle));
        assert_eq!(storage.record(handle), None);
        assert_eq!(storage.entry_frame_address(handle), None);
        assert_eq!(storage.published_entry_frame(handle), None);
        assert!(!storage.retire(handle));
    }

    #[test]
    fn registered_entry_frame_address_stays_stable_across_storage_growth() {
        let mut storage = JscEntryFrameStorage::default();
        let first_handle = storage.register_entry_frame(entry_registration(
            EntryFrameId(1),
            None,
            None,
            None,
            None,
        ));
        let first_address = storage
            .entry_frame_address(first_handle)
            .expect("first entry-frame address");

        for index in 2..128 {
            storage.register_entry_frame(entry_registration(
                EntryFrameId(index),
                Some(EntryFrameId(index - 1)),
                Some(CallFrameId(index + 100)),
                Some(FrameAddress(0x1000 + index as usize * 0x10)),
                Some(FrameAddress(0x2000 + index as usize * 0x10)),
            ));
        }

        assert_eq!(storage.len(), 127);
        assert_eq!(
            storage.entry_frame_address(first_handle),
            Some(first_address)
        );
    }

    #[test]
    fn storage_api_rejects_fabricated_handles_and_keeps_raw_values_separate() {
        let registration = entry_registration(
            EntryFrameId(10),
            Some(EntryFrameId(3)),
            Some(CallFrameId(14)),
            Some(FrameAddress(0x7000)),
            Some(FrameAddress(0x8000)),
        );
        let mut storage = JscEntryFrameStorage::default();
        let handle = storage.register_entry_frame(registration);
        let stored_address = storage
            .entry_frame_address(handle)
            .expect("stored entry-frame address");

        assert_ne!(stored_address, FrameAddress(registration.entry.0 as usize));
        assert_ne!(
            stored_address,
            registration.previous_top_call_frame.unwrap()
        );
        assert_ne!(
            stored_address,
            registration.previous_top_entry_frame.unwrap()
        );

        let symbolic_id_handle = JscEntryFrameStorageHandle {
            ordinal: registration.entry.0 as u64,
            entry: registration.entry,
            entry_frame: NonNull::dangling(),
        };
        let raw_top_call_frame_handle = JscEntryFrameStorageHandle {
            ordinal: registration.previous_top_call_frame.unwrap().0 as u64,
            entry: EntryFrameId(registration.previous_top_call_frame.unwrap().0 as u32),
            entry_frame: NonNull::dangling(),
        };
        let fabricated = JscEntryFrameStorageHandle {
            ordinal: handle.ordinal.saturating_add(1),
            entry: registration.entry,
            entry_frame: NonNull::dangling(),
        };
        assert_eq!(storage.entry_frame_address(symbolic_id_handle), None);
        assert_eq!(storage.published_entry_frame(symbolic_id_handle), None);
        assert_eq!(storage.entry_frame_address(raw_top_call_frame_handle), None);
        assert_eq!(
            storage.published_entry_frame(raw_top_call_frame_handle),
            None
        );
        assert_eq!(storage.entry_frame_address(fabricated), None);
        assert_eq!(storage.published_entry_frame(fabricated), None);
    }

    fn entry_registration(
        entry: EntryFrameId,
        previous_entry_frame: Option<EntryFrameId>,
        saved_top_call_frame: Option<CallFrameId>,
        previous_top_call_frame: Option<FrameAddress>,
        previous_top_entry_frame: Option<FrameAddress>,
    ) -> JscEntryFrameRegistration {
        JscEntryFrameRegistration {
            entry,
            previous_entry_frame,
            saved_top_call_frame,
            previous_top_call_frame,
            previous_top_entry_frame,
        }
    }
}
