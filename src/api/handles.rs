use core::ffi::c_void;
use core::ptr::NonNull;

/// Public opaque reference kind exposed by the C API.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ApiHandleKind {
    ContextGroup,
    GlobalContext,
    Context,
    Value,
    Object,
    String,
    Class,
    PropertyNameArray,
    PropertyNameAccumulator,
    Script,
    WeakValue,
}

/// Stable classification for `JSValueRef` without exposing encoded JS bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiValueRefKind {
    Immediate,
    Cell,
    Object,
    String,
    Symbol,
    BigInt,
    Exception,
}

/// Ownership rule for an opaque API reference crossing the C boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiReferenceOwnership {
    BorrowedForCall,
    RetainedRef,
    ProtectedGcValue,
    CopiedString,
    WeakObservedValue,
}

/// Shared opaque handle representation for C ABI references.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ApiOpaqueHandle {
    raw: NonNull<c_void>,
}

impl ApiOpaqueHandle {
    /// Creates an API handle from an opaque non-null pointer.
    ///
    /// # Safety
    ///
    /// The pointer must refer to the internal object kind expected by the
    /// wrapper that receives this handle, must belong to the correct context
    /// group, and must remain alive according to API retain/protect rules.
    pub const unsafe fn from_raw(raw: NonNull<c_void>) -> Self {
        Self { raw }
    }

    pub const fn as_raw(self) -> NonNull<c_void> {
        self.raw
    }
}

/// Audited Rust-side view of a C-facing opaque reference.
///
/// This deliberately separates "a non-null pointer passed across the C ABI"
/// from "a pointer that has been audited as the expected internal type." The
/// latter still does not own the VM object; ownership remains with ref-counted
/// API objects, GC protection, or the context group.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ApiInternalHandle {
    raw: NonNull<c_void>,
    kind: ApiHandleKind,
}

impl ApiInternalHandle {
    pub const fn new(raw: NonNull<c_void>, kind: ApiHandleKind) -> Self {
        Self { raw, kind }
    }

    pub const fn raw(self) -> NonNull<c_void> {
        self.raw
    }

    pub const fn kind(self) -> ApiHandleKind {
        self.kind
    }
}

/// Direction of a conversion between internal engine values and API refs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiCastDirection {
    ToInternal,
    ToOpaqueRef,
    ForGarbageCollection,
}

/// Contract recorded for an API cast.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiCastContract {
    expected_kind: ApiHandleKind,
    direction: ApiCastDirection,
    requires_lock: bool,
    ownership: ApiReferenceOwnership,
}

impl ApiCastContract {
    pub const fn new(
        expected_kind: ApiHandleKind,
        direction: ApiCastDirection,
        requires_lock: bool,
    ) -> Self {
        Self {
            expected_kind,
            direction,
            requires_lock,
            ownership: ApiReferenceOwnership::BorrowedForCall,
        }
    }

    pub const fn with_ownership(
        expected_kind: ApiHandleKind,
        direction: ApiCastDirection,
        requires_lock: bool,
        ownership: ApiReferenceOwnership,
    ) -> Self {
        Self {
            expected_kind,
            direction,
            requires_lock,
            ownership,
        }
    }

    pub const fn expected_kind(self) -> ApiHandleKind {
        self.expected_kind
    }

    pub const fn direction(self) -> ApiCastDirection {
        self.direction
    }

    pub const fn requires_lock(self) -> bool {
        self.requires_lock
    }

    pub const fn ownership(self) -> ApiReferenceOwnership {
        self.ownership
    }
}

macro_rules! api_handle {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub struct $name {
            handle: ApiOpaqueHandle,
        }

        impl $name {
            /// Rebuilds a typed API handle from an opaque handle.
            ///
            /// # Safety
            ///
            /// The opaque handle must have the exact internal kind represented
            /// by this wrapper and must satisfy the API locking and lifetime
            /// rules for the current entry scope.
            pub const unsafe fn from_opaque(handle: ApiOpaqueHandle) -> Self {
                Self { handle }
            }

            pub const fn as_opaque(self) -> ApiOpaqueHandle {
                self.handle
            }
        }
    };
}

api_handle!(
    ApiContextGroup,
    "Opaque context-group handle for VM-like shared state."
);
api_handle!(ApiContextRef, "Opaque execution context handle.");
api_handle!(ApiGlobalContext, "Opaque global context or realm handle.");
api_handle!(ApiValueRef, "Opaque JavaScript value reference.");
api_handle!(
    ApiObjectRef,
    "Opaque object-shaped JavaScript value reference."
);
api_handle!(ApiStringRef, "Opaque API string reference.");
api_handle!(ApiClassRef, "Opaque host class metadata reference.");
api_handle!(
    ApiPropertyNameArrayRef,
    "Opaque retained array of copied property names."
);
api_handle!(
    ApiPropertyNameAccumulatorRef,
    "Opaque transient accumulator supplied to property enumeration callbacks."
);
api_handle!(ApiScriptRef, "Opaque compiled script reference.");
api_handle!(ApiWeakValueRef, "Opaque weak JavaScript value reference.");
