use crate::runtime::exception::JsResult;
use crate::runtime::state::{ObjectId, RuntimeValue, StringId, SymbolId, WatchpointGeneration};

/// Property keys as seen by ECMAScript internal methods.
///
/// Fast paths may carry indexes separately, but observable semantics must still
/// route through the same key categories for proxies, typed arrays, private
/// fields, and ordinary named properties.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RuntimePropertyKey {
    String(StringId),
    Symbol(SymbolId),
    ArrayIndex(u32),
    IntegerIndex(u64),
    PrivateName(SymbolId),
}

impl Default for RuntimePropertyKey {
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
    pub keys: Vec<RuntimePropertyKey>,
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
        key: RuntimePropertyKey,
    ) -> JsResult<Option<PropertyDescriptor>>;
    fn define_own_property(
        &mut self,
        object: ObjectId,
        key: RuntimePropertyKey,
        descriptor: PropertyDescriptor,
        should_throw: bool,
    ) -> JsResult<bool>;
    fn has_property(&self, object: ObjectId, key: RuntimePropertyKey) -> JsResult<bool>;
    fn get(
        &self,
        object: ObjectId,
        key: RuntimePropertyKey,
        receiver: RuntimeValue,
    ) -> JsResult<PropertySlot>;
    fn set(
        &mut self,
        object: ObjectId,
        key: RuntimePropertyKey,
        value: RuntimeValue,
        receiver: RuntimeValue,
        slot: PutPropertySlot,
    ) -> JsResult<bool>;
    fn delete(
        &mut self,
        object: ObjectId,
        key: RuntimePropertyKey,
        slot: DeletePropertySlot,
    ) -> JsResult<bool>;
    fn own_property_keys(
        &self,
        object: ObjectId,
        request: OwnPropertyKeysRequest,
    ) -> JsResult<OwnPropertyKeysResult>;
}
