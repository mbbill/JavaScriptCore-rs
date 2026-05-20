//! Property keys, offsets, and metadata.
//!
//! Property key identity is owned by `strings`; object storage only records
//! handles to that identity plus offsets and descriptor metadata.

use crate::runtime::scope::ScopeOffset;
use crate::runtime::state::{HostHookId, ObjectId};
pub use crate::strings::{AtomId, PrivateName, PropertyKey};
use crate::value::JsValue;
use std::collections::HashSet;

/// Inline or out-of-line property slot offset.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct PropertyOffset(pub i32);

impl PropertyOffset {
    pub const INVALID: Self = Self(-1);

    pub const fn new(raw: i32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> i32 {
        self.0
    }

    pub const fn location(self) -> PropertyLocation {
        if self.0 < 0 {
            PropertyLocation::Invalid
        } else {
            PropertyLocation::InlineOrOutOfLine
        }
    }
}

/// Storage family selected by the structure for a property offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLocation {
    Invalid,
    InlineOrOutOfLine,
    StaticTable,
    CustomAccessor,
}

/// ECMAScript-facing descriptor attributes plus JSC-specific cache flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PropertyAttributes {
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
    pub accessor: bool,
    pub read_only_for_structures: bool,
    pub dont_delete_for_caches: bool,
    pub custom_accessor: bool,
    pub custom_value: bool,
    pub static_table_kind: Option<StaticPropertyKind>,
}

impl PropertyAttributes {
    pub const DATA_DEFAULT: Self = Self {
        writable: true,
        enumerable: true,
        configurable: true,
        accessor: false,
        read_only_for_structures: false,
        dont_delete_for_caches: false,
        custom_accessor: false,
        custom_value: false,
        static_table_kind: None,
    };

    pub const ACCESSOR_DEFAULT: Self = Self {
        writable: false,
        enumerable: true,
        configurable: true,
        accessor: true,
        read_only_for_structures: false,
        dont_delete_for_caches: false,
        custom_accessor: false,
        custom_value: false,
        static_table_kind: None,
    };

    pub const fn is_data(self) -> bool {
        !self.accessor
    }

    pub const fn is_accessor(self) -> bool {
        self.accessor
    }
}

/// Static property-table payload outside the compact structure attributes byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticPropertyKind {
    Function,
    BuiltinFunction,
    ConstantInteger,
    CellProperty,
    ClassStructure,
    PropertyCallback,
    DomAttribute,
    DomJitAttribute,
    DomJitFunction,
}

/// Slot lookup mode. VM inquiries must not perform user-observable work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyLookupMode {
    Get,
    HasProperty,
    GetOwnProperty,
    VmInquiry,
}

/// Result cacheability for a property slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyCacheability {
    Allowed,
    Disallowed,
    TaintedByOpaqueObject,
}

/// Shape of a property descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyDescriptorKind {
    Generic,
    Data,
    Accessor,
    CustomAccessor,
    NativeStatic,
}

/// Descriptor normalization state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyDescriptorState {
    Partial,
    Complete,
    ReifiedFromStructure,
    ReifiedFromStaticTable,
}

/// Data-property payload contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataDescriptor {
    pub value: Option<JsValue>,
    pub writable: Option<bool>,
}

/// Accessor-property payload contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessorDescriptor {
    pub getter: Option<JsValue>,
    pub setter: Option<JsValue>,
    pub custom_getter: bool,
    pub custom_setter: bool,
}

/// Descriptor payload selected after parsing ECMAScript descriptor fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyValueDescriptor {
    Generic,
    Data(DataDescriptor),
    Accessor(AccessorDescriptor),
}

/// Fully classified descriptor ready for compatibility checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletePropertyDescriptor {
    pub key: PropertyKey,
    pub value: PropertyValueDescriptor,
    pub attributes: PropertyAttributes,
}

