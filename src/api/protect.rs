use crate::api::handles::{ApiContextGroup, ApiValueRef};

/// VM heap slot used to count API protections.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ApiProtectionSlot(u32);

impl ApiProtectionSlot {
    pub const fn from_heap_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn heap_slot(self) -> u32 {
        self.0
    }
}

/// Owner of API protection/rooting state.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ApiProtectionOwner {
    context_group: ApiContextGroup,
}

impl ApiProtectionOwner {
    pub const fn new(context_group: ApiContextGroup) -> Self {
        Self { context_group }
    }
}

/// Balance action applied to a protected value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiProtectionAction {
    Protect,
    Unprotect,
}

/// Counter discipline for `JSValueProtect` / `JSValueUnprotect`.
///
/// A value may be protected multiple times. Rust ownership cannot express this
/// by itself because the C API exposes balanced calls rather than RAII-only
/// lifetimes, so the count remains VM heap state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiProtectionCount {
    slot: ApiProtectionSlot,
    count: u32,
}

impl ApiProtectionCount {
    pub const fn new(slot: ApiProtectionSlot, count: u32) -> Self {
        Self { slot, count }
    }

    pub const fn slot(self) -> ApiProtectionSlot {
        self.slot
    }

    pub const fn count(self) -> u32 {
        self.count
    }
}

/// Protected/rooted API value.
///
/// A protected value keeps a GC value alive until the embedder balances the
/// protection. The concrete root slot belongs to GC/VM integration and must not
/// be represented by ordinary Rust ownership alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtectedValue {
    owner: ApiProtectionOwner,
    value: ApiValueRef,
    slot: Option<ApiProtectionSlot>,
}

impl ProtectedValue {
    pub const fn from_protected_slot(owner: ApiProtectionOwner, value: ApiValueRef) -> Self {
        Self {
            owner,
            value,
            slot: None,
        }
    }

    pub const fn with_slot(
        owner: ApiProtectionOwner,
        value: ApiValueRef,
        slot: ApiProtectionSlot,
    ) -> Self {
        Self {
            owner,
            value,
            slot: Some(slot),
        }
    }

    pub const fn value(self) -> ApiValueRef {
        self.value
    }

    pub const fn owner(self) -> ApiProtectionOwner {
        self.owner
    }

    pub const fn slot(self) -> Option<ApiProtectionSlot> {
        self.slot
    }
}
