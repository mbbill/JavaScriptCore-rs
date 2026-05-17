use crate::runtime::exception::JsResult;
use crate::runtime::state::{ObjectId, RuntimeValue};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IteratorRecord {
    pub iterator: ObjectId,
    pub next_method: ObjectId,
    pub done: bool,
    pub kind: IteratorKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum IteratorKind {
    #[default]
    Sync,
    Async,
    Array,
    Map,
    Set,
    String,
    RegExpString,
    IteratorHelper,
    AsyncFromSync,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IteratorResult {
    pub value: RuntimeValue,
    pub done: bool,
    pub result_object: Option<ObjectId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GeneratorObject {
    pub object: Option<ObjectId>,
    pub state: GeneratorState,
    pub resume_mode: GeneratorResumeMode,
    pub next: RuntimeValue,
    pub this_value: RuntimeValue,
    pub frame: RuntimeValue,
    pub context: RuntimeValue,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum GeneratorState {
    Completed,
    Executing,
    #[default]
    Init,
    Suspended(u32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum GeneratorResumeMode {
    #[default]
    Normal,
    Return,
    Throw,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AsyncGeneratorObject {
    pub object: Option<ObjectId>,
    pub state: AsyncGeneratorState,
    pub queue: Option<AsyncGeneratorQueueId>,
    pub resume_value: RuntimeValue,
    pub resume_mode: AsyncGeneratorResumeMode,
    pub resume_promise: RuntimeValue,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct AsyncGeneratorQueueId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum AsyncGeneratorState {
    Completed,
    Executing,
    #[default]
    Init,
    AwaitingReturn,
    Suspended {
        reason: AsyncGeneratorSuspendReason,
        offset: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum AsyncGeneratorSuspendReason {
    #[default]
    Await,
    Yield,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum AsyncGeneratorResumeMode {
    Empty,
    #[default]
    Normal,
    Return,
    Throw,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AsyncGeneratorRequest {
    pub value: RuntimeValue,
    pub resume_mode: AsyncGeneratorResumeMode,
    pub promise: ObjectId,
}

pub trait IteratorOperations {
    fn get_iterator(&mut self, value: RuntimeValue, kind: IteratorKind)
        -> JsResult<IteratorRecord>;
    fn iterator_next(
        &mut self,
        record: IteratorRecord,
        value: Option<RuntimeValue>,
    ) -> JsResult<IteratorResult>;
    fn iterator_close(
        &mut self,
        record: IteratorRecord,
        completion: RuntimeValue,
    ) -> JsResult<RuntimeValue>;
    fn generator_resume(
        &mut self,
        generator: ObjectId,
        value: RuntimeValue,
        mode: GeneratorResumeMode,
    ) -> JsResult<IteratorResult>;
    fn async_generator_enqueue(
        &mut self,
        generator: ObjectId,
        request: AsyncGeneratorRequest,
    ) -> JsResult<()>;
    fn async_generator_resume_next(&mut self, generator: ObjectId) -> JsResult<()>;
}