/// Property metadata entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyDescriptor {
    pub key: PropertyKey,
    pub offset: PropertyOffset,
    pub attributes: PropertyAttributes,
    pub location: PropertyLocation,
    pub initial_value_hint: Option<JsValue>,
    pub kind: PropertyDescriptorKind,
    pub state: PropertyDescriptorState,
    pub value: PropertyValueDescriptor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PropertyDescriptorValidationError {
    EmptyTableName,
    DuplicateKey(PropertyKey),
    InvalidOffset(PropertyKey),
    LocationOffsetMismatch(PropertyKey),
    KindAttributeMismatch(PropertyKey),
    ValueDescriptorMismatch(PropertyKey),
    StaticKindMismatch(PropertyKey),
    CustomAccessorMismatch(PropertyKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyDescriptorBuilder {
    descriptor: PropertyDescriptor,
}

impl PropertyDescriptorBuilder {
    pub fn data(key: PropertyKey, offset: PropertyOffset, value: Option<JsValue>) -> Self {
        let attributes = PropertyAttributes::DATA_DEFAULT;
        Self {
            descriptor: PropertyDescriptor {
                key,
                offset,
                attributes,
                location: offset.location(),
                initial_value_hint: value,
                kind: PropertyDescriptorKind::Data,
                state: PropertyDescriptorState::Complete,
                value: PropertyValueDescriptor::Data(DataDescriptor {
                    value,
                    writable: Some(attributes.writable),
                }),
            },
        }
    }

    pub fn accessor(
        key: PropertyKey,
        offset: PropertyOffset,
        getter: Option<JsValue>,
        setter: Option<JsValue>,
    ) -> Self {
        let attributes = PropertyAttributes::ACCESSOR_DEFAULT;
        Self {
            descriptor: PropertyDescriptor {
                key,
                offset,
                attributes,
                location: offset.location(),
                initial_value_hint: None,
                kind: PropertyDescriptorKind::Accessor,
                state: PropertyDescriptorState::Complete,
                value: PropertyValueDescriptor::Accessor(AccessorDescriptor {
                    getter,
                    setter,
                    custom_getter: false,
                    custom_setter: false,
                }),
            },
        }
    }

    pub fn generic(key: PropertyKey) -> Self {
        Self {
            descriptor: PropertyDescriptor {
                key,
                offset: PropertyOffset::INVALID,
                attributes: PropertyAttributes::default(),
                location: PropertyLocation::Invalid,
                initial_value_hint: None,
                kind: PropertyDescriptorKind::Generic,
                state: PropertyDescriptorState::Partial,
                value: PropertyValueDescriptor::Generic,
            },
        }
    }

    pub fn custom_accessor(
        key: PropertyKey,
        getter: Option<JsValue>,
        setter: Option<JsValue>,
    ) -> Self {
        let attributes = PropertyAttributes {
            custom_accessor: true,
            ..PropertyAttributes::ACCESSOR_DEFAULT
        };
        Self {
            descriptor: PropertyDescriptor {
                key,
                offset: PropertyOffset::INVALID,
                attributes,
                location: PropertyLocation::CustomAccessor,
                initial_value_hint: None,
                kind: PropertyDescriptorKind::CustomAccessor,
                state: PropertyDescriptorState::Complete,
                value: PropertyValueDescriptor::Accessor(AccessorDescriptor {
                    getter,
                    setter,
                    custom_getter: true,
                    custom_setter: true,
                }),
            },
        }
    }

    pub fn native_static(
        key: PropertyKey,
        static_kind: StaticPropertyKind,
        value: PropertyValueDescriptor,
    ) -> Self {
        let accessor = matches!(value, PropertyValueDescriptor::Accessor(_));
        let attributes = PropertyAttributes {
            accessor,
            static_table_kind: Some(static_kind),
            ..PropertyAttributes::DATA_DEFAULT
        };
        Self {
            descriptor: PropertyDescriptor {
                key,
                offset: PropertyOffset::INVALID,
                attributes,
                location: PropertyLocation::StaticTable,
                initial_value_hint: None,
                kind: PropertyDescriptorKind::NativeStatic,
                state: PropertyDescriptorState::ReifiedFromStaticTable,
                value,
            },
        }
    }

    pub fn attributes(mut self, attributes: PropertyAttributes) -> Self {
        self.descriptor.attributes = attributes;
        self
    }

    pub fn state(mut self, state: PropertyDescriptorState) -> Self {
        self.descriptor.state = state;
        self
    }

    pub fn location(mut self, location: PropertyLocation) -> Self {
        self.descriptor.location = location;
        self
    }

    pub fn initial_value_hint(mut self, value: Option<JsValue>) -> Self {
        self.descriptor.initial_value_hint = value;
        self
    }

    pub fn build(self) -> Result<PropertyDescriptor, PropertyDescriptorValidationError> {
        validate_property_descriptor(&self.descriptor)?;
        Ok(self.descriptor)
    }
}

pub fn validate_property_descriptor(
    descriptor: &PropertyDescriptor,
) -> Result<(), PropertyDescriptorValidationError> {
    let location_from_offset = descriptor.offset.location();
    match descriptor.location {
        PropertyLocation::InlineOrOutOfLine if location_from_offset != descriptor.location => {
            return Err(PropertyDescriptorValidationError::InvalidOffset(
                descriptor.key,
            ));
        }
        PropertyLocation::Invalid if descriptor.offset != PropertyOffset::INVALID => {
            return Err(PropertyDescriptorValidationError::LocationOffsetMismatch(
                descriptor.key,
            ));
        }
        PropertyLocation::StaticTable | PropertyLocation::CustomAccessor
            if descriptor.offset != PropertyOffset::INVALID =>
        {
            return Err(PropertyDescriptorValidationError::LocationOffsetMismatch(
                descriptor.key,
            ));
        }
        _ => {}
    }

    if descriptor.attributes.custom_accessor && !descriptor.attributes.accessor {
        return Err(PropertyDescriptorValidationError::CustomAccessorMismatch(
            descriptor.key,
        ));
    }

    if descriptor.attributes.static_table_kind.is_some()
        != matches!(descriptor.location, PropertyLocation::StaticTable)
    {
        return Err(PropertyDescriptorValidationError::StaticKindMismatch(
            descriptor.key,
        ));
    }

    match descriptor.kind {
        PropertyDescriptorKind::Generic => {
            if !matches!(descriptor.value, PropertyValueDescriptor::Generic)
                || descriptor.attributes.accessor
            {
                return Err(PropertyDescriptorValidationError::ValueDescriptorMismatch(
                    descriptor.key,
                ));
            }
        }
        PropertyDescriptorKind::Data => {
            if descriptor.attributes.accessor
                || !matches!(descriptor.value, PropertyValueDescriptor::Data(_))
            {
                return Err(PropertyDescriptorValidationError::KindAttributeMismatch(
                    descriptor.key,
                ));
            }
        }
        PropertyDescriptorKind::Accessor => {
            if !descriptor.attributes.accessor
                || !matches!(descriptor.value, PropertyValueDescriptor::Accessor(_))
            {
                return Err(PropertyDescriptorValidationError::KindAttributeMismatch(
                    descriptor.key,
                ));
            }
        }
        PropertyDescriptorKind::CustomAccessor => {
            if !descriptor.attributes.accessor
                || !descriptor.attributes.custom_accessor
                || descriptor.location != PropertyLocation::CustomAccessor
                || !matches!(descriptor.value, PropertyValueDescriptor::Accessor(_))
            {
                return Err(PropertyDescriptorValidationError::CustomAccessorMismatch(
                    descriptor.key,
                ));
            }
        }
        PropertyDescriptorKind::NativeStatic => {
            if descriptor.location != PropertyLocation::StaticTable
                || descriptor.attributes.static_table_kind.is_none()
            {
                return Err(PropertyDescriptorValidationError::StaticKindMismatch(
                    descriptor.key,
                ));
            }
        }
    }

    match (descriptor.attributes.accessor, descriptor.value) {
        (true, PropertyValueDescriptor::Accessor(_))
        | (false, PropertyValueDescriptor::Data(_))
        | (false, PropertyValueDescriptor::Generic) => Ok(()),
        _ => Err(PropertyDescriptorValidationError::ValueDescriptorMismatch(
            descriptor.key,
        )),
    }
}

/// Owner of immutable object-property schema metadata.
///
/// Structure and object code may borrow these schemas to describe static data.
/// Adding, deleting, or reconfiguring runtime properties still belongs to the
/// owning structure/object mutation authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertySchemaOwner {
    Structure,
    ClassInfo,
    BuiltinObject,
    GlobalObject,
    ModuleNamespace,
    GeneratedStaticTable,
}

/// Source of a static property-table schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertySchemaProvenance {
    HandAuthoredRust,
    GeneratedFromEngineMetadata,
    Ecma262Intrinsic,
    HostBinding,
}

