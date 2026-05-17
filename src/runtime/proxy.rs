use crate::runtime::exception::JsResult;
use crate::runtime::function::{CallData, ConstructData};
use crate::runtime::property::{
    DeletePropertySlot, OwnPropertyKeysResult, PropertyDescriptor, PropertySlot, PutPropertySlot,
    RuntimePropertyKey,
};
use crate::runtime::state::{ObjectId, RuntimeValue, StructureId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyObject {
    /// Proxy internal fields and trap-cache state.
    ///
    /// Revocation clears target/handler semantics. Every trap result must be
    /// validated against target invariants before it is exposed to callers.
    pub object: Option<ObjectId>,
    pub target: RuntimeValue,
    pub handler: RuntimeValue,
    pub callable: bool,
    pub constructible: bool,
    pub trap_cache: ProxyTrapCache,
    pub revoked: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyTrapCache {
    pub handler_structure: Option<StructureId>,
    pub handler_prototype_structure: Option<StructureId>,
    pub cached_offsets: Vec<CachedProxyTrap>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CachedProxyTrap {
    pub trap: ProxyTrap,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ProxyTrap {
    GetPrototypeOf,
    SetPrototypeOf,
    IsExtensible,
    PreventExtensions,
    GetOwnPropertyDescriptor,
    DefineProperty,
    Has,
    #[default]
    Get,
    Set,
    DeleteProperty,
    OwnKeys,
    Apply,
    Construct,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyTrapCall {
    pub proxy: ObjectId,
    pub trap: ProxyTrap,
    pub handler: ObjectId,
    pub target: ObjectId,
    pub key: Option<RuntimePropertyKey>,
    pub value: RuntimeValue,
    pub receiver: RuntimeValue,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ProxyTrapResult {
    #[default]
    MissingTrap,
    Value(RuntimeValue),
    Boolean(bool),
    Descriptor(Option<PropertyDescriptor>),
    OwnKeys(OwnPropertyKeysResult),
    CallData(CallData),
    ConstructData(ConstructData),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyInvariantCheck {
    pub proxy: ObjectId,
    pub target: ObjectId,
    pub trap: ProxyTrap,
    pub key: Option<RuntimePropertyKey>,
    pub trap_result: ProxyTrapResult,
}

/// Proxy internal methods and invariant validation boundary.
pub trait ProxyOperations {
    fn get_handler_trap(&self, proxy: ObjectId, trap: ProxyTrap) -> JsResult<Option<ObjectId>>;
    fn call_trap(&mut self, call: ProxyTrapCall) -> JsResult<ProxyTrapResult>;
    fn validate_trap_result(&self, check: ProxyInvariantCheck) -> JsResult<bool>;
    fn revoke_proxy(&mut self, proxy: ObjectId) -> JsResult<()>;
    fn proxy_get(
        &mut self,
        proxy: ObjectId,
        key: RuntimePropertyKey,
        receiver: RuntimeValue,
    ) -> JsResult<PropertySlot>;
    fn proxy_set(
        &mut self,
        proxy: ObjectId,
        key: RuntimePropertyKey,
        value: RuntimeValue,
        receiver: RuntimeValue,
        slot: PutPropertySlot,
    ) -> JsResult<bool>;
    fn proxy_delete(
        &mut self,
        proxy: ObjectId,
        key: RuntimePropertyKey,
        slot: DeletePropertySlot,
    ) -> JsResult<bool>;
}
