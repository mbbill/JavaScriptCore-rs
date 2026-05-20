use crate::runtime::exception::JsResult;
use crate::runtime::state::{ObjectId, RuntimeValue, StringId, SymbolCellId, WatchpointGeneration};
use std::collections::HashSet;

/// Property access key as seen by ECMAScript internal methods.
///
/// This is not the canonical property-name identity. `strings::PropertyKey`
/// owns string/symbol/private-name identity; this runtime envelope names the
/// VM cells or numeric forms carried across object internal-method boundaries
/// until conversion to the string-layer key is authorized.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RuntimePropertyAccessKey {
    /// Runtime string cell that must be atomized before becoming a canonical
    /// string-layer property key.
    String(StringId),
    /// Runtime JS Symbol cell identity. Do not confuse this with
    /// `strings::SymbolUid`, which owns property-name identity.
    Symbol(SymbolCellId),
    /// Already-canonical array index fast path.
    ArrayIndex(u32),
    /// Integer-indexed exotic access that may not be an array index.
    IntegerIndex(u64),
    /// Runtime private-name cell identity before conversion to
    /// `strings::PrivateName`.
    PrivateName(SymbolCellId),
}

impl Default for RuntimePropertyAccessKey {
    fn default() -> Self {
        Self::String(StringId::default())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PropertyOffset(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PropertyDescriptor {
    /// Mirrors ECMAScript descriptors without choosing a storage layout.
    ///
    /// `value`, `get`, and `set` name already-rooted values or cells. Defining
    /// a property must validate descriptor compatibility before mutating the
    /// receiver structure or indexed storage.
    pub kind: DescriptorKind,
    pub value: RuntimeValue,
    pub get: Option<ObjectId>,
    pub set: Option<ObjectId>,
    pub attributes: PropertyAttributes,
}

/// Ownership of an immutable runtime descriptor table.
///
/// Runtime descriptor schemas are authored by the component that defines the
/// visible property contract. Mutation authority stays with the VM/object layer
/// that installs those properties; this table is static metadata only.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RuntimePropertyDescriptorOwner {
    RuntimeIntrinsic,
    BuiltinConstructor,
    BuiltinPrototype,
    GlobalObject,
    HostStaticObject,
    GeneratedBuiltinData,
}

/// Provenance for runtime descriptor schemas.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RuntimePropertyDescriptorProvenance {
    Ecma262,
    WebCompatibility,
    EngineIntrinsic,
    HostEmbedding,
    GeneratedStaticData,
}

/// Immutable runtime property descriptor entry.
///
/// The entry names descriptor shape and runtime-facing handles without defining
/// property lookup, allocation, accessor calls, or compatibility checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimePropertyDescriptorEntry {
    pub key: RuntimePropertyAccessKey,
    pub kind: DescriptorKind,
    pub attributes: PropertyAttributes,
    pub value: RuntimeValue,
    pub get: Option<ObjectId>,
    pub set: Option<ObjectId>,
}

/// Static table of runtime-facing property descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimePropertyDescriptorTable {
    pub name: &'static str,
    pub owner: RuntimePropertyDescriptorOwner,
    pub provenance: RuntimePropertyDescriptorProvenance,
    entries: &'static [RuntimePropertyDescriptorEntry],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimePropertyDescriptorValidationError {
    EmptyTableName,
    DuplicateKey(RuntimePropertyAccessKey),
    DataDescriptorHasAccessors(RuntimePropertyAccessKey),
    AccessorDescriptorMissingAccessors(RuntimePropertyAccessKey),
    GenericDescriptorHasPayload(RuntimePropertyAccessKey),
    PrivateKeyAttributeMismatch(RuntimePropertyAccessKey),
    CustomAccessorKindMismatch(RuntimePropertyAccessKey),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimePropertyDescriptorEntryBuilder {
    entry: RuntimePropertyDescriptorEntry,
}

