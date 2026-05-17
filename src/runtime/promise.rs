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
    pub kind: PromiseReactionKind,
    pub capability: Option<PromiseCapability>,
    pub handler: RuntimeValue,
    pub next: Option<PromiseReactionId>,
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
