use std::marker::PhantomData;

pub use crate::bytecode::SourceProviderId;
pub use crate::gc::{CellId, StructureId};
pub use crate::modules::ModuleRecordId;
pub use crate::strings::StringId;
/// Runtime-facing value representation.
///
/// `value::JsValue` owns the bit-level representation. Runtime code imports
/// this alias to describe API boundaries without creating a second value type.
pub use crate::value::JsValue as RuntimeValue;

/// Runtime object identity. The raw cell identity is owned by `gc`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ObjectId(pub CellId);

/// Runtime executable identity. The raw heap-cell identity is owned by `gc`;
/// executable/runtime code only carries the typed handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ExecutableId(pub CellId);

/// Runtime code-block identity. The raw heap-cell identity is owned by `gc`;
/// bytecode/compiler layers may borrow this typed handle but must not mint a
/// parallel cell identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeBlockId(pub CellId);

/// Native code thunk identity owned by the runtime/host integration layer.
///
/// This is not a GC cell identity and must not be widened to `CellId`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct NativeCodeId(pub u32);

/// Host hook identity owned by VM services.
///
/// Runtime records borrow this handle to name callbacks; host/VM integration
/// owns callback lifetime, reentrancy, and mutation authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct HostHookId(pub u32);

/// Runtime JS Symbol cell identity.
///
/// This names a GC/runtime symbol cell. It is intentionally separate from
/// `strings::SymbolUid`, which owns symbol property-name identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SymbolCellId(pub CellId);

/// VM stack-frame identity owned by interpreter/entry state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct StackFrameId(pub u32);

/// Monotonic watchpoint generation owned by the invalidation authority that
/// guards the corresponding structure, scope, or cache.
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
