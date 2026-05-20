use crate::runtime::exception::JsResult;
use crate::runtime::function::{CallData, ConstructData};
use crate::runtime::property::{
    DeletePropertySlot, OwnPropertyKeysResult, PropertyDescriptor, PropertySlot, PutPropertySlot,
    RuntimePropertyAccessKey,
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
    pub key: Option<RuntimePropertyAccessKey>,
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
    pub key: Option<RuntimePropertyAccessKey>,
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
        key: RuntimePropertyAccessKey,
        receiver: RuntimeValue,
    ) -> JsResult<PropertySlot>;
    fn proxy_set(
        &mut self,
        proxy: ObjectId,
        key: RuntimePropertyAccessKey,
        value: RuntimeValue,
        receiver: RuntimeValue,
        slot: PutPropertySlot,
    ) -> JsResult<bool>;
    fn proxy_delete(
        &mut self,
        proxy: ObjectId,
        key: RuntimePropertyAccessKey,
        slot: DeletePropertySlot,
    ) -> JsResult<bool>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProxyTrapPlan {
    ForwardToTarget,
    CallTrap(ProxyTrap),
    Revoked,
    NotCallable,
    NotConstructible,
}

impl ProxyObject {
    pub fn plan_trap(&self, trap: ProxyTrap) -> ProxyTrapPlan {
        if self.revoked {
            return ProxyTrapPlan::Revoked;
        }
        match trap {
            ProxyTrap::Apply if !self.callable => ProxyTrapPlan::NotCallable,
            ProxyTrap::Construct if !self.constructible => ProxyTrapPlan::NotConstructible,
            _ => {
                if self
                    .trap_cache
                    .cached_offsets
                    .iter()
                    .any(|cached| cached.trap == trap)
                {
                    ProxyTrapPlan::CallTrap(trap)
                } else {
                    ProxyTrapPlan::ForwardToTarget
                }
            }
        }
    }
}

pub fn proxy_boolean_trap_accepted(result: &ProxyTrapResult) -> Option<bool> {
    match result {
        ProxyTrapResult::Boolean(value) => Some(*value),
        ProxyTrapResult::MissingTrap => None,
        _ => Some(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoked_proxy_rejects_every_trap_plan() {
        let proxy = ProxyObject {
            revoked: true,
            ..ProxyObject::default()
        };

        assert_eq!(proxy.plan_trap(ProxyTrap::Get), ProxyTrapPlan::Revoked);
    }

    #[test]
    fn cached_trap_plans_handler_call() {
        let proxy = ProxyObject {
            trap_cache: ProxyTrapCache {
                cached_offsets: vec![CachedProxyTrap {
                    trap: ProxyTrap::Set,
                    offset: 1,
                }],
                ..ProxyTrapCache::default()
            },
            ..ProxyObject::default()
        };

        assert_eq!(
            proxy.plan_trap(ProxyTrap::Set),
            ProxyTrapPlan::CallTrap(ProxyTrap::Set)
        );
        assert_eq!(
            proxy.plan_trap(ProxyTrap::Get),
            ProxyTrapPlan::ForwardToTarget
        );
    }
}