/// Immutable property-table descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticPropertyTableDescriptor {
    pub name: &'static str,
    pub owner: PropertySchemaOwner,
    pub provenance: PropertySchemaProvenance,
    pub mode: PropertyTableMode,
    entries: &'static [PropertyDescriptor],
}

impl StaticPropertyTableDescriptor {
    pub const fn new(
        name: &'static str,
        owner: PropertySchemaOwner,
        provenance: PropertySchemaProvenance,
        mode: PropertyTableMode,
        entries: &'static [PropertyDescriptor],
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            mode,
            entries,
        }
    }

    /// Returns property metadata exactly as stored in the static table.
    pub const fn entries(&self) -> &'static [PropertyDescriptor] {
        self.entries
    }

    /// Returns one existing static descriptor by table index.
    pub const fn descriptor_at(&self, index: usize) -> Option<&'static PropertyDescriptor> {
        if index < self.entries.len() {
            Some(&self.entries[index])
        } else {
            None
        }
    }

    pub fn validate(&self) -> Result<(), PropertyDescriptorValidationError> {
        validate_property_descriptors(self.name, self.entries)
    }
}

/// Stack-only property-slot descriptor. It does not hold a Rust borrow of the
/// object because a future implementation may need it to act as a GC root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertySlot {
    this_value: JsValue,
    mode: PropertyLookupMode,
    cacheability: PropertyCacheability,
    offset: PropertyOffset,
    attributes: PropertyAttributes,
    value: Option<JsValue>,
    kind: PropertySlotKind,
    base: PropertySlotBase,
    additional_data: PropertySlotAdditionalData,
}

