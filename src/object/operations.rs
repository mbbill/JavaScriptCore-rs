//! Object internal-method contracts.
//!
//! The runtime implementation will live behind VM-aware code that can allocate,
//! throw, enter host callbacks, and run GC. This module names those operations
//! and their observable boundaries without performing property lookup.

use crate::gc::StructureId;
use crate::value::JsValue;

use super::property::{
    PrivateNameId, PropertyAttributes, PropertyDescriptor, PropertyKey, PropertyLookupMode,
    PropertyOffset, PropertySlot,
};

/// ECMAScript or JSC internal method being performed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectInternalMethodKind {
    Get,
    GetOwnProperty,
    HasProperty,
    Put,
    Delete,
    DefineOwnProperty,
    GetPrototypeOf,
    SetPrototypeOf,
    OwnPropertyKeys,
    IsExtensible,
    PreventExtensions,
}

/// Object family whose method table may override ordinary object behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExoticObjectKind {
    Ordinary,
    Array,
    StringObject,
    TypedArray,
    DataView,
    ModuleNamespace,
    Arguments,
    Proxy,
    GlobalObject,
    HostObject,
}

/// Method-table capability flags used by caches before calling slow paths.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObjectMethodTableCapabilities {
    pub kind: Option<ExoticObjectKind>,
    pub overrides_get_own_property_slot: bool,
    pub overrides_put: bool,
    pub overrides_delete: bool,
    pub overrides_define_own_property: bool,
    pub overrides_own_property_keys: bool,
    pub may_intercept_indexed_access: bool,
    pub get_own_property_slot_is_impure: bool,
}

/// Receiver used for accessor calls and `Reflect.get`/`super` semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyReceiver {
    value: JsValue,
}

impl PropertyReceiver {
    pub const fn new(value: JsValue) -> Self {
        Self { value }
    }

    pub const fn value(self) -> JsValue {
        self.value
    }
}

/// Shared context for object internal methods.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectInternalMethodContext {
    pub method: ObjectInternalMethodKind,
    pub receiver: PropertyReceiver,
    pub lookup_mode: PropertyLookupMode,
    pub strict: bool,
    pub may_throw: bool,
    pub is_initialization: bool,
    pub allow_user_observable_side_effects: bool,
}

impl ObjectInternalMethodContext {
    pub const fn vm_inquiry(receiver: PropertyReceiver) -> Self {
        Self {
            method: ObjectInternalMethodKind::GetOwnProperty,
            receiver,
            lookup_mode: PropertyLookupMode::VmInquiry,
            strict: false,
            may_throw: false,
            is_initialization: false,
            allow_user_observable_side_effects: false,
        }
    }
}

/// Mutation context mirrored from `PutPropertySlot`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyMutationContext {
    pub receiver: PropertyReceiver,
    pub strict: bool,
    pub initialization: bool,
    pub define_own_semantics: bool,
    pub cacheable: bool,
}

/// Policy for `[[DefineOwnProperty]]` and direct put helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyDefinitionPolicy {
    OrdinaryDefineOwn,
    DirectDataProperty,
    ArrayLength,
    IntegerIndexedElement,
    ReifyStaticProperty,
}

/// Result envelope for object operations that may throw later.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectOperationResult<T> {
    Completed(T),
    Rejected(ObjectOperationError),
}

/// Non-exceptional reason an operation cannot complete in this layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectOperationError {
    NonExtensible,
    NonConfigurable,
    ReadOnly,
    AccessorWithoutSetter,
    InvalidPrivateBrand,
    DetachedTypedArrayBuffer,
    OutOfBoundsIntegerIndexedElement,
    SideEffectsDisallowed,
    PrototypeCycle,
    TrapRejected,
}

/// Contract result for `[[Get]]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GetPropertyOutcome {
    Found(PropertySlot),
    Missing,
}

/// Contract result for `[[HasProperty]]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HasPropertyOutcome {
    Present,
    Missing,
}

/// Contract result for `[[Set]]`/put.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PutPropertyOutcome {
    Stored {
        base_structure: StructureId,
        offset: PropertyOffset,
    },
    Created {
        new_structure: StructureId,
        offset: PropertyOffset,
    },
    CalledSetter,
    CalledCustomAccessor,
    IgnoredSloppyFailure,
}

/// Contract result for `[[Delete]]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletePropertyOutcome {
    Deleted,
    Missing,
    Rejected,
}

/// Contract result for `[[DefineOwnProperty]]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefineOwnPropertyOutcome {
    DefinedData {
        offset: PropertyOffset,
        attributes: PropertyAttributes,
    },
    DefinedAccessor {
        offset: PropertyOffset,
        attributes: PropertyAttributes,
    },
    Reconfigured {
        offset: PropertyOffset,
        attributes: PropertyAttributes,
    },
    Rejected,
}

/// One step in a prototype-chain traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrototypeTraversalStep {
    pub structure: StructureId,
    pub action: PrototypeTraversalAction,
}

/// Action selected for a prototype traversal step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrototypeTraversalAction {
    CheckOwnProperties,
    InvokeExoticGetOwnProperty,
    StopAtNullPrototype,
    RestartForIndexedLookup,
    RejectCycle,
}

/// Planned prototype traversal for cache analysis or diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrototypeTraversalPlan {
    steps: Vec<PrototypeTraversalStep>,
    cacheable_until_structure: Option<StructureId>,
}

impl PrototypeTraversalPlan {
    pub fn steps(&self) -> &[PrototypeTraversalStep] {
        &self.steps
    }

    pub fn cacheable_until_structure(&self) -> Option<StructureId> {
        self.cacheable_until_structure
    }

    pub fn push_step(&mut self, step: PrototypeTraversalStep) {
        self.steps.push(step);
    }

    pub fn set_cacheable_until_structure(&mut self, structure: StructureId) {
        self.cacheable_until_structure = Some(structure);
    }
}

/// Ordinary lookup plan before invoking an exotic hook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryPropertyLookupPlan {
    pub base_structure: StructureId,
    pub key: PropertyKey,
    pub mode: PropertyLookupMode,
    pub check_indexed_storage_first: bool,
    pub prototype_traversal: PrototypeTraversalPlan,
}

/// Private-brand requirement for private fields and methods.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateBrandRequirement {
    pub brand: PrivateNameId,
    pub allow_static_brand: bool,
}

/// Brand check request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateBrandCheck {
    pub requirement: PrivateBrandRequirement,
    pub receiver: PropertyReceiver,
}

/// Result of a private-brand check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateBrandCheckResult {
    Present,
    Missing,
    WrongReceiver,
}

/// Dynamic object behavior supplied by class info or exotic objects.
pub trait ObjectInternalMethodHooks {
    fn capabilities(&self) -> ObjectMethodTableCapabilities;

    fn get_own_property(
        &self,
        key: PropertyKey,
        context: ObjectInternalMethodContext,
    ) -> ObjectOperationResult<GetPropertyOutcome>;

    fn put(
        &self,
        key: PropertyKey,
        value: JsValue,
        context: PropertyMutationContext,
    ) -> ObjectOperationResult<PutPropertyOutcome>;

    fn delete_property(
        &self,
        key: PropertyKey,
        context: ObjectInternalMethodContext,
    ) -> ObjectOperationResult<DeletePropertyOutcome>;

    fn define_own_property(
        &self,
        key: PropertyKey,
        descriptor: PropertyDescriptor,
        policy: PropertyDefinitionPolicy,
        context: ObjectInternalMethodContext,
    ) -> ObjectOperationResult<DefineOwnPropertyOutcome>;
}
