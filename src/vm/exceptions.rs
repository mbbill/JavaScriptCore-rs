//! Pending exception and termination state.

use crate::value::JsValue;

/// Opaque identity for exception scopes used to enforce checking discipline.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct ExceptionScopeId(pub u64);

/// Pending JavaScript exception value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingException {
    pub value: JsValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationReason {
    Watchdog,
    HostRequest,
    OutOfMemory,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExceptionCheckState {
    #[default]
    Clean,
    PendingCheck,
    Suspended,
}

/// VM-wide exception and termination state.
#[derive(Clone, Debug, Default)]
pub struct ExceptionState {
    pending: Option<PendingException>,
    last: Option<PendingException>,
    termination: Option<TerminationReason>,
    check_state: ExceptionCheckState,
    scope_depth: usize,
}

impl ExceptionState {
    pub fn pending(&self) -> Option<PendingException> {
        self.pending
    }

    pub fn throw(&mut self, value: JsValue) {
        let pending = PendingException { value };
        self.pending = Some(pending);
        self.last = Some(pending);
        self.check_state = ExceptionCheckState::PendingCheck;
    }

    pub fn clear_pending(&mut self) -> Option<PendingException> {
        let pending = self.pending.take();
        if pending.is_some() {
            self.check_state = ExceptionCheckState::Clean;
        }
        pending
    }

    pub fn last(&self) -> Option<PendingException> {
        self.last
    }

    pub fn request_termination(&mut self, reason: TerminationReason) {
        self.termination = Some(reason);
        self.check_state = ExceptionCheckState::PendingCheck;
    }

    pub fn termination(&self) -> Option<TerminationReason> {
        self.termination
    }

    pub fn check_state(&self) -> ExceptionCheckState {
        self.check_state
    }

    pub fn scope_depth(&self) -> usize {
        self.scope_depth
    }

    pub fn enter_scope(&mut self) -> ExceptionScopeId {
        self.scope_depth = self.scope_depth.saturating_add(1);
        ExceptionScopeId(self.scope_depth as u64)
    }

    pub fn leave_scope(&mut self, _scope: ExceptionScopeId) {
        self.scope_depth = self.scope_depth.saturating_sub(1);
    }
}