impl PropertySlot {
    pub fn new(this_value: JsValue, mode: PropertyLookupMode) -> Self {
        Self {
            this_value,
            mode,
            cacheability: PropertyCacheability::Allowed,
            offset: PropertyOffset::INVALID,
            attributes: PropertyAttributes::default(),
            value: None,
            kind: PropertySlotKind::Unset,
            base: PropertySlotBase::None,
            additional_data: PropertySlotAdditionalData::None,
        }
    }

    pub fn this_value(&self) -> JsValue {
        self.this_value
    }

    pub fn mode(&self) -> PropertyLookupMode {
        self.mode
    }

    pub fn cacheability(&self) -> PropertyCacheability {
        self.cacheability
    }

    pub fn offset(&self) -> PropertyOffset {
        self.offset
    }

    pub fn value(&self) -> Option<JsValue> {
        self.value
    }

    pub fn kind(&self) -> PropertySlotKind {
        self.kind
    }

    pub fn base(&self) -> PropertySlotBase {
        self.base
    }

    pub fn additional_data(&self) -> PropertySlotAdditionalData {
        self.additional_data
    }

    pub fn attributes(&self) -> PropertyAttributes {
        self.attributes
    }

    pub fn describe_value(
        &mut self,
        offset: PropertyOffset,
        attributes: PropertyAttributes,
        value: JsValue,
    ) {
        self.offset = offset;
        self.attributes = attributes;
        self.value = Some(value);
        self.kind = PropertySlotKind::Value;
        self.base = PropertySlotBase::ObjectStorage;
    }

    pub fn describe_accessor(
        &mut self,
        offset: PropertyOffset,
        attributes: PropertyAttributes,
        getter: Option<JsValue>,
    ) {
        self.offset = offset;
        self.attributes = attributes;
        self.value = getter;
        self.kind = PropertySlotKind::Accessor;
        self.base = PropertySlotBase::ObjectStorage;
    }

