use crate::runtime::exception::{ErrorType, ExceptionId, JsResult, LineColumn};
use crate::runtime::state::{ObjectId, RuntimeValue, SourceProviderId, StackFrameId, StringId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsErrorObject {
    /// Error object state after construction, before lazy stack materialization.
    pub object: Option<ObjectId>,
    pub error_type: ErrorType,
    pub message: Option<StringId>,
    pub cause: RuntimeValue,
    pub stack: ErrorStackState,
    pub source: ErrorSourceLocation,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ErrorStackState {
    pub frames: Vec<StackFrameId>,
    pub materialization: ErrorStackMaterialization,
    pub capture_owner: Option<ObjectId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ErrorStackMaterialization {
    #[default]
    Deferred,
    StackAccessorInstalled,
    StackStringMaterialized,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ErrorSourceLocation {
    pub provider: Option<SourceProviderId>,
    pub source_url: Option<StringId>,
    pub function_name: Option<StringId>,
    pub line_column: Option<LineColumn>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AggregateErrorObject {
    pub error: JsErrorObject,
    pub errors_array: Option<ObjectId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ErrorConstructionRequest {
    pub error_type: ErrorType,
    pub message: RuntimeValue,
    pub options: RuntimeValue,
    pub capture_stack: bool,
}

pub trait ErrorObjectOperations {
    fn construct_error(&mut self, request: ErrorConstructionRequest) -> JsResult<JsErrorObject>;
    fn construct_aggregate_error(
        &mut self,
        errors: RuntimeValue,
        request: ErrorConstructionRequest,
    ) -> JsResult<AggregateErrorObject>;
    fn materialize_error_stack(&mut self, error: ObjectId) -> JsResult<Option<StringId>>;
    fn error_from_exception(&mut self, exception: ExceptionId) -> JsResult<JsErrorObject>;
}
