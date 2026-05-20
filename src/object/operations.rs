//! Object internal-method contracts.
//!
//! The runtime implementation will live behind VM-aware code that can allocate,
//! throw, enter host callbacks, and run GC. This module names those operations
//! and their observable boundaries without performing property lookup.

use crate::gc::{
    static_barrier_schema_registry, BarrierDecisionError, BarrierFieldKind,
    BarrierMutationAuthority, BarrierRequirementOutcome, BarrierRequirementRequest,
    BarrierWriteContext, CellId, CellState, HeapSemanticOperation, StructureId,
};
use crate::strings::PrivateName;
use crate::value::JsValue;

use super::property::{
    validate_property_descriptor, PropertyAttributes, PropertyDescriptor, PropertyKey,
    PropertyLookupMode, PropertyOffset, PropertySlot, PropertyValueDescriptor,
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

/// Static owner of an object method-table descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectMethodTableOwner {
    OrdinaryObject,
    BuiltinObject,
    GlobalObject,
    ExoticObject,
    HostObject,
    GeneratedClassInfo,
}

/// Provenance for object method-table descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectMethodTableProvenance {
    RustSchema,
    EngineClassInfo,
    Ecma262InternalMethods,
    HostBinding,
    GeneratedStaticData,
}

/// Side-effect boundary advertised by a static object hook entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectMethodSideEffect {
    None,
    ReadsStorage,
    MayCallUserCode,
    HostDefined,
}

/// One immutable object method-table slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectMethodSlotDescriptor {
    pub method: ObjectInternalMethodKind,
    pub side_effect: ObjectMethodSideEffect,
    pub overridden: bool,
}

/// Immutable object method-table descriptor.
///
/// The table describes which internal methods are supplied by a class/exotic
/// family. It does not carry function pointers or invoke runtime hooks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectMethodTableDescriptor {
    pub name: &'static str,
    pub owner: ObjectMethodTableOwner,
    pub provenance: ObjectMethodTableProvenance,
    pub capabilities: ObjectMethodTableCapabilities,
    slots: &'static [ObjectMethodSlotDescriptor],
}

impl ObjectMethodTableDescriptor {
    pub const fn new(
        name: &'static str,
        owner: ObjectMethodTableOwner,
        provenance: ObjectMethodTableProvenance,
        capabilities: ObjectMethodTableCapabilities,
        slots: &'static [ObjectMethodSlotDescriptor],
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            capabilities,
            slots,
        }
    }

    /// Returns the immutable method slots for this class/exotic family.
    pub const fn slots(&self) -> &'static [ObjectMethodSlotDescriptor] {
        self.slots
    }

    /// Returns one existing static method slot by table index.
    pub const fn slot_at(&self, index: usize) -> Option<&'static ObjectMethodSlotDescriptor> {
        if index < self.slots.len() {
            Some(&self.slots[index])
        } else {
            None
        }
    }
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

/// Snapshot of ordinary object state required by semantic planners.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryObjectState {
    pub structure: StructureId,
    pub extensible: bool,
    pub own_properties: Vec<PropertyDescriptor>,
}

impl OrdinaryObjectState {
    pub fn new(structure: StructureId, extensible: bool) -> Self {
        Self {
            structure,
            extensible,
            own_properties: Vec::new(),
        }
    }

    pub fn with_properties(
        structure: StructureId,
        extensible: bool,
        own_properties: Vec<PropertyDescriptor>,
    ) -> Result<Self, ObjectOperationError> {
        for descriptor in &own_properties {
            validate_property_descriptor(descriptor)
                .map_err(|_| ObjectOperationError::TrapRejected)?;
        }
        Ok(Self {
            structure,
            extensible,
            own_properties,
        })
    }

