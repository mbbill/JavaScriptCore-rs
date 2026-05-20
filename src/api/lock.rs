use crate::api::exception::{ApiExceptionSemanticError, ApiExecutionResultRecord};
use crate::api::handles::ApiContextGroup;

/// API reentrancy state observed by entry scopes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiReentrancy {
    Entering,
    Reentered,
}

/// Recursive API lock state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiLockState {
    Unlocked,
    Held { depth: u32 },
    TemporarilyDropped { depth: u32 },
}

/// Lock acquisition policy for an API entry point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiLockPolicy {
    MustAcquire,
    AlreadyHeld,
    MayDropForHostCall,
    FinalizerNoAllocation,
    CallbackMayReenter,
    DebuggerEntry,
}

/// Public API operation that determines lock and exception discipline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiEntryKind {
    EvaluateScript,
    CheckSyntax,
    ValueConversion,
    ObjectPropertyAccess,
    CallbackInvocation,
    ClassFinalization,
    GarbageCollectionProtection,
    DebuggerInspection,
}

/// Snapshot used by `DropAllLocks`-style scopes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiLockDropState {
    context_group: ApiContextGroup,
    dropped_depth: u32,
    may_reacquire: bool,
}

impl ApiLockDropState {
    pub const fn new(context_group: ApiContextGroup, dropped_depth: u32) -> Self {
        Self {
            context_group,
            dropped_depth,
            may_reacquire: true,
        }
    }

    pub const fn finalizer_drop(context_group: ApiContextGroup, dropped_depth: u32) -> Self {
        Self {
            context_group,
            dropped_depth,
            may_reacquire: false,
        }
    }

    pub const fn dropped_depth(self) -> u32 {
        self.dropped_depth
    }

    pub const fn may_reacquire(self) -> bool {
        self.may_reacquire
    }
}

/// API lock and entry discipline token.
///
/// Every public API entry point must acquire or verify this lock before touching
/// VM, realm, value, object, string, exception, or protection state. The final
/// locking primitive is unresolved and may be a Rust guard, C ABI shim, or both.
#[derive(Debug)]
pub struct ApiLock {
    context_group: ApiContextGroup,
    policy: ApiLockPolicy,
}

impl ApiLock {
    pub const fn for_context_group(context_group: ApiContextGroup) -> Self {
        Self {
            context_group,
            policy: ApiLockPolicy::MustAcquire,
        }
    }

    pub const fn with_policy(context_group: ApiContextGroup, policy: ApiLockPolicy) -> Self {
        Self {
            context_group,
            policy,
        }
    }

    pub const fn context_group(&self) -> ApiContextGroup {
        self.context_group
    }

    pub const fn policy(&self) -> ApiLockPolicy {
        self.policy
    }
}

/// Scoped proof that an API call is inside the lock and VM entry boundary.
#[derive(Debug)]
pub struct ApiEntryScope<'lock> {
    lock: &'lock ApiLock,
    reentrancy: ApiReentrancy,
    lock_state: ApiLockState,
    entry_kind: ApiEntryKind,
}

impl<'lock> ApiEntryScope<'lock> {
    pub const fn new(lock: &'lock ApiLock, reentrancy: ApiReentrancy) -> Self {
        Self {
            lock,
            reentrancy,
            lock_state: ApiLockState::Held { depth: 1 },
            entry_kind: ApiEntryKind::EvaluateScript,
        }
    }

    pub const fn with_state(
        lock: &'lock ApiLock,
        reentrancy: ApiReentrancy,
        lock_state: ApiLockState,
    ) -> Self {
        Self {
            lock,
            reentrancy,
            lock_state,
            entry_kind: ApiEntryKind::EvaluateScript,
        }
    }

    pub const fn for_entry_kind(
        lock: &'lock ApiLock,
        reentrancy: ApiReentrancy,
        lock_state: ApiLockState,
        entry_kind: ApiEntryKind,
    ) -> Self {
        Self {
            lock,
            reentrancy,
            lock_state,
            entry_kind,
        }
    }

    pub const fn lock(&self) -> &'lock ApiLock {
        self.lock
    }

    pub const fn reentrancy(&self) -> ApiReentrancy {
        self.reentrancy
    }

    pub const fn lock_state(&self) -> ApiLockState {
        self.lock_state
    }

    pub const fn entry_kind(&self) -> ApiEntryKind {
        self.entry_kind
    }

    pub const fn observe_entry(&self) -> ApiEntryObservationRecord {
        ApiEntryObservationRecord {
            context_group: self.lock.context_group(),
            entry_kind: self.entry_kind,
            lock_policy: self.lock.policy(),
            lock_state: self.lock_state,
            reentrancy: self.reentrancy,
        }
    }

    pub fn observe_exit(
        &self,
        result: ApiExecutionResultRecord,
    ) -> Result<ApiExitObservationRecord, ApiExceptionSemanticError> {
        result.validate()?;
        Ok(ApiExitObservationRecord {
            entry: self.observe_entry(),
            result,
            lock_state_after_exit: self.lock_state,
        })
    }
}

/// API entry boundary observed after lock and reentrancy state are known.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiEntryObservationRecord {
    pub context_group: ApiContextGroup,
    pub entry_kind: ApiEntryKind,
    pub lock_policy: ApiLockPolicy,
    pub lock_state: ApiLockState,
    pub reentrancy: ApiReentrancy,
}

/// API exit boundary observed after VM-facing work returned to API code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExitObservationRecord {
    pub entry: ApiEntryObservationRecord,
    pub result: ApiExecutionResultRecord,
    pub lock_state_after_exit: ApiLockState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::handles::ApiOpaqueHandle;
    use core::ffi::c_void;
    use core::ptr::NonNull;

    fn context_group() -> ApiContextGroup {
        let raw = NonNull::<c_void>::dangling();
        let handle = unsafe { ApiOpaqueHandle::from_raw(raw) };
        unsafe { ApiContextGroup::from_opaque(handle) }
    }

    #[test]
    fn records_api_entry_and_exit_boundary() {
        let lock = ApiLock::with_policy(context_group(), ApiLockPolicy::MustAcquire);
        let scope = ApiEntryScope::for_entry_kind(
            &lock,
            ApiReentrancy::Entering,
            ApiLockState::Held { depth: 1 },
            ApiEntryKind::EvaluateScript,
        );

        let exit = scope
            .observe_exit(ApiExecutionResultRecord::returned_void())
            .expect("exit observation");

        assert_eq!(exit.entry.entry_kind, ApiEntryKind::EvaluateScript);
        assert_eq!(exit.entry.lock_policy, ApiLockPolicy::MustAcquire);
        assert_eq!(exit.result, ApiExecutionResultRecord::returned_void());
    }
}