    pub fn describe_custom(&mut self, attributes: PropertyAttributes) {
        self.offset = PropertyOffset::INVALID;
        self.attributes = attributes;
        self.value = None;
        self.kind = PropertySlotKind::Custom;
        self.base = PropertySlotBase::CustomObject;
    }

    pub fn disable_caching(&mut self) {
        self.cacheability = PropertyCacheability::Disallowed;
    }

    pub fn set_tainted_by_opaque_object(&mut self) {
        self.cacheability = PropertyCacheability::TaintedByOpaqueObject;
    }
}

/// Additional payload attached to a stack-only property slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertySlotAdditionalData {
    None,
    DomAttribute(DomAttributeSlot),
    ModuleNamespace(ModuleNamespaceSlot),
}

/// DOM/custom accessor annotation kept opaque to the object model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomAttributeSlot {
    pub getter: Option<HostHookId>,
    pub setter: Option<HostHookId>,
}

/// Module namespace slot payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleNamespaceSlot {
    pub environment: ObjectId,
    pub scope_offset: ScopeOffset,
}

/// Cached put slot kind mirrored from `PutPropertySlot`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PutPropertySlotKind {
    Uncacheable,
    ExistingProperty,
    NewProperty,
    SetterProperty,
    CustomValue,
    CustomAccessor,
}

/// Call-site context that selected a put/define path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PutPropertySlotContext {
    Unknown,
    PutById,
    PutByIdEval,
}

/// Property-slot payload class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertySlotKind {
    Unset,
    Value,
    Accessor,
    Custom,
    ModuleNamespace,
}

/// Base that supplied a property slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertySlotBase {
    None,
    ObjectStorage,
    PrototypeObject,
    StringLength,
    StaticTable,
    CustomObject,
}

/// Kind of private field stored on an object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateFieldKind {
    Field,
    Method,
    Getter,
    Setter,
    AccessorPair,
    BrandOnly,
}

/// Private field or brand metadata hidden from public property enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateFieldDescriptor {
    /// Canonical private-name identity owned by the string/name layer.
    ///
    /// Object metadata borrows this copyable handle for lookup and brand checks;
    /// it does not mint or reinterpret private-name identity.
    pub name: PrivateName,
    pub kind: PrivateFieldKind,
    pub offset: PropertyOffset,
    pub attributes: PropertyAttributes,
}

/// Enumeration buckets in ECMAScript order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnumerationBucket {
    ArrayIndex,
    StringProperty,
    SymbolProperty,
    PrivateName,
}

/// Enumeration ordering contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnumerationOrder {
    ArrayIndicesAscending,
    PropertyInsertionOrder,
    SymbolInsertionOrder,
    StructureTableOrder,
}

/// One key emitted by property-name collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnumerationRecord {
    pub key: PropertyKey,
    pub bucket: EnumerationBucket,
    pub order: EnumerationOrder,
    pub enumerable: bool,
}

/// Shape-owned or dictionary-owned property metadata table.
#[derive(Clone, Debug, Default)]
pub struct PropertyTable {
    entries: Vec<PropertyDescriptor>,
    mode: PropertyTableMode,
}

