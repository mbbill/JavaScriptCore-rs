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
    ModuleLoadTopSettled,
    ModuleLoadTopRejected,
    ModuleLoadSpecifierTransform,
    ModuleLoadCombinedLoadSettled,
    ModuleLoadCombinedStateSettled,
    ModuleLoadLinkEvaluateSettled,
    ModuleLoadReturnRecord,
    ModuleLoadReturnModuleKey,
    ModuleLoadStoreError,
    DynamicImportLoadSettled,
    DynamicImportEvaluateSettled,
    ImportModuleNamespace,
    WebAssemblyCompileStreaming,
    WebAssemblyInstantiateStreaming,
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
    /// Queue ownership is ref-counted in C++ and detached from GC cell storage.
    ///
    /// The Rust runtime must keep the same authority boundary: queue mutation is
    /// controlled by the VM/microtask dispatcher, while queued JS values are
    /// traced through the aggregate visitor for the queue.
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

/// Dispatcher family encoded in a queued task's compact dispatcher field.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MicrotaskDispatcherKind {
    #[default]
    None,
    GlobalObject,
    EngineDebuggable,
    HostWebCore,
    OpaqueHost,
}

/// Marking state for queue entries kept alive across a GC visit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MicrotaskMarkingState {
    #[default]
    NotMarking,
    MarkingQueuedTasks,
    MarkingKeptAliveTasks,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MicrotaskQueuePlanError {
    CheckpointAlreadyRunning,
    QueueNotScheduled,
    NoQueuedTasks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum JobQueueRunStepError {
    CheckpointNotRunning,
    NoQueuedTasks,
    TaskNotRunnable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MicrotaskCheckpointPlan {
    pub queued_to_run: usize,
    pub kept_alive: usize,
    pub dispatcher_kind: MicrotaskDispatcherKind,
    pub allow_global_object_switch: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobQueueRunStepRecord {
    pub task: QueuedTask,
    pub queue_before: MicrotaskQueue,
    pub dispatcher_kind: MicrotaskDispatcherKind,
    pub action: JobQueueRunStepAction,
}

impl JobQueueRunStepRecord {
    pub fn queue_after_result(&self, result: QueuedTaskResult) -> MicrotaskQueue {
        let mut queue = self.queue_before.clone();
        match result {
            QueuedTaskResult::Executed | QueuedTaskResult::Discard => {
                queue.queued_count = queue.queued_count.saturating_sub(1);
            }
            QueuedTaskResult::Suspended => {
                queue.is_scheduled_to_run = true;
            }
        }
        queue.is_scheduled_to_run = queue.queued_count > 0 || result == QueuedTaskResult::Suspended;
        queue
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum JobQueueRunStepAction {
    RunInternalMicrotask(InternalMicrotaskKind),
    RunHostMicrotask,
}

pub fn plan_job_queue_run_step(
    queue: &MicrotaskQueue,
    task: QueuedTask,
) -> Result<JobQueueRunStepRecord, JobQueueRunStepError> {
    if !queue.is_performing_checkpoint {
        return Err(JobQueueRunStepError::CheckpointNotRunning);
    }
    if queue.queued_count == 0 {
        return Err(JobQueueRunStepError::NoQueuedTasks);
    }
    let dispatcher_kind = task.dispatcher_kind();
    if dispatcher_kind == MicrotaskDispatcherKind::None && task.job == InternalMicrotaskKind::None {
        return Err(JobQueueRunStepError::TaskNotRunnable);
    }
    let action = if task.job == InternalMicrotaskKind::Opaque {
        JobQueueRunStepAction::RunHostMicrotask
    } else {
        JobQueueRunStepAction::RunInternalMicrotask(task.job)
    };

    Ok(JobQueueRunStepRecord {
        task,
        queue_before: queue.clone(),
        dispatcher_kind,
        action,
    })
}

impl MicrotaskQueue {
    pub fn plan_enqueue(&self) -> MicrotaskQueue {
        let mut next = self.clone();
        next.queued_count = next.queued_count.saturating_add(1);
        next.is_scheduled_to_run = true;
        next
    }

    pub fn plan_checkpoint(
        &self,
        dispatcher_kind: MicrotaskDispatcherKind,
        allow_global_object_switch: bool,
    ) -> Result<MicrotaskCheckpointPlan, MicrotaskQueuePlanError> {
        if self.is_performing_checkpoint {
            return Err(MicrotaskQueuePlanError::CheckpointAlreadyRunning);
        }
        if !self.is_scheduled_to_run {
            return Err(MicrotaskQueuePlanError::QueueNotScheduled);
        }
        if self.queued_count == 0 {
            return Err(MicrotaskQueuePlanError::NoQueuedTasks);
        }
        Ok(MicrotaskCheckpointPlan {
            queued_to_run: self.queued_count,
            kept_alive: self.kept_alive_count,
            dispatcher_kind,
            allow_global_object_switch,
        })
    }
}

impl QueuedTask {
    pub fn dispatcher_kind(&self) -> MicrotaskDispatcherKind {
        match self.dispatcher {
            MicrotaskDispatcherRef::None => MicrotaskDispatcherKind::None,
            MicrotaskDispatcherRef::Global(_) => MicrotaskDispatcherKind::GlobalObject,
            MicrotaskDispatcherRef::Dispatcher(_) => MicrotaskDispatcherKind::EngineDebuggable,
            MicrotaskDispatcherRef::Host(_) => MicrotaskDispatcherKind::HostWebCore,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_plan_marks_queue_scheduled() {
        let queue = MicrotaskQueue::default();

        let next = queue.plan_enqueue();

        assert_eq!(next.queued_count, 1);
        assert!(next.is_scheduled_to_run);
    }

    #[test]
    fn checkpoint_plan_rejects_reentrant_drain() {
        let queue = MicrotaskQueue {
            queued_count: 1,
            is_scheduled_to_run: true,
            is_performing_checkpoint: true,
            ..MicrotaskQueue::default()
        };

        assert_eq!(
            queue.plan_checkpoint(MicrotaskDispatcherKind::GlobalObject, false),
            Err(MicrotaskQueuePlanError::CheckpointAlreadyRunning)
        );
    }

    #[test]
    fn job_queue_run_step_records_internal_task_without_running_it() {
        let queue = MicrotaskQueue {
            queued_count: 2,
            is_scheduled_to_run: true,
            is_performing_checkpoint: true,
            ..MicrotaskQueue::default()
        };
        let task = QueuedTask {
            job: InternalMicrotaskKind::PromiseReactionJob,
            dispatcher: MicrotaskDispatcherRef::Global(ObjectId(crate::gc::CellId(1))),
            ..QueuedTask::default()
        };

        let step = plan_job_queue_run_step(&queue, task).unwrap();

        assert_eq!(
            step.action,
            JobQueueRunStepAction::RunInternalMicrotask(InternalMicrotaskKind::PromiseReactionJob)
        );
        assert_eq!(
            step.queue_after_result(QueuedTaskResult::Executed)
                .queued_count,
            1
        );
    }
}