    pub fn own_property(&self, key: PropertyKey) -> Option<&PropertyDescriptor> {
        self.own_properties
            .iter()
            .find(|descriptor| descriptor.key == key)
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPropertyOperationRecord<T> {
    pub operation: ExecutionPropertyOperation,
    pub key: PropertyKey,
    pub receiver: PropertyReceiver,
    pub gc_boundary: ObjectOperationGcBoundary,
    pub completion: ObjectOperationResult<T>,
}

impl<T> ExecutionPropertyOperationRecord<T> {
    pub fn is_throw_boundary(&self) -> bool {
        matches!(&self.completion, ObjectOperationResult::Rejected(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionPropertyOperation {
    GetOwn,
    HasOwn,
    Put,
    Delete,
    DefineOwn,
}

/// GC boundary advertised by an object semantic operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectOperationGcBoundary {
    pub heap_operation: HeapSemanticOperation,
    pub requires_no_gc_scope: bool,
}

impl ObjectOperationGcBoundary {
    pub const fn for_operation(operation: ExecutionPropertyOperation) -> Self {
        match operation {
            ExecutionPropertyOperation::GetOwn | ExecutionPropertyOperation::HasOwn => Self {
                heap_operation: HeapSemanticOperation::Observe,
                requires_no_gc_scope: true,
            },
            ExecutionPropertyOperation::Put
            | ExecutionPropertyOperation::Delete
            | ExecutionPropertyOperation::DefineOwn => Self {
                heap_operation: HeapSemanticOperation::MutatePublishedCell,
                requires_no_gc_scope: false,
            },
        }
    }
}

pub fn adapt_get_own_property_for_execution(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    context: ObjectInternalMethodContext,
) -> ExecutionPropertyOperationRecord<GetPropertyOutcome> {
    ExecutionPropertyOperationRecord {
        operation: ExecutionPropertyOperation::GetOwn,
        key,
        receiver: context.receiver,
        gc_boundary: ObjectOperationGcBoundary::for_operation(ExecutionPropertyOperation::GetOwn),
        completion: ordinary_get_own_property(state, key, context),
    }
}

pub fn adapt_has_own_property_for_execution(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    receiver: PropertyReceiver,
) -> ExecutionPropertyOperationRecord<HasPropertyOutcome> {
    ExecutionPropertyOperationRecord {
        operation: ExecutionPropertyOperation::HasOwn,
        key,
        receiver,
        gc_boundary: ObjectOperationGcBoundary::for_operation(ExecutionPropertyOperation::HasOwn),
        completion: ObjectOperationResult::Completed(ordinary_has_property(state, key)),
    }
}

pub fn adapt_put_property_for_execution(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    value: JsValue,
    context: PropertyMutationContext,
) -> ExecutionPropertyOperationRecord<PutPropertyOutcome> {
    ExecutionPropertyOperationRecord {
        operation: ExecutionPropertyOperation::Put,
        key,
        receiver: context.receiver,
        gc_boundary: ObjectOperationGcBoundary::for_operation(ExecutionPropertyOperation::Put),
        completion: ordinary_put_property(state, key, value, context),
    }
}

pub fn adapt_delete_property_for_execution(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    context: ObjectInternalMethodContext,
) -> ExecutionPropertyOperationRecord<DeletePropertyOutcome> {
    ExecutionPropertyOperationRecord {
        operation: ExecutionPropertyOperation::Delete,
        key,
        receiver: context.receiver,
        gc_boundary: ObjectOperationGcBoundary::for_operation(ExecutionPropertyOperation::Delete),
        completion: ordinary_delete_property(state, key, context),
    }
}

pub fn adapt_define_own_property_for_execution(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    descriptor: &PropertyDescriptor,
    receiver: PropertyReceiver,
) -> ExecutionPropertyOperationRecord<OrdinaryDefineOwnPropertyPlan> {
    ExecutionPropertyOperationRecord {
        operation: ExecutionPropertyOperation::DefineOwn,
        key,
        receiver,
        gc_boundary: ObjectOperationGcBoundary::for_operation(
            ExecutionPropertyOperation::DefineOwn,
        ),
        completion: ordinary_define_own_property(state, key, descriptor),
    }
}

/// Property write barrier input for a value stored in object-owned storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectPropertyMutationBarrierRequest {
    pub owner: CellId,
    pub key: PropertyKey,
    pub offset: PropertyOffset,
    pub value: JsValue,
    pub owner_state: CellState,
    pub target_state: Option<CellState>,
    pub initializing: bool,
    pub owner_is_published: bool,
}

impl ObjectPropertyMutationBarrierRequest {
    pub const fn store(
        owner: CellId,
        key: PropertyKey,
        offset: PropertyOffset,
        value: JsValue,
        owner_state: CellState,
        target_state: Option<CellState>,
    ) -> Self {
        Self {
            owner,
            key,
            offset,
            value,
            owner_state,
            target_state,
            initializing: false,
            owner_is_published: true,
        }
    }

    pub const fn initializing(mut self) -> Self {
        self.initializing = true;
        self.owner_is_published = false;
        self
    }
}

/// Planned barrier result for an object property mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectPropertyMutationBarrierRecord {
    pub owner: CellId,
    pub key: PropertyKey,
    pub offset: PropertyOffset,
    pub value_kind: crate::value::ValueKind,
    pub outcome: BarrierRequirementOutcome,
}

pub fn plan_object_property_mutation_barrier(
    request: ObjectPropertyMutationBarrierRequest,
) -> Result<ObjectPropertyMutationBarrierRecord, BarrierDecisionError> {
    let context = if request.initializing {
        BarrierWriteContext::initializing(
            BarrierFieldKind::Value,
            request.owner_state,
            request.target_state,
        )
    } else {
        BarrierWriteContext::store(
            BarrierFieldKind::Value,
            request.owner_state,
            request.target_state,
        )
    };
    let authority = if request.initializing {
        BarrierMutationAuthority::UnpublishedCellInitialization
    } else {
        BarrierMutationAuthority::MutatorFieldWrite
    };
    let outcome = static_barrier_schema_registry().evaluate_requirement(
        BarrierRequirementRequest::new(context)
            .authority(authority)
            .owner_is_published(request.owner_is_published),
    )?;

    Ok(ObjectPropertyMutationBarrierRecord {
        owner: request.owner,
        key: request.key,
        offset: request.offset,
        value_kind: request.value.kind(),
        outcome,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryDefineOwnPropertyPlan {
    pub key: PropertyKey,
    pub outcome: DefineOwnPropertyOutcome,
    pub compatibility: PropertyDescriptorCompatibility,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyDescriptorCompatibility {
    Compatible,
    CreatesNewProperty,
    MissingOnNonExtensible,
    ChangesNonConfigurable,
    ChangesDescriptorKind,
    ChangesEnumerable,
    WritesReadOnlyData,
    ChangesAccessor,
}

pub fn ordinary_get_own_property(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    context: ObjectInternalMethodContext,
) -> ObjectOperationResult<GetPropertyOutcome> {
    let Some(descriptor) = state.own_property(key) else {
        return ObjectOperationResult::Completed(GetPropertyOutcome::Missing);
    };

    descriptor_to_property_slot(descriptor, context)
        .map(|slot| ObjectOperationResult::Completed(GetPropertyOutcome::Found(slot)))
        .unwrap_or(ObjectOperationResult::Rejected(
            ObjectOperationError::SideEffectsDisallowed,
        ))
}

pub fn ordinary_has_property(state: &OrdinaryObjectState, key: PropertyKey) -> HasPropertyOutcome {
    if state.own_property(key).is_some() {
        HasPropertyOutcome::Present
    } else {
        HasPropertyOutcome::Missing
    }
}

pub fn ordinary_delete_property(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    context: ObjectInternalMethodContext,
) -> ObjectOperationResult<DeletePropertyOutcome> {
    let Some(descriptor) = state.own_property(key) else {
        return ObjectOperationResult::Completed(DeletePropertyOutcome::Missing);
    };

    if descriptor.attributes.configurable {
        ObjectOperationResult::Completed(DeletePropertyOutcome::Deleted)
    } else if context.strict || context.may_throw {
        ObjectOperationResult::Rejected(ObjectOperationError::NonConfigurable)
    } else {
        ObjectOperationResult::Completed(DeletePropertyOutcome::Rejected)
    }
}

pub fn ordinary_put_property(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    value: JsValue,
    context: PropertyMutationContext,
) -> ObjectOperationResult<PutPropertyOutcome> {
    let Some(current) = state.own_property(key) else {
        if !state.extensible {
            return reject_or_ignore(context.strict, ObjectOperationError::NonExtensible);
        }

        return ObjectOperationResult::Completed(PutPropertyOutcome::Created {
            new_structure: state.structure,
            offset: PropertyOffset::INVALID,
        });
    };

    match current.value {
        PropertyValueDescriptor::Data(data) => {
            if !data.writable.unwrap_or(current.attributes.writable) {
                return reject_or_ignore(context.strict, ObjectOperationError::ReadOnly);
            }
            let _stored_value = value;
            ObjectOperationResult::Completed(PutPropertyOutcome::Stored {
                base_structure: state.structure,
                offset: current.offset,
            })
        }
        PropertyValueDescriptor::Accessor(accessor) => {
            if accessor.custom_setter {
                if context.cacheable {
                    ObjectOperationResult::Completed(PutPropertyOutcome::CalledCustomAccessor)
                } else {
                    ObjectOperationResult::Rejected(ObjectOperationError::SideEffectsDisallowed)
                }
            } else if accessor.setter.is_some() {
                ObjectOperationResult::Completed(PutPropertyOutcome::CalledSetter)
            } else {
                reject_or_ignore(context.strict, ObjectOperationError::AccessorWithoutSetter)
            }
        }
        PropertyValueDescriptor::Generic => {
            reject_or_ignore(context.strict, ObjectOperationError::ReadOnly)
        }
    }
}

pub fn ordinary_define_own_property(
    state: &OrdinaryObjectState,
    key: PropertyKey,
    descriptor: &PropertyDescriptor,
) -> ObjectOperationResult<OrdinaryDefineOwnPropertyPlan> {
    if validate_property_descriptor(descriptor).is_err() {
        return ObjectOperationResult::Rejected(ObjectOperationError::TrapRejected);
    }

    let current = state.own_property(key);
    let compatibility =
        validate_ordinary_descriptor_compatibility(current, descriptor, state.extensible);
    if compatibility != PropertyDescriptorCompatibility::Compatible
        && compatibility != PropertyDescriptorCompatibility::CreatesNewProperty
    {
        return ObjectOperationResult::Rejected(match compatibility {
            PropertyDescriptorCompatibility::MissingOnNonExtensible => {
                ObjectOperationError::NonExtensible
            }
            PropertyDescriptorCompatibility::ChangesNonConfigurable
            | PropertyDescriptorCompatibility::ChangesDescriptorKind
            | PropertyDescriptorCompatibility::ChangesEnumerable
            | PropertyDescriptorCompatibility::ChangesAccessor => {
                ObjectOperationError::NonConfigurable
            }
            PropertyDescriptorCompatibility::WritesReadOnlyData => ObjectOperationError::ReadOnly,
            PropertyDescriptorCompatibility::Compatible
            | PropertyDescriptorCompatibility::CreatesNewProperty => {
                ObjectOperationError::TrapRejected
            }
        });
    }

    let outcome = match (current, descriptor.attributes.accessor) {
        (Some(current), _) => DefineOwnPropertyOutcome::Reconfigured {
            offset: current.offset,
            attributes: descriptor.attributes,
        },
        (None, true) => DefineOwnPropertyOutcome::DefinedAccessor {
            offset: descriptor.offset,
            attributes: descriptor.attributes,
        },
        (None, false) => DefineOwnPropertyOutcome::DefinedData {
            offset: descriptor.offset,
            attributes: descriptor.attributes,
        },
    };

    ObjectOperationResult::Completed(OrdinaryDefineOwnPropertyPlan {
        key,
        outcome,
        compatibility,
    })
}

pub fn validate_ordinary_descriptor_compatibility(
    current: Option<&PropertyDescriptor>,
    descriptor: &PropertyDescriptor,
    extensible: bool,
) -> PropertyDescriptorCompatibility {
    let Some(current) = current else {
        return if extensible {
            PropertyDescriptorCompatibility::CreatesNewProperty
        } else {
            PropertyDescriptorCompatibility::MissingOnNonExtensible
        };
    };

    if current.attributes.configurable {
        return PropertyDescriptorCompatibility::Compatible;
    }

    if descriptor.attributes.configurable {
        return PropertyDescriptorCompatibility::ChangesNonConfigurable;
    }
    if descriptor.attributes.enumerable != current.attributes.enumerable {
        return PropertyDescriptorCompatibility::ChangesEnumerable;
    }
    if descriptor.attributes.accessor != current.attributes.accessor {
        return PropertyDescriptorCompatibility::ChangesDescriptorKind;
    }

    match (current.value, descriptor.value) {
        (PropertyValueDescriptor::Data(current_data), PropertyValueDescriptor::Data(new_data))
            if !current.attributes.writable =>
        {
            let current_value = current_data
                .value
                .or(current.initial_value_hint)
                .unwrap_or_else(JsValue::undefined);
            let tries_to_write = new_data.writable.unwrap_or(false)
                || new_data.value.is_some_and(|value| current_value != value);
            if tries_to_write {
                return PropertyDescriptorCompatibility::WritesReadOnlyData;
            }
        }
        (
            PropertyValueDescriptor::Accessor(current_accessor),
            PropertyValueDescriptor::Accessor(new_accessor),
        ) => {
            let getter_changes =
                new_accessor.getter.is_some() && new_accessor.getter != current_accessor.getter;
            let setter_changes =
                new_accessor.setter.is_some() && new_accessor.setter != current_accessor.setter;
            if getter_changes || setter_changes {
                return PropertyDescriptorCompatibility::ChangesAccessor;
            }
        }
        _ => {}
    }

    PropertyDescriptorCompatibility::Compatible
}

fn descriptor_to_property_slot(
    descriptor: &PropertyDescriptor,
    context: ObjectInternalMethodContext,
) -> Option<PropertySlot> {
    match descriptor.value {
        PropertyValueDescriptor::Data(data) => {
            let mut slot = PropertySlot::new(context.receiver.value(), context.lookup_mode);
            slot.describe_value(
                descriptor.offset,
                descriptor.attributes,
                data.value
                    .or(descriptor.initial_value_hint)
                    .unwrap_or_else(JsValue::undefined),
            );
            Some(slot)
        }
        PropertyValueDescriptor::Accessor(accessor) => {
            if descriptor.attributes.custom_accessor
                && !context.allow_user_observable_side_effects
                && context.lookup_mode != PropertyLookupMode::VmInquiry
            {
                return None;
            }
            let mut slot = PropertySlot::new(context.receiver.value(), context.lookup_mode);
            if descriptor.attributes.custom_accessor {
                slot.describe_custom(descriptor.attributes);
                slot.disable_caching();
            } else {
                slot.describe_accessor(descriptor.offset, descriptor.attributes, accessor.getter);
            }
            Some(slot)
        }
        PropertyValueDescriptor::Generic => {
            let mut slot = PropertySlot::new(context.receiver.value(), context.lookup_mode);
            slot.disable_caching();
            Some(slot)
        }
    }
}

fn reject_or_ignore(
    strict: bool,
    error: ObjectOperationError,
) -> ObjectOperationResult<PutPropertyOutcome> {
    if strict {
        ObjectOperationResult::Rejected(error)
    } else {
        ObjectOperationResult::Completed(PutPropertyOutcome::IgnoredSloppyFailure)
    }
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
    /// Canonical private-name identity owned by `strings`.
    ///
    /// Brand checks borrow this handle; object code has no authority to allocate
    /// or widen it into public property identity.
    pub brand: PrivateName,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::StructureId;
    use crate::object::{DataDescriptor, PropertyDescriptorBuilder};
    use crate::strings::{AtomId, Identifier};

    fn key(slot: u32) -> PropertyKey {
        PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(slot)))
    }

    fn context() -> ObjectInternalMethodContext {
        ObjectInternalMethodContext::vm_inquiry(PropertyReceiver::new(JsValue::undefined()))
    }

    #[test]
    fn ordinary_get_reifies_data_descriptor_as_slot() {
        let descriptor = PropertyDescriptorBuilder::data(
            key(1),
            PropertyOffset::new(0),
            Some(JsValue::from_i32(9)),
        )
        .build()
        .unwrap();
        let state =
            OrdinaryObjectState::with_properties(StructureId::new(1), true, vec![descriptor])
                .unwrap();

        let result = ordinary_get_own_property(&state, key(1), context());

        match result {
            ObjectOperationResult::Completed(GetPropertyOutcome::Found(slot)) => {
                assert_eq!(slot.value(), Some(JsValue::from_i32(9)));
                assert_eq!(slot.offset(), PropertyOffset::new(0));
            }
            other => assert_eq!(
                other,
                ObjectOperationResult::Completed(GetPropertyOutcome::Missing)
            ),
        }
    }

    #[test]
    fn ordinary_define_rejects_new_property_on_non_extensible_state() {
        let descriptor = PropertyDescriptorBuilder::data(key(1), PropertyOffset::new(0), None)
            .build()
            .unwrap();
        let state = OrdinaryObjectState::new(StructureId::new(1), false);

        let result = ordinary_define_own_property(&state, key(1), &descriptor);

        assert_eq!(
            result,
            ObjectOperationResult::Rejected(ObjectOperationError::NonExtensible)
        );
    }

    #[test]
    fn ordinary_define_rejects_writes_to_non_writable_non_configurable_data() {
        let current_attributes = PropertyAttributes {
            writable: false,
            configurable: false,
            ..PropertyAttributes::DATA_DEFAULT
        };
        let current = PropertyDescriptorBuilder::data(key(1), PropertyOffset::new(0), None)
            .attributes(current_attributes)
            .build()
            .unwrap();
        let proposed = PropertyDescriptor {
            value: PropertyValueDescriptor::Data(DataDescriptor {
                value: Some(JsValue::from_i32(1)),
                writable: None,
            }),
            ..current.clone()
        };
        let state =
            OrdinaryObjectState::with_properties(StructureId::new(1), true, vec![current]).unwrap();

        let result = ordinary_define_own_property(&state, key(1), &proposed);

        assert_eq!(
            result,
            ObjectOperationResult::Rejected(ObjectOperationError::ReadOnly)
        );
    }

    #[test]
    fn execution_property_adapter_preserves_ordinary_put_rejection() {
        let descriptor = PropertyDescriptorBuilder::data(key(1), PropertyOffset::new(0), None)
            .attributes(PropertyAttributes {
                writable: false,
                ..PropertyAttributes::DATA_DEFAULT
            })
            .build()
            .unwrap();
        let descriptor = PropertyDescriptor {
            value: PropertyValueDescriptor::Data(DataDescriptor {
                value: None,
                writable: Some(false),
            }),
            ..descriptor
        };
        let state =
            OrdinaryObjectState::with_properties(StructureId::new(1), true, vec![descriptor])
                .unwrap();

        let record = adapt_put_property_for_execution(
            &state,
            key(1),
            JsValue::from_i32(4),
            PropertyMutationContext {
                receiver: PropertyReceiver::new(JsValue::undefined()),
                strict: true,
                initialization: false,
                define_own_semantics: false,
                cacheable: true,
            },
        );

        assert_eq!(record.operation, ExecutionPropertyOperation::Put);
        assert!(record.is_throw_boundary());
        assert_eq!(
            record.completion,
            ObjectOperationResult::Rejected(ObjectOperationError::ReadOnly)
        );
        assert_eq!(
            record.gc_boundary,
            ObjectOperationGcBoundary {
                heap_operation: HeapSemanticOperation::MutatePublishedCell,
                requires_no_gc_scope: false
            }
        );
    }

    #[test]
    fn object_property_mutation_barrier_requires_cell_value_store_barrier() {
        let value = JsValue::from_encoded(
            crate::value::static_value_representation_layout()
                .encode_cell_payload(0x1234)
                .unwrap(),
        );

        let record =
            plan_object_property_mutation_barrier(ObjectPropertyMutationBarrierRequest::store(
                CellId(1),
                key(1),
                PropertyOffset::new(0),
                value,
                CellState::PossiblyBlack,
                Some(CellState::DefinitelyWhite),
            ));

        assert_eq!(
            record.map(|record| record.outcome),
            Ok(BarrierRequirementOutcome::Required(
                crate::gc::BarrierAction::MarkingBarrier
            ))
        );
    }

    #[test]
    fn object_property_mutation_barrier_allows_unpublished_initialization() {
        let record = plan_object_property_mutation_barrier(
            ObjectPropertyMutationBarrierRequest::store(
                CellId(1),
                key(1),
                PropertyOffset::new(0),
                JsValue::undefined(),
                CellState::DefinitelyWhite,
                None,
            )
            .initializing(),
        );

        assert_eq!(
            record.map(|record| record.outcome),
            Ok(BarrierRequirementOutcome::NotRequired(
                crate::gc::BarrierNotRequiredReason::NullOrNonCellTarget
            ))
        );
    }
}