impl RuntimePropertyDescriptorEntryBuilder {
    pub const fn data(
        key: RuntimePropertyAccessKey,
        value: RuntimeValue,
        attributes: PropertyAttributes,
    ) -> Self {
        Self {
            entry: RuntimePropertyDescriptorEntry {
                key,
                kind: DescriptorKind::Data,
                attributes,
                value,
                get: None,
                set: None,
            },
        }
    }

    pub const fn accessor(
        key: RuntimePropertyAccessKey,
        get: Option<ObjectId>,
        set: Option<ObjectId>,
        attributes: PropertyAttributes,
    ) -> Self {
        Self {
            entry: RuntimePropertyDescriptorEntry {
                key,
                kind: DescriptorKind::Accessor,
                attributes,
                value: RuntimeValue::undefined(),
                get,
                set,
            },
        }
    }

    pub const fn generic(key: RuntimePropertyAccessKey, attributes: PropertyAttributes) -> Self {
        Self {
            entry: RuntimePropertyDescriptorEntry {
                key,
                kind: DescriptorKind::Generic,
                attributes,
                value: RuntimeValue::undefined(),
                get: None,
                set: None,
            },
        }
    }

    pub fn build(
        self,
    ) -> Result<RuntimePropertyDescriptorEntry, RuntimePropertyDescriptorValidationError> {
        validate_runtime_property_descriptor_entry(&self.entry)?;
        Ok(self.entry)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimePropertyDescriptorTableBuilder {
    name: &'static str,
    owner: RuntimePropertyDescriptorOwner,
    provenance: RuntimePropertyDescriptorProvenance,
    entries: &'static [RuntimePropertyDescriptorEntry],
}

impl RuntimePropertyDescriptorTableBuilder {
    pub const fn new(
        name: &'static str,
        owner: RuntimePropertyDescriptorOwner,
        provenance: RuntimePropertyDescriptorProvenance,
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            entries: &[],
        }
    }

    pub const fn entries(mut self, entries: &'static [RuntimePropertyDescriptorEntry]) -> Self {
        self.entries = entries;
        self
    }

    pub fn build(
        self,
    ) -> Result<RuntimePropertyDescriptorTable, RuntimePropertyDescriptorValidationError> {
        let table = RuntimePropertyDescriptorTable::new(
            self.name,
            self.owner,
            self.provenance,
            self.entries,
        );
        table.validate()?;
        Ok(table)
    }
}

impl RuntimePropertyDescriptorTable {
    pub const fn new(
        name: &'static str,
        owner: RuntimePropertyDescriptorOwner,
        provenance: RuntimePropertyDescriptorProvenance,
        entries: &'static [RuntimePropertyDescriptorEntry],
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            entries,
        }
    }

    /// Returns the immutable descriptor entries owned by this static table.
    pub const fn entries(&self) -> &'static [RuntimePropertyDescriptorEntry] {
        self.entries
    }

    /// Returns one existing descriptor by static table index.
    pub const fn descriptor_at(
        &self,
        index: usize,
    ) -> Option<&'static RuntimePropertyDescriptorEntry> {
        if index < self.entries.len() {
            Some(&self.entries[index])
        } else {
            None
        }
    }

    pub fn validate(&self) -> Result<(), RuntimePropertyDescriptorValidationError> {
        validate_runtime_property_descriptor_table(self)
    }
}

