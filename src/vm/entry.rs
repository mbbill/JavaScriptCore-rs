//! VM entry and top-frame bookkeeping.
//!
//! Call frames are not owned by `Vm`; this module records frame addresses that
//! interpreter, generated code, debugger, and GC integration may need to see.

use core::marker::PhantomData;

/// Opaque stack/frame address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct FrameAddress(pub usize);

/// VM entry reason. It controls whether reentry and user-observable work are allowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Script,
    HostCall,
    Microtask,
    Debugger,
    VmInquiry,
}

/// Active entry-frame and top-frame bookkeeping.
#[derive(Clone, Debug, Default)]
pub struct VmEntryState {
    entry_depth: usize,
    entry_frame: Option<FrameAddress>,
    top_frame: Option<FrameAddress>,
    kind: Option<EntryKind>,
    disallow_user_observable_work: bool,
}

impl VmEntryState {
    pub fn entry_depth(&self) -> usize {
        self.entry_depth
    }

    pub fn top_frame(&self) -> Option<FrameAddress> {
        self.top_frame
    }

    pub fn kind(&self) -> Option<EntryKind> {
        self.kind
    }

    pub fn disallows_user_observable_work(&self) -> bool {
        self.disallow_user_observable_work
    }

    pub fn enter(&mut self, top_frame: Option<FrameAddress>, kind: EntryKind) -> VmEntryGuard<'_> {
        let previous_top_frame = self.top_frame;
        let previous_kind = self.kind;
        let previous_disallow = self.disallow_user_observable_work;
        if self.entry_depth == 0 {
            self.entry_frame = top_frame;
        }
        self.entry_depth += 1;
        self.top_frame = top_frame;
        self.kind = Some(kind);
        self.disallow_user_observable_work = matches!(kind, EntryKind::VmInquiry);
        VmEntryGuard {
            state: self,
            previous_top_frame,
            previous_kind,
            previous_disallow,
            _borrow: PhantomData,
        }
    }
}

/// Scoped VM entry guard.
///
/// The raw pointer is an ABI-boundary placeholder for future interpreter/JIT
/// entry code. It is not exposed publicly.
pub struct VmEntryGuard<'vm> {
    state: &'vm mut VmEntryState,
    previous_top_frame: Option<FrameAddress>,
    previous_kind: Option<EntryKind>,
    previous_disallow: bool,
    _borrow: PhantomData<&'vm mut VmEntryState>,
}

impl Drop for VmEntryGuard<'_> {
    fn drop(&mut self) {
        self.state.entry_depth = self.state.entry_depth.saturating_sub(1);
        self.state.top_frame = self.previous_top_frame;
        self.state.kind = self.previous_kind;
        self.state.disallow_user_observable_work = self.previous_disallow;
        if self.state.entry_depth == 0 {
            self.state.entry_frame = None;
        }
    }
}
