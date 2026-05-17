//! Inline-cache attachment points.
//!
//! These types name where future property, call, construct, and global caches
//! attach. They preserve JSC's split between handler-list ICs, repatching ICs,
//! call link metadata, access cases, and GC-aware stubs without defining cache
//! probes, shape checks, patching, or stub generation.

use crate::jit::{
    CallBoundaryId, DependencyStrength, JitCodeId, JitType, WatchpointDependency,
    WatchpointDependencyId,
};
use crate::object::{AtomId, PrivateNameId, PropertyOffset, StructureId, SymbolId};
use crate::runtime::{CodeBlockId, ExecutableId, ObjectId};

/// Stable identity for an inline-cache slot within linked code state.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineCacheSlotId(pub u32);

/// Kind of runtime operation an inline-cache slot may later accelerate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheKind {
    PropertyLoad,
    PropertyStore,
    ElementLoad,
    ElementStore,
    GlobalLoad,
    GlobalStore,
    Call,
    Construct,
    Delete,
    HasProperty,
    InstanceOf,
    PrivateBrand,
}

/// Reserved cache attachment point owned by code-block-equivalent state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCacheSlot {
    pub id: InlineCacheSlotId,
    pub kind: InlineCacheKind,
    pub owner: Option<CodeBlockId>,
    pub bytecode_index: Option<u32>,
    pub state: InlineCacheState,
    pub dispatch: InlineCacheDispatch,
    pub cases: Vec<AccessCaseDescriptor>,
    pub stubs: Vec<InlineCacheStub>,
    pub watchpoints: Vec<WatchpointDependencyId>,
}

/// Runtime state of an IC attachment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheState {
    Uninitialized,
    ColdSlowPath,
    Monomorphic,
    Polymorphic,
    Megamorphic,
    Resetting,
    Disabled,
}

/// Dispatch strategy reserved for the future generated site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheDispatch {
    DataOnlyHandlerChain,
    RepatchingSlab,
    SharedStatelessStub,
    SlowPathOnly,
}

/// Property key category used by IC metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheKey {
    Atom(AtomId),
    Symbol(SymbolId),
    PrivateName(PrivateNameId),
    ArrayIndex(u32),
    Dynamic,
}

/// Access case family mirrored from JSC's `AccessCase` contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessCaseKind {
    Load,
    Transition,
    Replace,
    Delete,
    Miss,
    Getter,
    Setter,
    CustomAccessor,
    IntrinsicGetter,
    ArrayLength,
    StringLength,
    ModuleNamespaceLoad,
    ProxyObject,
    InstanceOf,
    IndexedLoad,
    IndexedStore,
    IndexedIn,
    Megamorphic,
}

/// Structure and property condition for one polymorphic access case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessCaseDescriptor {
    pub kind: AccessCaseKind,
    pub key: CacheKey,
    pub base_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub holder: Option<ObjectId>,
    pub offset: Option<PropertyOffset>,
    pub via_global_proxy: bool,
    pub may_call_js: bool,
    pub dependencies: Vec<WatchpointDependency>,
}

/// Stable identity for an IC stub or handler node.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineCacheStubId(pub u64);

/// Kind of stub storage an IC metadata node describes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheStubKind {
    SlowPathHandler,
    DataOnlyHandler,
    HandlerWithCallLinkInfo,
    PolymorphicAccessStub,
    SharedStatelessStub,
    RepatchingStub,
}

/// Metadata for a generated or reserved IC stub.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCacheStub {
    pub id: InlineCacheStubId,
    pub kind: InlineCacheStubKind,
    pub owner_slot: InlineCacheSlotId,
    pub code: Option<JitCodeId>,
    pub tier: JitType,
    pub cases: Vec<AccessCaseDescriptor>,
    pub weak_structures: Vec<StructureId>,
    pub call_links: Vec<CallLinkInfoDescriptor>,
    pub invalidation_strength: DependencyStrength,
}

/// Call-link mode for call ICs and IC stubs that may call JS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallLinkMode {
    Init,
    Monomorphic,
    Polymorphic,
    Virtual,
    Direct,
}

/// Call kind associated with a call-link descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkedCallKind {
    Call,
    Construct,
    TailCall,
    VarargsCall,
    VarargsConstruct,
}

/// Data-only call link metadata. Callees and code blocks are weak IDs because
/// GC/liveness ownership belongs to runtime code and generated-code finalizers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallLinkInfoDescriptor {
    pub mode: CallLinkMode,
    pub call_kind: LinkedCallKind,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub callee: Option<ObjectId>,
    pub target_code_block: Option<CodeBlockId>,
    pub boundary: Option<CallBoundaryId>,
    pub slow_path_count: u32,
    pub max_argument_count_including_this: u8,
}

/// IC reset request. The VM will later translate this into handler unlinking,
/// slab restoration, and watchpoint cleanup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCacheInvalidation {
    pub slot: InlineCacheSlotId,
    pub reason: InlineCacheInvalidationReason,
    pub affected_stubs: Vec<InlineCacheStubId>,
}

/// Reason an IC is no longer valid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineCacheInvalidationReason {
    WatchpointFired,
    OwnerCodeBlockDied,
    StubOwnerDied,
    WeakStructureDied,
    MegamorphicPolicy,
    ExplicitReset,
}
