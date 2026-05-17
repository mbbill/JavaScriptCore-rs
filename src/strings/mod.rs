//! String, symbol, identifier, and property-key contracts.
//!
//! This module owns the Rust-side skeleton for JSC's uniqued names and runtime
//! strings. It intentionally does not intern text, flatten ropes, or implement
//! property lookup. Those behaviors must be added behind VM/GC-aware APIs.

mod atom;
mod property_key;
mod string;
mod symbol;

pub use atom::{
    AtomDomain, AtomId, AtomLifetime, AtomTable, AtomTableId, AtomTableMutation, AtomTableScope,
    CacheableIdentifier, CacheableIdentifierClassification, CacheableIdentifierOwner,
    CacheableIdentifierStorage, CommonIdentifier, CommonIdentifierKind, CommonIdentifierSlot,
    Identifier, UniquedIdentifier,
};
pub use property_key::{
    NumericPropertyKeyKind, PrivateSymbolMode, PropertyIndex, PropertyKey,
    PropertyKeyClassification, PropertyKeyConversion, PropertyKeyConverter, PropertyKeyKind,
    PropertyNameFilter, PropertyNameMode,
};
pub use string::{
    AtomizationMode, ExternalString, ExternalStringOwner, ExternalStringPinning, FlatString,
    FlatStringStorage, JsString, RopeArity, RopeFiber, RopeFiberOwnership, RopeKind, RopePiece,
    RopePieceKind, RopeResolutionMode, RopeResolutionOutcome, RopeString, RuntimeStringKind,
    StringAtomizationState, StringCellState, StringEncoding, StringInterner, StringLength,
    StringOwnership, SubstringBase, SubstringString,
};
pub use symbol::{
    PrivateName, SymbolCell, SymbolDescription, SymbolIdentity, SymbolKind, SymbolRegistryId,
    SymbolRegistryKind, SymbolUid, SymbolUidAllocator, SymbolUniquenessDomain,
};
