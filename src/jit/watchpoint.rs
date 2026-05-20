//! Optimized-code invalidation dependencies.
//!
//! Watchpoints are correctness mechanisms. Future implementations must trace,
//! fire, or invalidate dependencies according to object, structure, code-block,
//! and GC contracts. This skeleton only records the dependency graph; it never
//! observes or mutates live runtime state.

use crate::gc::StructureId;
use crate::object::PropertyOffset;
use crate::runtime::{CodeBlockId, ObjectId, WatchpointGeneration};
use crate::strings::PropertyKey;

/// Stable identity for a watchpoint dependency in code side data.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WatchpointDependencyId(pub u64);

/// Ownership and tracing policy for a dependency.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyStrength {
    /// The compiled artifact owns a traced GC edge while executable code exists.
    StrongGc,
    /// The compiled artifact can be cleared if the target dies during GC.
    WeakGc,
    /// A non-GC subsystem is responsible for firing the invalidation edge.
    ExternalInvalidation,
    /// The edge is sampled during compilation and rechecked before install.
    CompileTimeAssumption,
}

/// Opaque invalidation edge reserved for future optimized code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatchpointDependency {
    pub id: WatchpointDependencyId,
    pub strength: DependencyStrength,
    pub target: WatchpointTarget,
    pub generation: Option<WatchpointGeneration>,
}

/// Stable identity for a future watchpoint set.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WatchpointSetId(pub u64);

/// Snapshot of a watchpoint set state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchpointSetState {
    Clear,
    Watched,
    Invalidated,
    DeferredFire,
}

/// Runtime owner of a watchpoint set or invalidation edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchpointOwner {
    CodeBlock(CodeBlockId),
    Object(ObjectId),
    Structure(StructureId),
    InlineCache(crate::jit::InlineCacheSlotId),
    SharedStub(crate::jit::JitCodeId),
    External,
}

/// Specific condition a compiled artifact or IC stub depends on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchpointTarget {
    StructureTransition {
        structure: StructureId,
    },
    PropertyReplacement {
        object: ObjectId,
        property: PropertyKey,
        offset: PropertyOffset,
    },
    PrototypeChain {
        base: ObjectId,
    },
    GlobalProperty {
        global: ObjectId,
        property: PropertyKey,
    },
    CodeBlockJettison {
        code_block: CodeBlockId,
    },
    InlineCacheReset {
        slot: crate::jit::InlineCacheSlotId,
    },
    WasmInstanceInvalidation {
        instance_raw_id: u64,
    },
    External,
}

/// Fire policy for a future watchpoint installation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchpointFirePolicy {
    FireImmediatelyIfWatched,
    MarkInvalidatedOnly,
    DeferUntilSafepoint,
    RecheckBeforeInstall,
}

/// Reserved watchpoint set metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchpointSetDescriptor {
    pub id: WatchpointSetId,
    pub owner: WatchpointOwner,
    pub state: WatchpointSetState,
    pub fire_policy: WatchpointFirePolicy,
    pub dependencies: Vec<WatchpointDependencyId>,
}

/// A pending watchpoint event. It is data only; dispatch belongs to VM code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatchpointFireEvent {
    pub set: WatchpointSetId,
    pub target: WatchpointTarget,
    pub generation: WatchpointGeneration,
}