pub fn validate_runtime_property_descriptor_entry(
    entry: &RuntimePropertyDescriptorEntry,
) -> Result<(), RuntimePropertyDescriptorValidationError> {
    let key_is_private = matches!(entry.key, RuntimePropertyAccessKey::PrivateName(_));
    if key_is_private != entry.attributes.is_private {
        return Err(
            RuntimePropertyDescriptorValidationError::PrivateKeyAttributeMismatch(entry.key),
        );
    }

    if entry.attributes.is_custom_accessor && entry.kind != DescriptorKind::Accessor {
        return Err(
            RuntimePropertyDescriptorValidationError::CustomAccessorKindMismatch(entry.key),
        );
    }

    match entry.kind {
        DescriptorKind::Data => {
            if entry.get.is_some() || entry.set.is_some() {
                return Err(
                    RuntimePropertyDescriptorValidationError::DataDescriptorHasAccessors(entry.key),
                );
            }
        }
        DescriptorKind::Accessor => {
            if entry.get.is_none() && entry.set.is_none() {
                return Err(
                    RuntimePropertyDescriptorValidationError::AccessorDescriptorMissingAccessors(
                        entry.key,
                    ),
                );
            }
        }
        DescriptorKind::Generic => {
            if entry.get.is_some()
                || entry.set.is_some()
                || entry.value != RuntimeValue::undefined()
            {
                return Err(
                    RuntimePropertyDescriptorValidationError::GenericDescriptorHasPayload(
                        entry.key,
                    ),
                );
            }
        }
    }

    Ok(())
}

