use crate::runtime::interpreter::{BytecodeIndex, CallFrameId};
use crate::runtime::realm::RealmId;
use crate::runtime::state::{ObjectId, RuntimeValue, StackFrameId, StringId};

/// JavaScript exception object contract.
///
/// Exceptions are VM state, not Rust panics. The thrown value and captured stack
/// are GC-owned data reached through barriers or handles.
#[derive(Clone, Debug, Default)]
pub struct Exception {
    pub id: Option<ExceptionId>,
    pub thrown_value: RuntimeValue,
    pub stack: ExceptionStack,
    pub capture: StackCaptureAction,
    pub inspector_notified: bool,
    pub wasm_tag_wrapping: WasmTagWrappingState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ExceptionId(pub ObjectId);

#[derive(Clone, Debug, Default)]
pub struct ExceptionStack {
    pub frames: Vec<StackFrameId>,
    pub capture_owner: Option<ObjectId>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum StackCaptureAction {
    #[default]
    CaptureStack,
    DoNotCaptureStack,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum WasmTagWrappingState {
    #[default]
    NotApplicable,
    WrappedForJsTag,
    UnwrappedForJsTag,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PendingException {
    /// Pending exception marker stored on the VM.
    ///
    /// Rust callers use `Result` only to make propagation explicit; the thrown
    /// JS value, stack, and inspector notification state stay in VM-owned cells.
    pub exception_id: Option<ExceptionId>,
    pub is_termination: bool,
    pub state: PendingExceptionState,
    pub event_location: Option<ExceptionEventLocation>,
}

pub type JsResult<T> = Result<T, PendingException>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PendingExceptionState {
    #[default]
    Clear,
    PendingThrow,
    PendingTermination,
    BeingUnwound,
    Reported,
}

/// Scoped permission to throw and verify pending-exception discipline.
#[derive(Debug)]
pub struct ThrowScope<'vm> {
    pub state: ThrowScopeState,
    pub event_location: Option<ExceptionEventLocation>,
    _vm: std::marker::PhantomData<&'vm mut ()>,
}

impl<'vm> ThrowScope<'vm> {
    pub fn new_placeholder() -> Self {
        Self {
            state: ThrowScopeState::default(),
            event_location: None,
            _vm: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ThrowScopeState {
    #[default]
    Armed,
    Released,
    SimulatedThrow,
}

#[derive(Debug)]
pub struct ExceptionScope<'vm> {
    pub state: ExceptionScopeState,
    _vm: std::marker::PhantomData<&'vm mut ()>,
}

impl<'vm> ExceptionScope<'vm> {
    pub fn new_placeholder() -> Self {
        Self {
            state: ExceptionScopeState::default(),
            _vm: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ExceptionScopeState {
    #[default]
    Observing,
    Consumed,
    Propagating,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct HandlerInfo {
    pub start: u32,
    pub end: u32,
    pub target: u32,
    pub stack_depth: u32,
    pub scope_depth: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CatchInfo {
    pub handler: HandlerInfo,
    pub frame: Option<CallFrameId>,
    pub catch_pc: CatchPc,
    pub try_depth_for_throw: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CatchPc {
    Interpreter(BytecodeIndex),
    Native(u32),
}

impl Default for CatchPc {
    fn default() -> Self {
        Self::Interpreter(BytecodeIndex::default())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Unwinder {
    /// Stack-unwind metadata produced by handler lookup.
    ///
    /// This names frames and catch targets but does not pop Rust frames or run
    /// control flow. The eventual interpreter/JIT boundary will consume it.
    pub current_frame: Option<CallFrameId>,
    pub current_exception: Option<ExceptionId>,
    pub catch_info: Option<CatchInfo>,
}

impl Unwinder {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ErrorFactory {
    pub realm: Option<RealmId>,
    pub structures_ready: bool,
    pub source_appender: Option<SourceAppenderId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SourceAppenderId(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ErrorInstance {
    /// Deferred materialization state for JS `Error` objects.
    ///
    /// Message, cause, source location, and stack strings can be installed
    /// lazily; this contract records ownership without computing strings.
    pub object: Option<ObjectId>,
    pub error_type: ErrorType,
    pub message: Option<StringId>,
    pub cause: RuntimeValue,
    pub stack_trace: Vec<StackFrameId>,
    pub line_column: Option<LineColumn>,
    pub source_url: Option<StringId>,
    pub materialization: ErrorMaterializationState,
    pub flags: ErrorFlags,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ErrorType {
    #[default]
    Error,
    EvalError,
    RangeError,
    ReferenceError,
    SyntaxError,
    TypeError,
    UriError,
    AggregateError,
    SuppressedError,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct LineColumn {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ErrorMaterializationState {
    #[default]
    Deferred,
    StackPropertyMaterialized,
    FullyMaterialized,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ErrorFlags {
    pub stack_overflow: bool,
    pub out_of_memory: bool,
    pub native_getter_type_error: bool,
    pub parse_error: bool,
    pub catchable_from_wasm: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ExceptionEventLocation {
    pub stack_position: usize,
    pub function_name: Option<StringId>,
    pub file: Option<StringId>,
    pub line: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminationException {
    pub reason_code: u32,
    pub reason: TerminationReason,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TerminationReason {
    #[default]
    Watchdog,
    OutOfMemory,
    StackOverflow,
    HostRequested,
}
