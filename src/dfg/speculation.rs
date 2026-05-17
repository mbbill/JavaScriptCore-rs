//! Speculation and abstract-value contracts for optimized tiers.
//!
//! This module records where speculation is assumed, where checks are expected,
//! and what recovery data an OSR exit would need. It does not evaluate types or
//! decide whether a speculation is profitable.

use crate::dfg::{DfgEdgeId, DfgNodeId, DfgValueRep};
use crate::object::StructureId;
use crate::runtime::{CodeBlockId, ObjectId};

/// Compact type lattice name used by graph descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculatedType {
    Unknown,
    Empty,
    Boolean,
    Int32,
    Int52,
    Double,
    Number,
    String,
    Symbol,
    BigInt,
    Cell,
    Object,
    Array,
    Function,
    FinalObject,
    RegExpObject,
    Other,
}

/// Where a prediction came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PredictionSource {
    ValueProfile,
    ArrayProfile,
    CallLinkInfo,
    BytecodeSemantic,
    AbstractInterpreter,
    Watchpoint,
    OsrExitProfile,
    FtlFeedback,
}

/// Direction of a speculation check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculationDirection {
    AssumeInput,
    ProveOutput,
    GuardHeapState,
    GuardControlFlow,
    GuardCallTarget,
}

/// Program point where speculation is attached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpeculationSite {
    pub owner: CodeBlockId,
    pub node: Option<DfgNodeId>,
    pub edge: Option<DfgEdgeId>,
    pub bytecode_index: Option<u32>,
}

/// Stable identity for a speculation check.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SpeculationCheckId(pub u32);

/// Check family. Later lowering decides the concrete machine sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculationCheckKind {
    Type,
    Structure,
    Cell,
    ArrayMode,
    Bounds,
    NonNull,
    Int32Overflow,
    DoubleToInt,
    CallTarget,
    Watchpoint,
    Unreachable,
}

/// Abstract value source carried in diagnostics and recovery metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbstractValueSource {
    Node(DfgNodeId),
    Edge(DfgEdgeId),
    VirtualRegister(crate::bytecode::VirtualRegister),
    ConstantPool(u32),
    HeapObject(ObjectId),
    Unknown,
}

/// A data-only speculation check descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpeculationCheck {
    pub id: SpeculationCheckId,
    pub site: SpeculationSite,
    pub kind: SpeculationCheckKind,
    pub direction: SpeculationDirection,
    pub predicted: SpeculatedType,
    pub representation: DfgValueRep,
    pub source: PredictionSource,
    pub structure: Option<StructureId>,
    pub recovery: Option<SpeculationRecovery>,
}

/// Additional recovery action needed when a speculation fails.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculationRecoveryKind {
    None,
    RestoreValue,
    ReboxDouble,
    ReconstructArguments,
    MaterializeObject,
    RewindArithAdd,
    RewindBooleanCheck,
}

/// Recovery descriptor. Register and stack layout remain symbolic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpeculationRecovery {
    pub kind: SpeculationRecoveryKind,
    pub value_source: AbstractValueSource,
    pub target_register: Option<crate::bytecode::VirtualRegister>,
    pub immediate: Option<i32>,
}