pub fn validate_runtime_property_descriptor_table(
    table: &RuntimePropertyDescriptorTable,
) -> Result<(), RuntimePropertyDescriptorValidationError> {
    if table.name.is_empty() {
        return Err(RuntimePropertyDescriptorValidationError::EmptyTableName);
    }

    let mut keys = HashSet::new();
    for entry in table.entries {
        validate_runtime_property_descriptor_entry(entry)?;
        if !keys.insert(entry.key) {
            return Err(RuntimePropertyDescriptorValidationError::DuplicateKey(
                entry.key,
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DescriptorKind {
    #[default]
    Generic,
    Data,
    Accessor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PropertyAttributes {
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
    pub is_private: bool,
    pub is_custom_accessor: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PropertySlot {
    /// Result of a get or has lookup.
    ///
    /// The slot carries enough provenance for inline caches and accessors while
    /// leaving calls, allocation, and prototype walking to future runtime code.
    pub value: RuntimeValue,
    pub holder: Option<ObjectId>,
    pub offset: Option<PropertyOffset>,
    pub attributes: PropertyAttributes,
    pub resolution: PropertyResolution,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PropertyResolution {
    #[default]
    Missing,
    OwnData,
    OwnAccessor,
    PrototypeData,
    PrototypeAccessor,
    IndexedStorage,
    Intercepted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PutPropertySlot {
    pub receiver: Option<ObjectId>,
    pub should_throw: bool,
    pub mode: PutMode,
    pub result: PutResult,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PutMode {
    #[default]
    Set,
    DefineOwnProperty,
    InitializePrivateField,
    DirectWithoutTransition,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PutResult {
    #[default]
    Pending,
    Succeeded,
    FailedSilently,
    ThrowTypeError,
    RequiresAccessorCall,
    RequiresStructureTransition,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeletePropertySlot {
    pub should_throw: bool,
    pub result: DeleteResult,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DeleteResult {
    #[default]
    Pending,
    Deleted,
    Missing,
    NonConfigurable,
    Intercepted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OwnPropertyKeysRequest {
    pub mode: DontEnumPropertiesMode,
    pub include_symbols: bool,
    pub include_private_names: bool,
    pub order: PropertyEnumerationOrder,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DontEnumPropertiesMode {
    #[default]
    Include,
    Exclude,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PropertyEnumerationOrder {
    /// Integer indexes, strings, then symbols, matching ECMAScript own-key order.
    #[default]
    Canonical,
    StorageOrder,
    ProxyTrapOrder,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OwnPropertyKeysResult {
    pub keys: Vec<RuntimePropertyAccessKey>,
    pub source: PropertyKeysSource,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PropertyKeysSource {
    #[default]
    Ordinary,
    IndexedStorage,
    ProxyTrap,
    ModuleNamespace,
    HostObject,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PrototypeMutationMode {
    #[default]
    OrdinarySetPrototypeOf,
    DirectWithoutCycleCheck,
    ProxyTrap,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExtensibilityState {
    pub is_extensible: bool,
    pub prevent_extensions_generation: WatchpointGeneration,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexedAccessContract {
    pub may_intercept_indexed_accesses: bool,
    pub holes_forward_to_prototype: bool,
    pub length_observable: bool,
    pub integer_indexed_exotic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RuntimePropertyCompatibility {
    Compatible,
    CreatesNewProperty,
    MissingOnNonExtensible,
    ChangesNonConfigurable,
    ChangesDescriptorKind,
    ChangesEnumerable,
    WritesReadOnlyData,
    ChangesAccessor,
}

pub fn runtime_property_slot_from_descriptor(
    object: ObjectId,
    descriptor: &PropertyDescriptor,
) -> PropertySlot {
    let resolution = match descriptor.kind {
        DescriptorKind::Data => PropertyResolution::OwnData,
        DescriptorKind::Accessor if descriptor.attributes.is_custom_accessor => {
            PropertyResolution::Intercepted
        }
        DescriptorKind::Accessor => PropertyResolution::OwnAccessor,
        DescriptorKind::Generic => PropertyResolution::Missing,
    };

    PropertySlot {
        value: descriptor.value,
        holder: Some(object),
        offset: None,
        attributes: descriptor.attributes,
        resolution,
    }
}

pub fn plan_runtime_put(
    current: Option<&PropertyDescriptor>,
    extensible: ExtensibilityState,
    mut slot: PutPropertySlot,
) -> PutPropertySlot {
    slot.result = match current {
        Some(descriptor) if descriptor.kind == DescriptorKind::Accessor => {
            if descriptor.set.is_some() || descriptor.attributes.is_custom_accessor {
                PutResult::RequiresAccessorCall
            } else if slot.should_throw {
                PutResult::ThrowTypeError
            } else {
                PutResult::FailedSilently
            }
        }
        Some(descriptor) if descriptor.attributes.writable => PutResult::Succeeded,
        Some(_) if slot.should_throw => PutResult::ThrowTypeError,
        Some(_) => PutResult::FailedSilently,
        None if extensible.is_extensible => PutResult::RequiresStructureTransition,
        None if slot.should_throw => PutResult::ThrowTypeError,
        None => PutResult::FailedSilently,
    };
    slot
}

pub fn plan_runtime_delete(
    current: Option<&PropertyDescriptor>,
    mut slot: DeletePropertySlot,
) -> DeletePropertySlot {
    slot.result = match current {
        None => DeleteResult::Missing,
        Some(descriptor) if descriptor.attributes.configurable => DeleteResult::Deleted,
        Some(_) => DeleteResult::NonConfigurable,
    };
    slot
}

pub fn validate_runtime_define_own_property(
    current: Option<&PropertyDescriptor>,
    descriptor: &PropertyDescriptor,
    extensible: ExtensibilityState,
) -> RuntimePropertyCompatibility {
    let Some(current) = current else {
        return if extensible.is_extensible {
            RuntimePropertyCompatibility::CreatesNewProperty
        } else {
            RuntimePropertyCompatibility::MissingOnNonExtensible
        };
    };

    if current.attributes.configurable {
        return RuntimePropertyCompatibility::Compatible;
    }

    if descriptor.attributes.configurable {
        return RuntimePropertyCompatibility::ChangesNonConfigurable;
    }
    if descriptor.attributes.enumerable != current.attributes.enumerable {
        return RuntimePropertyCompatibility::ChangesEnumerable;
    }
    if descriptor.kind != current.kind {
        return RuntimePropertyCompatibility::ChangesDescriptorKind;
    }

    match (current.kind, descriptor.kind) {
        (DescriptorKind::Data, DescriptorKind::Data)
            if !current.attributes.writable
                && (descriptor.attributes.writable || descriptor.value != current.value) =>
        {
            return RuntimePropertyCompatibility::WritesReadOnlyData;
        }
        (DescriptorKind::Accessor, DescriptorKind::Accessor) => {
            let getter_changes = descriptor.get.is_some() && descriptor.get != current.get;
            let setter_changes = descriptor.set.is_some() && descriptor.set != current.set;
            if getter_changes || setter_changes {
                return RuntimePropertyCompatibility::ChangesAccessor;
            }
        }
        _ => {}
    }

    RuntimePropertyCompatibility::Compatible
}

/// ECMAScript object internal-method boundary.
///
/// Implementors must preserve exception discipline, proxy invariants,
/// descriptor validation, prototype-cycle checks, and cache invalidation.
pub trait RuntimePropertyOperations {
    fn get_prototype_of(&self, object: ObjectId) -> JsResult<RuntimeValue>;
    fn set_prototype_of(
        &mut self,
        object: ObjectId,
        prototype: RuntimeValue,
        mode: PrototypeMutationMode,
    ) -> JsResult<bool>;
    fn is_extensible(&self, object: ObjectId) -> JsResult<ExtensibilityState>;
    fn prevent_extensions(&mut self, object: ObjectId) -> JsResult<bool>;
    fn get_own_property(
        &self,
        object: ObjectId,
        key: RuntimePropertyAccessKey,
    ) -> JsResult<Option<PropertyDescriptor>>;
    fn define_own_property(
        &mut self,
        object: ObjectId,
        key: RuntimePropertyAccessKey,
        descriptor: PropertyDescriptor,
        should_throw: bool,
    ) -> JsResult<bool>;
    fn has_property(&self, object: ObjectId, key: RuntimePropertyAccessKey) -> JsResult<bool>;
    fn get(
        &self,
        object: ObjectId,
        key: RuntimePropertyAccessKey,
        receiver: RuntimeValue,
    ) -> JsResult<PropertySlot>;
    fn set(
        &mut self,
        object: ObjectId,
        key: RuntimePropertyAccessKey,
        value: RuntimeValue,
        receiver: RuntimeValue,
        slot: PutPropertySlot,
    ) -> JsResult<bool>;
    fn delete(
        &mut self,
        object: ObjectId,
        key: RuntimePropertyAccessKey,
        slot: DeletePropertySlot,
    ) -> JsResult<bool>;
    fn own_property_keys(
        &self,
        object: ObjectId,
        request: OwnPropertyKeysRequest,
    ) -> JsResult<OwnPropertyKeysResult>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    fn object(slot: u32) -> ObjectId {
        ObjectId(CellId(slot))
    }

    fn data_descriptor(writable: bool, configurable: bool) -> PropertyDescriptor {
        PropertyDescriptor {
            kind: DescriptorKind::Data,
            value: RuntimeValue::from_i32(1),
            get: None,
            set: None,
            attributes: PropertyAttributes {
                writable,
                enumerable: true,
                configurable,
                is_private: false,
                is_custom_accessor: false,
            },
        }
    }

    #[test]
    fn runtime_put_plans_transition_for_extensible_missing_property() {
        let slot = plan_runtime_put(
            None,
            ExtensibilityState {
                is_extensible: true,
                prevent_extensions_generation: WatchpointGeneration(0),
            },
            PutPropertySlot::default(),
        );

        assert_eq!(slot.result, PutResult::RequiresStructureTransition);
    }

    #[test]
    fn runtime_define_rejects_rewriting_non_writable_data() {
        let current = data_descriptor(false, false);
        let replacement = data_descriptor(true, false);

        let compatibility = validate_runtime_define_own_property(
            Some(&current),
            &replacement,
            ExtensibilityState {
                is_extensible: true,
                prevent_extensions_generation: WatchpointGeneration(0),
            },
        );

        assert_eq!(
            compatibility,
            RuntimePropertyCompatibility::WritesReadOnlyData
        );
    }

    #[test]
    fn runtime_property_slot_tracks_descriptor_resolution() {
        let descriptor = data_descriptor(true, true);

        let slot = runtime_property_slot_from_descriptor(object(1), &descriptor);

        assert_eq!(slot.holder, Some(object(1)));
        assert_eq!(slot.resolution, PropertyResolution::OwnData);
        assert_eq!(slot.value, RuntimeValue::from_i32(1));
    }
}