impl PropertyTable {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            mode: PropertyTableMode::Shape,
        }
    }

    pub fn entries(&self) -> &[PropertyDescriptor] {
        &self.entries
    }

    pub fn mode(&self) -> PropertyTableMode {
        self.mode
    }

    pub fn enter_dictionary_mode(&mut self) {
        self.mode = PropertyTableMode::Dictionary;
    }

    pub fn reserve_transition_slot(
        &mut self,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) -> PropertyOffset {
        // Placeholder offset assignment for metadata construction only. Real
        // transition code must coordinate this with inline/out-of-line storage.
        let offset = PropertyOffset(self.entries.len() as i32);
        self.entries.push(PropertyDescriptor {
            key,
            offset,
            attributes,
            location: PropertyLocation::InlineOrOutOfLine,
            initial_value_hint: None,
            kind: if attributes.accessor {
                PropertyDescriptorKind::Accessor
            } else {
                PropertyDescriptorKind::Data
            },
            state: PropertyDescriptorState::ReifiedFromStructure,
            value: if attributes.accessor {
                PropertyValueDescriptor::Accessor(AccessorDescriptor {
                    getter: None,
                    setter: None,
                    custom_getter: false,
                    custom_setter: false,
                })
            } else {
                PropertyValueDescriptor::Data(DataDescriptor {
                    value: None,
                    writable: Some(attributes.writable),
                })
            },
        });
        offset
    }

    pub fn validate(&self) -> Result<(), PropertyDescriptorValidationError> {
        validate_property_descriptors("property-table", &self.entries)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PropertyTableBuilder {
    entries: Vec<PropertyDescriptor>,
    mode: PropertyTableMode,
}

impl PropertyTableBuilder {
    pub fn new(mode: PropertyTableMode) -> Self {
        Self {
            entries: Vec::new(),
            mode,
        }
    }

    pub fn push(
        mut self,
        descriptor: PropertyDescriptor,
    ) -> Result<Self, PropertyDescriptorValidationError> {
        validate_property_descriptor(&descriptor)?;
        if self.entries.iter().any(|entry| entry.key == descriptor.key) {
            return Err(PropertyDescriptorValidationError::DuplicateKey(
                descriptor.key,
            ));
        }
        self.entries.push(descriptor);
        Ok(self)
    }

    pub fn build(self) -> Result<PropertyTable, PropertyDescriptorValidationError> {
        validate_property_descriptors("property-table", &self.entries)?;
        Ok(PropertyTable {
            entries: self.entries,
            mode: self.mode,
        })
    }
}

pub fn validate_property_descriptors(
    table_name: &str,
    entries: &[PropertyDescriptor],
) -> Result<(), PropertyDescriptorValidationError> {
    if table_name.is_empty() {
        return Err(PropertyDescriptorValidationError::EmptyTableName);
    }

    let mut seen = HashSet::new();
    for entry in entries {
        validate_property_descriptor(entry)?;
        if !seen.insert(entry.key) {
            return Err(PropertyDescriptorValidationError::DuplicateKey(entry.key));
        }
    }
    Ok(())
}

/// Summary produced from a validated static property table.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticPropertyTableAnalysis {
    pub entry_count: usize,
    pub data_count: usize,
    pub accessor_count: usize,
    pub custom_accessor_count: usize,
    pub native_static_count: usize,
    pub enumerable_count: usize,
    pub max_offset: Option<PropertyOffset>,
}

pub fn analyze_static_property_table(
    table: &StaticPropertyTableDescriptor,
) -> Result<StaticPropertyTableAnalysis, PropertyDescriptorValidationError> {
    table.validate()?;

    let mut analysis = StaticPropertyTableAnalysis {
        entry_count: table.entries.len(),
        ..StaticPropertyTableAnalysis::default()
    };

    for entry in table.entries {
        match entry.kind {
            PropertyDescriptorKind::Data => analysis.data_count += 1,
            PropertyDescriptorKind::Accessor => analysis.accessor_count += 1,
            PropertyDescriptorKind::CustomAccessor => analysis.custom_accessor_count += 1,
            PropertyDescriptorKind::NativeStatic => analysis.native_static_count += 1,
            PropertyDescriptorKind::Generic => {}
        }
        if entry.attributes.enumerable {
            analysis.enumerable_count += 1;
        }
        if entry.location == PropertyLocation::InlineOrOutOfLine {
            analysis.max_offset = Some(
                analysis
                    .max_offset
                    .filter(|offset| offset.raw() >= entry.offset.raw())
                    .unwrap_or(entry.offset),
            );
        }
    }

    Ok(analysis)
}

