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
}
