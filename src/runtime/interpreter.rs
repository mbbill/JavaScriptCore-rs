use crate::runtime::function::{CallData, CodeSpecializationKind, ConstructData};
use crate::runtime::realm::RealmId;
use crate::runtime::scope::ScopeId;
use crate::runtime::state::{CodeBlockId, ObjectId, RuntimeValue, StackFrameId};

/// Dispatch and VM execution services.
///
/// This skeleton defines frame and register ownership boundaries only. It does
/// not dispatch bytecode or create a reduced execution path.
#[derive(Clone, Debug, Default)]
pub struct Interpreter {
    pub dispatch_generation: u64,
    pub opcode_table_generation: u64,
    pub boundary: InterpreterBoundary,
}

/// Boundary exposed to the VM once bytecode has already been prepared.
///
/// The interpreter is a consumer of code blocks and frame records. Parsing,
/// linking, JIT tier selection, and host callback execution remain outside this
/// module until their ownership contracts are explicit.
#[derive(Clone, Debug, Default)]
pub struct InterpreterBoundary {
    pub accepts_program_code_blocks: bool,
    pub accepts_function_code_blocks: bool,
    pub accepts_eval_code_blocks: bool,
    pub accepts_module_code_blocks: bool,
}

/// VM entry guard contract.
///
/// A real guard borrows the VM, installs entry/top-frame state, and restores it
/// on drop. Raw frame pointers and ABI records are unsafe boundaries.
#[derive(Debug)]
pub struct VmEntryScope<'vm> {
    pub entry_state: VmEntryState,
    _vm: std::marker::PhantomData<&'vm mut ()>,
}

