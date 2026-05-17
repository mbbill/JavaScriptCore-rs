use crate::runtime::exception::JsResult;
use crate::runtime::realm::RealmId;
use crate::runtime::state::{HostHookId, ObjectId, RuntimeValue};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct MicrotaskIdentifier(pub u64);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueuedTask {
    /// A queued task stores a dispatcher/global edge, an internal job kind, a
    /// small payload byte, and up to three rooted arguments.
    pub id: Option<MicrotaskIdentifier>,
    pub dispatcher: MicrotaskDispatcherRef,
    pub job: InternalMicrotaskKind,
    pub payload: u8,
    pub arguments: [RuntimeValue; 3],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MicrotaskDispatcherRef {
    #[default]
    None,
    Global(ObjectId),
    Dispatcher(ObjectId),
    Host(HostHookId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InternalMicrotaskKind {
    #[default]
    None,
    PromiseResolveThenableJobFast,
    PromiseResolveThenableJobWithInternalMicrotaskFast,
    PromiseResolveThenableJob,
    PromiseResolveThenableJobWithInternalMicrotask,
    PromiseResolveWithoutHandlerJob,
    PromiseFulfillWithoutHandlerJob,
    PromiseRaceResolveJob,
    PromiseAllResolveJob,
    PromiseAllSettledResolveJob,
    PromiseAnyResolveJob,
    PromiseFinallyReactionJob,
    PromiseFinallyAwaitJob,
    PromiseReactionJob,
    AsyncFunctionResume,
    AsyncFromSyncIteratorContinue,
    AsyncFromSyncIteratorDone,
    AsyncGeneratorYieldAwaited,
    AsyncGeneratorBodyCallNormal,
    AsyncGeneratorBodyCallReturn,
    AsyncGeneratorResumeNext,
    InvokeFunctionJob,
    AsyncModuleExecutionResume,
    AsyncModuleExecutionDone,
    ModuleRegistryFetchSettled,
    ModuleRegistryModuleSettled,
    ModuleGraphLoadingError,
    ModuleLoadStep,
    DynamicImportLoadSettled,
    DynamicImportEvaluateSettled,
    ImportModuleNamespace,
    Opaque,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum QueuedTaskResult {
    #[default]
    Executed,
    Discard,
    Suspended,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MicrotaskQueue {
    pub owner_realm: Option<RealmId>,
    pub queued_count: usize,
    pub kept_alive_count: usize,
    pub is_scheduled_to_run: bool,
    pub is_performing_checkpoint: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MicrotaskCheckpoint {
    pub queue: MicrotaskQueue,
    pub current_global_object: Option<ObjectId>,
    pub allow_global_object_switch: bool,
    pub top_exception_scope_installed: bool,
}

/// Host and VM boundary for job enqueueing and microtask checkpoints.
pub trait MicrotaskQueueOperations {
    fn enqueue_microtask(&mut self, task: QueuedTask) -> JsResult<MicrotaskIdentifier>;
    fn schedule_to_run_if_needed(&mut self, queue: MicrotaskQueue) -> JsResult<()>;
    fn perform_microtask_checkpoint(
        &mut self,
        checkpoint: MicrotaskCheckpoint,
    ) -> JsResult<QueuedTaskResult>;
    fn run_internal_microtask(&mut self, task: QueuedTask) -> JsResult<QueuedTaskResult>;
}
