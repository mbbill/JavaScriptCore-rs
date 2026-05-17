//! Property keys, offsets, and metadata.
//!
//! Property key identity is owned by `strings`; object storage only records
//! handles to that identity plus offsets and descriptor metadata.

pub use crate::strings::{AtomId, PropertyKey};
use crate::strings::{PrivateName, SymbolUid};
use crate::value::JsValue;

pub type SymbolId = SymbolUid;
pub type PrivateNameId = PrivateName;

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
}

impl PropertyAttributes {
    pub const DATA_DEFAULT: Self = Self {
        writable: true,
        enumerable: true,
        configurable: true,
        accessor: false,
        read_only_for_structures: false,
        dont_delete_for_caches: false,
    };

    pub const ACCESSOR_DEFAULT: Self = Self {
        writable: false,
        enumerable: true,
        configurable: true,
        accessor: true,
        read_only_for_structures: false,
        dont_delete_for_caches: false,
    };

    pub const fn is_data(self) -> bool {
        !self.accessor
    }

    pub const fn is_accessor(self) -> bool {
        self.accessor
    }
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
    pub name: PrivateNameId,
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
}

/// Ownership mode for property metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PropertyTableMode {
    #[default]
    Shape,
    Dictionary,
    Static,
}
