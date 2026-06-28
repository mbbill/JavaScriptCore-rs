//! String, symbol, identifier, and property-key contracts.
//!
//! This module owns the Rust-side skeleton for JSC's uniqued names and runtime
//! strings. It intentionally does not intern text, flatten ropes, or implement
//! property lookup. Those behaviors must be added behind VM/GC-aware APIs.

mod atom;
mod property_key;
mod string;
mod string_impl;
mod symbol;

pub use atom::{
    plan_atom_registry_interning, validate_atom_registry_descriptor, AtomDomain, AtomId,
    AtomInterningAction, AtomInterningPlanEntry, AtomLifetime, AtomRegistryDescriptor,
    AtomRegistryDescriptorBuilder, AtomRegistryOwner, AtomRegistryProvenance,
    AtomRegistryValidationError, AtomTable, AtomTableId, AtomTableMutation, AtomTableScope,
    CacheableIdentifier, CacheableIdentifierClassification, CacheableIdentifierOwner,
    CacheableIdentifierStorage, CommonIdentifier, CommonIdentifierDescriptor, CommonIdentifierKind,
    CommonIdentifierSlot, Identifier, UniquedIdentifier,
};
pub use property_key::{
    classify_property_key, classify_property_name_text, NumericPropertyKeyKind, PrivateSymbolMode,
    PropertyIndex, PropertyKey, PropertyKeyClassification, PropertyKeyConversion,
    PropertyKeyConverter, PropertyKeyKind, PropertyNameFilter, PropertyNameMode,
};
pub use string::{
    canonicalize_static_string_registry, validate_external_string, validate_flat_string,
    validate_static_string_descriptor, validate_string_registry_descriptor,
    validate_substring_string, AtomizationMode, ExternalString, ExternalStringOwner,
    ExternalStringPinning, FlatString, FlatStringStorage, JsString, RopeArity, RopeFiber,
    RopeFiberMutationAuthority, RopeFiberOwnership, RopeKind, RopePiece, RopePieceKind,
    RopeResolutionMode, RopeResolutionOutcome, RopeString, RuntimeStringKind,
    StaticStringCanonicalForm, StaticStringCanonicalization, StaticStringDescriptor,
    StaticStringDescriptorBuilder, StringAtomizationState, StringBorrowOwner, StringBorrowScope,
    StringCellState, StringEncoding, StringId, StringInterner, StringLength, StringOwnership,
    StringRegistryDescriptor, StringRegistryDescriptorBuilder, StringRegistryOwner,
    StringRegistryProvenance, StringValidationError, SubstringBase, SubstringString,
};
pub use string_impl::{BufferOwnership, StringData, StringImpl, StringKind, MAX_LENGTH};
pub use symbol::{
    validate_symbol_registry_descriptor, validate_symbol_registry_entry, PrivateName, SymbolCell,
    SymbolDescription, SymbolIdentity, SymbolKind, SymbolRegistryDescriptor,
    SymbolRegistryDescriptorBuilder, SymbolRegistryEntryDescriptor, SymbolRegistryId,
    SymbolRegistryKind, SymbolRegistryOwner, SymbolRegistryProvenance,
    SymbolRegistryValidationError, SymbolUid, SymbolUidAllocator, SymbolUniquenessDomain,
};