pub fn enumerate_static_property_table(
    table: &StaticPropertyTableDescriptor,
    property_name_mode: crate::strings::PropertyNameMode,
    private_symbol_mode: crate::strings::PrivateSymbolMode,
) -> Result<Vec<EnumerationRecord>, PropertyDescriptorValidationError> {
    table.validate()?;

    let mut indices = Vec::new();
    let mut strings = Vec::new();
    let mut symbols = Vec::new();
    let mut private_names = Vec::new();

    for entry in table.entries {
        if !include_property_key(entry.key, property_name_mode, private_symbol_mode) {
            continue;
        }

        let record = EnumerationRecord {
            key: entry.key,
            bucket: enumeration_bucket(entry.key),
            order: enumeration_order(entry.key),
            enumerable: entry.attributes.enumerable,
        };

        match record.bucket {
            EnumerationBucket::ArrayIndex => indices.push(record),
            EnumerationBucket::StringProperty => strings.push(record),
            EnumerationBucket::SymbolProperty => symbols.push(record),
            EnumerationBucket::PrivateName => private_names.push(record),
        }
    }

    indices.sort_by_key(|record| record.key.as_index().map(|index| index.get()).unwrap_or(0));

    let mut records = Vec::with_capacity(indices.len() + strings.len() + symbols.len());
    records.extend(indices);
    records.extend(strings);
    records.extend(symbols);
    records.extend(private_names);
    Ok(records)
}

fn include_property_key(
    key: PropertyKey,
    property_name_mode: crate::strings::PropertyNameMode,
    private_symbol_mode: crate::strings::PrivateSymbolMode,
) -> bool {
    match key {
        PropertyKey::String(_) | PropertyKey::Index(_) => property_name_mode.includes_strings(),
        PropertyKey::Symbol(_) => property_name_mode.includes_symbols(),
        PropertyKey::PrivateName(_) => {
            private_symbol_mode == crate::strings::PrivateSymbolMode::Include
        }
    }
}

const fn enumeration_bucket(key: PropertyKey) -> EnumerationBucket {
    match key {
        PropertyKey::Index(_) => EnumerationBucket::ArrayIndex,
        PropertyKey::String(_) => EnumerationBucket::StringProperty,
        PropertyKey::Symbol(_) => EnumerationBucket::SymbolProperty,
        PropertyKey::PrivateName(_) => EnumerationBucket::PrivateName,
    }
}

const fn enumeration_order(key: PropertyKey) -> EnumerationOrder {
    match key {
        PropertyKey::Index(_) => EnumerationOrder::ArrayIndicesAscending,
        PropertyKey::String(_) => EnumerationOrder::PropertyInsertionOrder,
        PropertyKey::Symbol(_) => EnumerationOrder::SymbolInsertionOrder,
        PropertyKey::PrivateName(_) => EnumerationOrder::StructureTableOrder,
    }
}

