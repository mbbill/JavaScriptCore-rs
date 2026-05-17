use std::marker::PhantomData;

pub use crate::bytecode::SourceProviderId;
pub use crate::gc::StructureId;
pub use crate::modules::ModuleRecordId;

/// Placeholder for the runtime value transport type until `value` exposes
/// its concrete `JsValue` contract.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeValue {
    raw_placeholder: usize,
}

impl RuntimeValue {
    pub const fn opaque(raw_placeholder: usize) -> Self {
        Self { raw_placeholder }
    }
}

/// Stable index for GC cells while the heap handle layer is still being designed.
///
/// Runtime contracts use typed identifiers instead of raw pointers so that later
/// GC integration can choose between handles, barriers, compressed pointers, or
/// table indexes without changing the high-level execution APIs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CellId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ObjectId(pub CellId);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ExecutableId(pub CellId);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeBlockId(pub CellId);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct NativeCodeId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct HostHookId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct StringId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct StackFrameId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct WatchpointGeneration(pub u64);

/// A write edge from one GC-managed owner to another.
///
/// This deliberately stores only metadata. The eventual GC layer owns the
/// barrier operation and rooting policy; runtime structures state which owner
/// would be responsible for the edge.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BarrieredCell<T> {
    pub owner: Option<CellId>,
    pub target: Option<CellId>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> BarrieredCell<T> {
    pub const fn empty(owner: Option<CellId>) -> Self {
        Self {
            owner,
            target: None,
            _marker: PhantomData,
        }
    }

    pub const fn with_target(owner: CellId, target: CellId) -> Self {
        Self {
            owner: Some(owner),
            target: Some(target),
            _marker: PhantomData,
        }
    }
}

/// Canonical runtime structures shared by a VM or realm.
///
/// Stored references must be rooted or barriered by the owning VM/heap once GC
/// contracts are available.
#[derive(Clone, Debug, Default)]
pub struct RuntimeStructures {
    pub executable_structure: Option<StructureId>,
    pub function_structure: Option<StructureId>,
    pub scope_structure: Option<StructureId>,
    pub exception_structure: Option<StructureId>,
}

/// VM-wide caches for strings, executables, structures, and services.
#[derive(Clone, Debug, Default)]
pub struct RuntimeCaches {
    pub generation: u64,
    pub string_table_generation: u64,
    pub structure_cache_generation: u64,
    pub executable_cache_generation: u64,
    pub code_block_generation: u64,
}

/// Host services that may reenter the engine.
///
/// FFI callbacks, timers, watchdogs, and microtask hooks are unsafe boundaries
/// because they can observe VM state and trigger GC.
#[derive(Clone, Debug, Default)]
pub struct VmServices {
    pub has_microtask_hook: bool,
    pub has_watchdog: bool,
    pub module_loader_hook: Option<HostHookId>,
    pub promise_rejection_tracker_hook: Option<HostHookId>,
    pub uncaught_exception_reporter_hook: Option<HostHookId>,
    pub script_interrupt_hook: Option<HostHookId>,
}
