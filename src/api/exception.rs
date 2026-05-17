use crate::api::handles::ApiValueRef;

/// How an API operation reported exception state.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiThrowDisposition {
    DidNotThrow,
    PendingException,
    Terminated,
}

/// Nullability and ownership contract for an exception out-parameter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiExceptionSlotPolicy {
    IgnoredWhenNull,
    ClearedBeforeCall,
    WrittenOnlyOnThrow,
}

/// Exception out-parameter slot.
///
/// This models `JSValueRef* exception` without exposing a raw pointer. The C ABI
/// shim owns pointer validation and null checks; Rust entry code should traffic
/// in this structured slot after the API lock has been acquired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExceptionSlot {
    policy: ApiExceptionSlotPolicy,
    current: Option<ApiValueRef>,
}

impl ApiExceptionSlot {
    pub const fn new(policy: ApiExceptionSlotPolicy) -> Self {
        Self {
            policy,
            current: None,
        }
    }

    pub const fn with_current(policy: ApiExceptionSlotPolicy, current: ApiValueRef) -> Self {
        Self {
            policy,
            current: Some(current),
        }
    }

    pub const fn policy(self) -> ApiExceptionSlotPolicy {
        self.policy
    }

    pub const fn current(self) -> Option<ApiValueRef> {
        self.current
    }
}

/// Exception out-parameter bridge.
///
/// This mirrors the public C API pattern without deciding final `JSValueRef`
/// bit-compatibility. Setting this result must stay synchronized with VM
/// pending-exception state.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExceptionResult {
    disposition: ApiThrowDisposition,
    exception: Option<ApiValueRef>,
}

impl ApiExceptionResult {
    pub const fn none() -> Self {
        Self {
            disposition: ApiThrowDisposition::DidNotThrow,
            exception: None,
        }
    }

    pub const fn pending(exception: ApiValueRef) -> Self {
        Self {
            disposition: ApiThrowDisposition::PendingException,
            exception: Some(exception),
        }
    }

    pub const fn disposition(self) -> ApiThrowDisposition {
        self.disposition
    }

    pub const fn exception(self) -> Option<ApiValueRef> {
        self.exception
    }
}

/// API operation result that can carry either a value or exception metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiOperationResult<T> {
    Value(T),
    Exception(ApiExceptionResult),
}