impl<'vm> VmEntryScope<'vm> {
    pub fn new_placeholder() -> Self {
        Self {
            entry_state: VmEntryState::default(),
            _vm: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VmEntryState {
    pub entry_frame: Option<EntryFrameId>,
    pub top_call_frame: Option<CallFrameId>,
    pub lexical_realm: Option<RealmId>,
    pub reason: VmEntryReason,
    pub host_reentry_depth: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum VmEntryReason {
    #[default]
    Api,
    Microtask,
    ModuleEvaluation,
    Debugger,
    HostCallback,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct EntryFrameId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CallFrameId(pub u32);

#[derive(Clone, Debug, Default)]
pub struct EntryFrame {
    /// VM-entry frame state belongs to the entry guard, not to a JS call frame.
    ///
    /// It records enough restoration metadata for nested host reentry without
    /// committing to the platform ABI layout used by C++ `EntryFrame`.
    pub id: Option<EntryFrameId>,
    pub previous_entry_frame: Option<EntryFrameId>,
    pub saved_top_call_frame: Option<CallFrameId>,
    pub callee_saves_record: Option<CalleeSavesRecordId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CalleeSavesRecordId(pub u32);

#[derive(Clone, Debug, Default)]
pub struct ProtoCallFrame {
    /// Stack-only frame builder used before a full `CallFrame` is installed.
    ///
    /// It mirrors JavaScriptCore's proto frame role: hold callee, code block,
    /// `this`, context, and padded argument metadata at the VM/interpreter ABI.
    pub code_block: Option<CodeBlockId>,
    pub callee: Option<ObjectId>,
    pub argument_count: u32,
    pub argument_count_including_this: u32,
    pub padded_argument_count: u32,
    pub this_value: RuntimeValue,
    pub context: Option<ObjectId>,
    pub lexical_realm: Option<RealmId>,
}

#[derive(Clone, Debug, Default)]
pub struct CallFrameLayout {
    pub caller_frame_slot: FrameSlot,
    pub return_pc_slot: FrameSlot,
    pub code_block_slot: FrameSlot,
    pub callee_slot: FrameSlot,
    pub argument_count_slot: FrameSlot,
    pub this_argument_slot: FrameSlot,
    pub first_argument_slot: FrameSlot,
    pub first_local_slot: FrameSlot,
    pub header_slot_count: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct FrameSlot(pub i32);

#[derive(Clone, Debug, Default)]
pub struct CallFrameHeader {
    /// Fixed header slots visible to stack walking, exception unwinding, and
    /// debugger inspection. Slot offsets live in `CallFrameLayout`.
    pub caller: CallerFrame,
    pub return_pc: ReturnAddress,
    pub code_block: Option<CodeBlockId>,
    pub callee: Option<ObjectId>,
    pub argument_count_including_this: u32,
    pub call_site: CallSite,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CallerFrame {
    #[default]
    None,
    Entry(EntryFrameId),
    Call(CallFrameId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ReturnAddress(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CallSite {
    #[default]
    Unknown,
    BytecodeOffset(BytecodeIndex),
    CodeOrigin(CodeOriginIndex),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct BytecodeIndex(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeOriginIndex(pub u32);

#[derive(Clone, Debug, Default)]
pub struct FrameArguments {
    pub this_value_slot: FrameSlot,
    pub first_argument_slot: FrameSlot,
    pub argument_count: u32,
    pub capture_state: ArgumentsCaptureState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArgumentsCaptureState {
    #[default]
    FrameResident,
    MaterializedObject(ObjectId),
    Forwarded,
}

#[derive(Clone, Debug, Default)]
pub struct FrameLocals {
    pub first_local_slot: FrameSlot,
    pub local_count: u32,
    pub scope_register: Option<FrameSlot>,
}

#[derive(Debug)]
pub struct CallFrame<'vm> {
    /// Borrowed view of an installed frame.
    ///
    /// The lifetime models exclusive VM stack access while a frame is being
    /// inspected or prepared. The actual register storage remains owned by the
    /// VM stack segment.
    pub id: Option<CallFrameId>,
    pub layout: CallFrameLayout,
    pub header: CallFrameHeader,
    pub arguments: FrameArguments,
    pub locals: FrameLocals,
    pub lexical_scope: Option<ScopeId>,
    pub lexical_realm: Option<RealmId>,
    _vm: std::marker::PhantomData<&'vm mut ()>,
}

impl<'vm> CallFrame<'vm> {
    pub fn new_placeholder(layout: CallFrameLayout) -> Self {
        Self {
            id: None,
            layout,
            header: CallFrameHeader::default(),
            arguments: FrameArguments::default(),
            locals: FrameLocals::default(),
            lexical_scope: None,
            lexical_realm: None,
            _vm: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FrameCursor {
    pub current_frame_id: Option<CallFrameId>,
    pub entry_frame_id: Option<EntryFrameId>,
    pub include_native_frames: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Value(RuntimeValue),
    FramePointer(CallFrameId),
    EntryFramePointer(EntryFrameId),
    CodePointer(CodeBlockId),
    ScopePointer(ScopeId),
}

#[derive(Clone, Debug, Default)]
pub struct RegisterFile {
    pub register_count: usize,
    pub frame_capacity: usize,
    pub argument_capacity: usize,
    pub active_segment: Option<StackSegmentId>,
}

#[derive(Clone, Debug, Default)]
pub struct StackSegment {
    pub id: Option<StackSegmentId>,
    pub capacity_in_registers: usize,
    pub committed_registers: usize,
    pub owner_entry_frame: Option<EntryFrameId>,
    pub top_call_frame: Option<CallFrameId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct StackSegmentId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct DispatchPc(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InterpreterRequest {
    /// Requests are already linked to code blocks or call metadata.
    ///
    /// This is intentionally not an evaluator API. Future execution code can
    /// consume these variants after parser/linker/JIT layers have done their
    /// work and after exception discipline has installed a throw scope.
    Program(ProgramEntry),
    Module(ModuleEntry),
    Call(CallEntry),
    Construct(ConstructEntry),
    Eval(EvalEntry),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramEntry {
    pub code_block: CodeBlockId,
    pub global_object: ObjectId,
    pub this_object: ObjectId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleEntry {
    pub code_block: CodeBlockId,
    pub module_environment: ObjectId,
    pub sent_value: RuntimeValue,
    pub resume_mode: RuntimeValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallEntry {
    pub function: ObjectId,
    pub call_data: CallData,
    pub this_value: RuntimeValue,
    pub context: Option<ObjectId>,
    pub argument_count: u32,
    pub specialization: CodeSpecializationKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstructEntry {
    pub function: ObjectId,
    pub construct_data: ConstructData,
    pub new_target: RuntimeValue,
    pub argument_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvalEntry {
    pub code_block: CodeBlockId,
    pub this_value: RuntimeValue,
    pub scope: ScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionResult<T> {
    Returned(T),
    Threw(crate::runtime::exception::PendingException),
    Terminated(crate::runtime::exception::TerminationException),
    Suspended(SuspensionRecord),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuspensionRecord {
    pub frame: Option<CallFrameId>,
    pub stack_frame: Option<StackFrameId>,
    pub resume_mode: RuntimeValue,
}
