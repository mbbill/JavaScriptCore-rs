use crate::runtime::exception::{ExceptionId, JsResult};
use crate::runtime::jobs::InternalMicrotaskKind;
use crate::runtime::state::{ObjectId, RuntimeValue};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PromiseId(pub ObjectId);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsPromise {
    /// Promise state, payload, and compact inline reaction metadata.
    ///
    /// JSC stores this in two machine words. The Rust skeleton keeps the same
    /// split between settlement value, flags, and pending reactions without
    /// committing to a representation.
    pub promise: Option<PromiseId>,
    pub state: PromiseState,
    pub flags: PromiseFlags,
    pub payload: PromisePayload,
    pub reaction: PromiseReactionStorage,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PromiseState {
    #[default]
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PromiseFlags {
    pub is_handled: bool,
    pub first_resolving_function_called: bool,
}

/// Compact flag layout for a promise cell.
///
/// C++ stores status, handled state, resolving-function state, inline reaction
/// kind, and an internal microtask discriminator in the upper bits of a packed
/// pointer word. Rust code must treat this as a representation contract, not as
/// authority to mutate promise state without the VM barrier path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PromisePackedFlags {
    pub state: PromiseState,
    pub is_handled: bool,
    pub first_resolving_function_called: bool,
    pub inline_reaction_kind: PromiseInlineReactionKind,
    pub inline_microtask: Option<InternalMicrotaskKind>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PromiseInlineReactionKind {
    #[default]
    None,
    InternalMicrotask,
    FulfillHandler,
    RejectHandler,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PromisePayload {
    #[default]
    Empty,
    Settlement(RuntimeValue),
    ReactionList(PromiseReactionId),
    InlineReactionContext(RuntimeValue),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PromiseReactionId(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PromiseReactionStorage {
    #[default]
    None,
    InlineInternalMicrotask {
        kind: InternalMicrotaskKind,
        result_promise: PromiseId,
        context: RuntimeValue,
    },
    InlineHandler {
        kind: PromiseReactionKind,
        result_promise: PromiseId,
        handler: RuntimeValue,
    },
    List(PromiseReactionId),
}

/// Heap cell shape for spilled promise reactions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PromiseReactionCellKind {
    #[default]
    Slim,
    Full,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PromiseReactionKind {
    #[default]
    Fulfill,
    Reject,
    Finally,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromiseReaction {
    pub id: Option<PromiseReactionId>,
    pub cell_kind: PromiseReactionCellKind,
    pub kind: PromiseReactionKind,
    pub capability: Option<PromiseCapability>,
    pub handler: RuntimeValue,
    pub secondary_handler: RuntimeValue,
    pub context: RuntimeValue,
    pub next: Option<PromiseReactionId>,
    pub internal_microtask: Option<InternalMicrotaskKind>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromiseCapability {
    pub promise: PromiseId,
    pub resolve: ObjectId,
    pub reject: ObjectId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromiseResolvingFunctions {
    pub promise: PromiseId,
    pub resolve: ObjectId,
    pub reject: ObjectId,
    pub first_call_guard_shared: bool,
    pub internal_microtask: Option<InternalMicrotaskKind>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThenableJob {
    pub promise_to_resolve: PromiseId,
    pub thenable: RuntimeValue,
    pub then: ObjectId,
    pub internal_microtask: Option<InternalMicrotaskKind>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromiseCombinatorContext {
    pub kind: PromiseCombinatorKind,
    pub result_capability: PromiseCapability,
    pub remaining_elements: u32,
    pub values: Option<ObjectId>,
    pub already_called_flags: Option<ObjectId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PromiseCombinatorKind {
    #[default]
    All,
    AllSettled,
    Any,
    Race,
}

/// Abstract Promise operations and Web IDL hooks.
pub trait PromiseOperations {
    fn resolve_promise(&mut self, promise: PromiseId, resolution: RuntimeValue) -> JsResult<()>;
    fn fulfill_promise(&mut self, promise: PromiseId, value: RuntimeValue) -> JsResult<()>;
    fn reject_promise(&mut self, promise: PromiseId, reason: RuntimeValue) -> JsResult<()>;
    fn reject_with_exception(&mut self, promise: PromiseId, exception: ExceptionId)
        -> JsResult<()>;
    fn perform_promise_then(
        &mut self,
        promise: PromiseId,
        on_fulfilled: RuntimeValue,
        on_rejected: RuntimeValue,
        result_capability: Option<PromiseCapability>,
    ) -> JsResult<ObjectId>;
    fn new_promise_capability(&mut self, constructor: RuntimeValue) -> JsResult<PromiseCapability>;
    fn enqueue_promise_reaction_job(
        &mut self,
        reaction: PromiseReaction,
        argument: RuntimeValue,
    ) -> JsResult<()>;
    fn track_rejection(
        &mut self,
        promise: PromiseId,
        operation: PromiseRejectionOperation,
    ) -> JsResult<()>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PromiseRejectionOperation {
    #[default]
    Reject,
    Handle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromiseSettlementPlan {
    pub promise: PromiseId,
    pub from: PromiseState,
    pub to: PromiseState,
    pub payload: RuntimeValue,
    pub enqueue_reactions: bool,
    pub track_rejection: Option<PromiseRejectionOperation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PromiseSettlementError {
    AlreadySettled,
    MissingPromiseId,
}

impl JsPromise {
    pub fn plan_fulfill(
        &self,
        value: RuntimeValue,
    ) -> Result<PromiseSettlementPlan, PromiseSettlementError> {
        self.plan_settlement(PromiseState::Fulfilled, value)
    }

    pub fn plan_reject(
        &self,
        reason: RuntimeValue,
    ) -> Result<PromiseSettlementPlan, PromiseSettlementError> {
        let mut plan = self.plan_settlement(PromiseState::Rejected, reason)?;
        if !self.flags.is_handled {
            plan.track_rejection = Some(PromiseRejectionOperation::Reject);
        }
        Ok(plan)
    }

    pub fn plan_mark_handled(&self) -> Option<PromiseRejectionOperation> {
        (self.state == PromiseState::Rejected && !self.flags.is_handled)
            .then_some(PromiseRejectionOperation::Handle)
    }

    fn plan_settlement(
        &self,
        to: PromiseState,
        payload: RuntimeValue,
    ) -> Result<PromiseSettlementPlan, PromiseSettlementError> {
        let Some(promise) = self.promise else {
            return Err(PromiseSettlementError::MissingPromiseId);
        };
        if self.state != PromiseState::Pending {
            return Err(PromiseSettlementError::AlreadySettled);
        }
        Ok(PromiseSettlementPlan {
            promise,
            from: PromiseState::Pending,
            to,
            payload,
            enqueue_reactions: !matches!(self.reaction, PromiseReactionStorage::None),
            track_rejection: None,
        })
    }
}

pub fn promise_reaction_job_kind(reaction: &PromiseReaction) -> InternalMicrotaskKind {
    reaction
        .internal_microtask
        .unwrap_or(InternalMicrotaskKind::PromiseReactionJob)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    fn promise(slot: u32) -> PromiseId {
        PromiseId(ObjectId(CellId(slot)))
    }

    #[test]
    fn pending_promise_reject_plan_tracks_unhandled_rejection() {
        let promise = JsPromise {
            promise: Some(promise(1)),
            reaction: PromiseReactionStorage::List(PromiseReactionId(2)),
            ..JsPromise::default()
        };

        let plan = promise.plan_reject(RuntimeValue::from_i32(9)).unwrap();

        assert_eq!(plan.to, PromiseState::Rejected);
        assert!(plan.enqueue_reactions);
        assert_eq!(
            plan.track_rejection,
            Some(PromiseRejectionOperation::Reject)
        );
    }

    #[test]
    fn settled_promise_cannot_be_settled_again() {
        let promise = JsPromise {
            promise: Some(promise(1)),
            state: PromiseState::Fulfilled,
            ..JsPromise::default()
        };

        assert_eq!(
            promise.plan_fulfill(RuntimeValue::undefined()),
            Err(PromiseSettlementError::AlreadySettled)
        );
    }
}
