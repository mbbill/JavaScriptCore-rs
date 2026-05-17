use crate::bytecode::code_block::{
    BytecodeIndex, CallSiteIndex, SourcePosition, SourceProviderId, SourceRange,
};

/// Opaque handle to an inline-call-frame descriptor owned by a future JIT tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct InlineCallFrameRef(pub u32);

/// Opaque handle to a linked code block for source-origin tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct CodeBlockRef(pub u32);

/// Bytecode position plus optional inline stack context.
///
/// JSC packs `CodeOrigin` aggressively for generated code. The Rust skeleton
/// keeps the semantic shape visible while leaving pointer packing and lifetime
/// ownership to the JIT/runtime layers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeOrigin {
    pub bytecode_index: BytecodeIndex,
    pub inline_call_frame: Option<InlineCallFrameRef>,
}

impl CodeOrigin {
    pub const fn new(bytecode_index: BytecodeIndex) -> Self {
        Self {
            bytecode_index,
            inline_call_frame: None,
        }
    }

    pub const fn with_inline_call_frame(
        bytecode_index: BytecodeIndex,
        inline_call_frame: InlineCallFrameRef,
    ) -> Self {
        Self {
            bytecode_index,
            inline_call_frame: Some(inline_call_frame),
        }
    }

    pub const fn is_set(self) -> bool {
        self.bytecode_index.is_valid()
    }
}

/// Code origin resolved against the owning code block.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FullCodeOrigin {
    pub code_block: CodeBlockRef,
    pub origin: CodeOrigin,
}

/// One inlining edge in a generated-code inline stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCallFrameRecord {
    pub reference: InlineCallFrameRef,
    pub caller: CodeOrigin,
    pub callee: CodeBlockRef,
    pub call_site: Option<CallSiteIndex>,
    pub source_range: Option<SourceRange>,
}

/// Side table used by debugger, profiler, stack traces, and deoptimization.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodeOriginTable {
    pub inline_call_frames: Vec<InlineCallFrameRecord>,
    pub pc_mappings: Vec<ProgramCounterOrigin>,
    pub source_mappings: Vec<BytecodeSourceMapping>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProgramCounterOrigin {
    pub pc_offset: u32,
    pub origin: CodeOrigin,
    pub width: ProgramCounterMappingWidth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProgramCounterMappingWidth {
    Byte,
    NativeInstruction,
    ReturnPoint,
    SlowPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BytecodeSourceMapping {
    pub bytecode_index: BytecodeIndex,
    pub provider: Option<SourceProviderId>,
    pub source_range: SourceRange,
    pub position_kind: SourcePositionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourcePositionKind {
    Expression,
    Statement,
    Call,
    Construct,
    Return,
    Throw,
    Synthetic,
}

/// Result shape for source-note lookups without committing to a lookup engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceNoteLookup {
    pub bytecode_index: BytecodeIndex,
    pub position: SourcePosition,
    pub range: SourceRange,
}
