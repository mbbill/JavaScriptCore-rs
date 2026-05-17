//! DOMJIT contracts.
//!
//! DOMJIT gives embedders a way to expose host-side structure and call
//! knowledge to optimizing tiers. This module records that trust boundary
//! without embedding WebCore or generating host stubs.

use crate::runtime::{HostHookId, ObjectId};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DomJitSignatureId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomJitEffect {
    Pure,
    ReadsWorld,
    WritesWorld,
    MayCallScript,
    MayThrow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomJitCallSignature {
    pub id: DomJitSignatureId,
    pub hook: HostHookId,
    pub receiver: Option<ObjectId>,
    pub effect: DomJitEffect,
    pub argument_count: u16,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DomJitStructurePlan {
    pub signature: Option<DomJitSignatureId>,
    pub requires_watchpoint: bool,
    pub can_inline_load: bool,
    pub can_inline_call: bool,
}
