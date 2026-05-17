use std::marker::PhantomData;

use crate::runtime::scope::{GlobalLexicalEnvironment, ScopeId};
use crate::runtime::state::{HostHookId, ObjectId, StringId, StructureId, WatchpointGeneration};

/// Logical realm root.
///
/// A realm owns or indexes global state, intrinsic tables, structures,
/// watchpoints, and host hooks. Cyclic intrinsic initialization must be staged
/// before publication.
#[derive(Clone, Debug, Default)]
pub struct Realm {
    pub id: RealmId,
    pub lifecycle: RealmLifecycleState,
    pub global_object: Option<GlobalObjectId>,
    pub global_this: Option<GlobalThis>,
    pub global_lexical_environment: Option<GlobalLexicalEnvironment>,
    pub structures: RealmStructures,
    pub intrinsics: Intrinsics,
    pub watchpoints: RealmWatchpoints,
    pub hooks: HostRealmHooks,
    pub microtasks: RealmMicrotaskState,
}

/// GC-managed global object and realm root contract.
#[derive(Clone, Debug, Default)]
pub struct GlobalObject {
    /// Realm root object visible to scripts and host callbacks.
    ///
    /// It owns the global lexical scope edge, canonical structures, and host
    /// hook table. Initialization is staged so cyclic builtin construction can
    /// complete before the realm is published.
    pub id: Option<GlobalObjectId>,
    pub realm_id: RealmId,
    pub global_this: Option<ObjectId>,
    pub global_scope: Option<ScopeId>,
    pub global_lexical_scope: Option<ScopeId>,
    pub structures: RealmStructures,
    pub host_hooks: HostRealmHooks,
    pub lifecycle: GlobalObjectLifecycle,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct RealmId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct GlobalObjectId(pub ObjectId);

#[derive(Clone, Debug, Default)]
pub struct GlobalThis {
    pub realm_id: RealmId,
    pub object: Option<ObjectId>,
    pub proxy: Option<ObjectId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum RealmLifecycleState {
    #[default]
    Allocated,
    IntrinsicsInitializing,
    HostInitializing,
    Published,
    TearingDown,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum GlobalObjectLifecycle {
    #[default]
    Allocated,
    StructuresInstalling,
    BuiltinsInstalling,
    Ready,
    Detached,
}

#[derive(Clone, Debug, Default)]
pub struct Intrinsics {
    pub slots_reserved: usize,
    pub object_prototype: LazyIntrinsic<ObjectId>,
    pub function_prototype: LazyIntrinsic<ObjectId>,
    pub array_prototype: LazyIntrinsic<ObjectId>,
    pub promise_constructor: LazyIntrinsic<ObjectId>,
    pub module_loader: LazyIntrinsic<ObjectId>,
    pub throw_type_error_function: LazyIntrinsic<ObjectId>,
}

/// Barriered intrinsic slot.
///
/// Writes after publication require owner-aware GC barriers. Initialization
/// before escape may use a future cell-initialization API.
#[derive(Clone, Debug)]
pub struct IntrinsicSlot<T> {
    pub owner_realm: RealmId,
    pub state: IntrinsicState,
    pub initialization_epoch: u64,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Default for IntrinsicSlot<T> {
    fn default() -> Self {
        Self {
            owner_realm: RealmId::default(),
            state: IntrinsicState::default(),
            initialization_epoch: 0,
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub enum LazyIntrinsic<T> {
    #[default]
    Uninitialized,
    Initializing,
    Initialized(IntrinsicSlot<T>),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum IntrinsicState {
    #[default]
    Reserved,
    Initializing,
    Initialized,
    Published,
}

#[derive(Clone, Debug, Default)]
pub struct RealmStructures {
    /// Canonical structures installed per realm/global object.
    ///
    /// Structure IDs are placeholders for future GC-managed structures and
    /// watchpoint ownership. They are not allocation paths in this module.
    pub canonical_structure_count: usize,
    pub object_structure: Option<StructureId>,
    pub function_structure: Option<StructureId>,
    pub host_function_structure: Option<StructureId>,
    pub lexical_environment_structure: Option<StructureId>,
    pub module_environment_structure: Option<StructureId>,
    pub error_structures: ErrorStructures,
}

#[derive(Clone, Debug, Default)]
pub struct ErrorStructures {
    pub error: Option<StructureId>,
    pub eval_error: Option<StructureId>,
    pub range_error: Option<StructureId>,
    pub reference_error: Option<StructureId>,
    pub syntax_error: Option<StructureId>,
    pub type_error: Option<StructureId>,
    pub uri_error: Option<StructureId>,
    pub aggregate_error: Option<StructureId>,
}

#[derive(Clone, Debug, Default)]
pub struct RealmWatchpoints {
    pub invalidation_generation: u64,
    pub array_structure_generation: WatchpointGeneration,
    pub function_structure_generation: WatchpointGeneration,
    pub global_property_generation: WatchpointGeneration,
    pub module_namespace_generation: WatchpointGeneration,
}

/// Host callbacks for module loading, promises, and embedding integration.
#[derive(Clone, Debug, Default)]
pub struct HostRealmHooks {
    /// Host integration points corresponding to `GlobalObjectMethodTable`.
    ///
    /// Each hook is an identifier, not a callback pointer. The embedding layer
    /// owns FFI lifetimes, reentrancy policy, and exception translation.
    pub can_reenter_vm: bool,
    pub supports_rich_source_info: Option<HostHookId>,
    pub should_interrupt_script: Option<HostHookId>,
    pub module_loader_import: Option<HostHookId>,
    pub module_loader_resolve: Option<HostHookId>,
    pub module_loader_fetch: Option<HostHookId>,
    pub module_loader_evaluate: Option<HostHookId>,
    pub promise_rejection_tracker: Option<HostHookId>,
    pub uncaught_exception_reporter: Option<HostHookId>,
    pub current_script_execution_owner: Option<HostHookId>,
    pub code_for_eval: Option<HostHookId>,
    pub can_compile_strings: Option<HostHookId>,
    pub default_language: Option<StringId>,
}

#[derive(Clone, Debug, Default)]
pub struct RealmMicrotaskState {
    pub queue_generation: u64,
    pub runnability: MicrotaskRunnability,
    pub has_pending_checkpoint: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MicrotaskRunnability {
    #[default]
    Runnable,
    Suspended,
    Stopped,
}
