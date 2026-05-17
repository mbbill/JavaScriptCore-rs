//! VM coordination skeleton.
//!
//! `Vm` owns one engine instance's heap, roots, entry bookkeeping, exception
//! state, runtime structures, caches, and host services. It is the coordinator
//! for execution and GC, not a global convenience object.

#![deny(unsafe_op_in_unsafe_fn)]

mod config;
mod entry;
mod exceptions;
mod runtime;

use crate::gc::Heap;

pub use self::runtime::{
    GlobalObjectId, GlobalObjectRecord, GlobalRuntimeState, RuntimeCaches, RuntimeStructures,
    VmServices,
};
pub use config::{HeapPolicy, HostCapabilities, VmConfig};
pub use entry::{EntryKind, FrameAddress, VmEntryGuard, VmEntryState};
pub use exceptions::{
    ExceptionCheckState, ExceptionScopeId, ExceptionState, PendingException, TerminationReason,
};

/// Engine instance and owner of heap-wide runtime state.
#[derive(Debug)]
pub struct Vm {
    config: VmConfig,
    heap: Heap,
    entry: VmEntryState,
    exceptions: ExceptionState,
    structures: RuntimeStructures,
    caches: RuntimeCaches,
    globals: GlobalRuntimeState,
    services: VmServices,
}

impl Vm {
    pub fn new(config: VmConfig) -> Self {
        Self {
            config,
            heap: Heap::new(),
            entry: VmEntryState::default(),
            exceptions: ExceptionState::default(),
            structures: RuntimeStructures::default(),
            caches: RuntimeCaches::default(),
            globals: GlobalRuntimeState::default(),
            services: VmServices::default(),
        }
    }

    pub fn config(&self) -> &VmConfig {
        &self.config
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn heap_access(&mut self) -> HeapAccessToken<'_> {
        HeapAccessToken {
            heap: &mut self.heap,
        }
    }

    pub fn entry_state(&self) -> &VmEntryState {
        &self.entry
    }

    pub fn enter(&mut self, top_frame: Option<FrameAddress>, kind: EntryKind) -> VmEntryGuard<'_> {
        self.entry.enter(top_frame, kind)
    }

    pub fn exception_state(&self) -> &ExceptionState {
        &self.exceptions
    }

    pub fn exception_state_mut(&mut self) -> &mut ExceptionState {
        &mut self.exceptions
    }

    pub fn runtime_structures(&self) -> &RuntimeStructures {
        &self.structures
    }

    pub fn runtime_caches(&self) -> &RuntimeCaches {
        &self.caches
    }

    pub fn global_runtime_state(&self) -> &GlobalRuntimeState {
        &self.globals
    }

    pub fn global_runtime_state_mut(&mut self) -> &mut GlobalRuntimeState {
        &mut self.globals
    }

    pub fn services(&self) -> &VmServices {
        &self.services
    }
}

/// Explicit proof that the mutator may allocate or inspect GC cells.
pub struct HeapAccessToken<'vm> {
    heap: &'vm mut Heap,
}

impl<'vm> HeapAccessToken<'vm> {
    pub fn heap(&self) -> &Heap {
        self.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        self.heap
    }
}
