//! Profiler contracts.
//!
//! Profilers observe execution, allocation, type flow, and control flow. They
//! must not own VM state; this module records sample and report shapes.

use crate::bytecode::BytecodeIndex;
use crate::runtime::{CodeBlockId, StackFrameId};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ProfilerRunId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilerKind {
    Sampling,
    Type,
    ControlFlow,
    Heap,
    Bytecode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilerSample {
    pub run: ProfilerRunId,
    pub frame: Option<StackFrameId>,
    pub code_block: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub kind: ProfilerKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfilerReport {
    pub samples: Vec<ProfilerSample>,
    pub dropped_sample_count: u64,
}