/// Ownership mode for property metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PropertyTableMode {
    #[default]
    Shape,
    Dictionary,
    Static,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::{AtomId, Identifier};

    fn key(slot: u32) -> PropertyKey {
        PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(slot)))
    }

    #[test]
    fn property_table_builder_accepts_distinct_data_descriptors() {
        let first = PropertyDescriptorBuilder::data(key(1), PropertyOffset::new(0), None)
            .build()
            .unwrap();
        let second = PropertyDescriptorBuilder::data(key(2), PropertyOffset::new(1), None)
            .build()
            .unwrap();

        let table = PropertyTableBuilder::new(PropertyTableMode::Shape)
            .push(first)
            .unwrap()
            .push(second)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(table.entries().len(), 2);
        assert_eq!(table.validate(), Ok(()));
    }

    #[test]
    fn property_table_builder_rejects_duplicate_keys() {
        let descriptor = PropertyDescriptorBuilder::data(key(1), PropertyOffset::new(0), None)
            .build()
            .unwrap();

        let error = PropertyTableBuilder::new(PropertyTableMode::Shape)
            .push(descriptor.clone())
            .unwrap()
            .push(descriptor)
            .unwrap_err();

        assert_eq!(
            error,
            PropertyDescriptorValidationError::DuplicateKey(key(1))
        );
    }

    #[test]
    fn property_descriptor_rejects_accessor_payload_on_data_attributes() {
        let descriptor = PropertyDescriptor {
            key: key(1),
            offset: PropertyOffset::new(0),
            attributes: PropertyAttributes::DATA_DEFAULT,
            location: PropertyLocation::InlineOrOutOfLine,
            initial_value_hint: None,
            kind: PropertyDescriptorKind::Data,
            state: PropertyDescriptorState::Complete,
            value: PropertyValueDescriptor::Accessor(AccessorDescriptor {
                getter: None,
                setter: None,
                custom_getter: false,
                custom_setter: false,
            }),
        };

        assert_eq!(
            validate_property_descriptor(&descriptor),
            Err(PropertyDescriptorValidationError::KindAttributeMismatch(
                key(1)
            ))
        );
    }

    #[test]
    fn static_property_analysis_counts_descriptor_shapes() {
        static ENTRIES: &[PropertyDescriptor] = &[
            PropertyDescriptor {
                key: PropertyKey::from_index(crate::strings::PropertyIndex::from_canonical_index(
                    2,
                )),
                offset: PropertyOffset::new(0),
                attributes: PropertyAttributes::DATA_DEFAULT,
                location: PropertyLocation::InlineOrOutOfLine,
                initial_value_hint: None,
                kind: PropertyDescriptorKind::Data,
                state: PropertyDescriptorState::Complete,
                value: PropertyValueDescriptor::Data(DataDescriptor {
                    value: None,
                    writable: Some(true),
                }),
            },
            PropertyDescriptor {
                key: PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
                    8,
                ))),
                offset: PropertyOffset::INVALID,
                attributes: PropertyAttributes {
                    custom_accessor: true,
                    ..PropertyAttributes::ACCESSOR_DEFAULT
                },
                location: PropertyLocation::CustomAccessor,
                initial_value_hint: None,
                kind: PropertyDescriptorKind::CustomAccessor,
                state: PropertyDescriptorState::Complete,
                value: PropertyValueDescriptor::Accessor(AccessorDescriptor {
                    getter: None,
                    setter: None,
                    custom_getter: true,
                    custom_setter: true,
                }),
            },
        ];
        static TABLE: StaticPropertyTableDescriptor = StaticPropertyTableDescriptor::new(
            "static",
            PropertySchemaOwner::BuiltinObject,
            PropertySchemaProvenance::HandAuthoredRust,
            PropertyTableMode::Static,
            ENTRIES,
        );

        let analysis = analyze_static_property_table(&TABLE).unwrap();

        assert_eq!(analysis.entry_count, 2);
        assert_eq!(analysis.data_count, 1);
        assert_eq!(analysis.custom_accessor_count, 1);
        assert_eq!(analysis.max_offset, Some(PropertyOffset::new(0)));
    }

    #[test]
    fn static_property_enumeration_orders_indices_before_strings() {
        static ENTRIES: &[PropertyDescriptor] = &[
            PropertyDescriptor {
                key: PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
                    1,
                ))),
                offset: PropertyOffset::new(0),
                attributes: PropertyAttributes::DATA_DEFAULT,
                location: PropertyLocation::InlineOrOutOfLine,
                initial_value_hint: None,
                kind: PropertyDescriptorKind::Data,
                state: PropertyDescriptorState::Complete,
                value: PropertyValueDescriptor::Data(DataDescriptor {
                    value: None,
                    writable: Some(true),
                }),
            },
            PropertyDescriptor {
                key: PropertyKey::from_index(crate::strings::PropertyIndex::from_canonical_index(
                    1,
                )),
                offset: PropertyOffset::new(1),
                attributes: PropertyAttributes::DATA_DEFAULT,
                location: PropertyLocation::InlineOrOutOfLine,
                initial_value_hint: None,
                kind: PropertyDescriptorKind::Data,
                state: PropertyDescriptorState::Complete,
                value: PropertyValueDescriptor::Data(DataDescriptor {
                    value: None,
                    writable: Some(true),
                }),
            },
        ];
        static TABLE: StaticPropertyTableDescriptor = StaticPropertyTableDescriptor::new(
            "static",
            PropertySchemaOwner::BuiltinObject,
            PropertySchemaProvenance::HandAuthoredRust,
            PropertyTableMode::Static,
            ENTRIES,
        );

        let records = enumerate_static_property_table(
            &TABLE,
            crate::strings::PropertyNameMode::Strings,
            crate::strings::PrivateSymbolMode::Exclude,
        )
        .unwrap();

        assert_eq!(records[0].bucket, EnumerationBucket::ArrayIndex);
        assert_eq!(records[1].bucket, EnumerationBucket::StringProperty);
    }
}
