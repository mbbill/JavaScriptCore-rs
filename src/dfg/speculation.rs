//! Speculation and abstract-value contracts for optimized tiers.
//!
//! This module records where speculation is assumed, where checks are expected,
//! and what recovery data an OSR exit would need. It does not evaluate types or
//! decide whether a speculation is profitable.

use crate::dfg::{DfgEdgeId, DfgNodeId, DfgValueRep, OsrExitKind};
use crate::gc::StructureId;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculationPreconditionKind {
    Type,
    Structure,
    Cell,
    ArrayMode,
    Bounds,
    NonNull,
    ArithmeticRange,
    NumericConversion,
    CallTarget,
    Watchpoint,
    ControlReachability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpeculationFailureSemantics {
    pub precondition: SpeculationPreconditionKind,
    pub exit_kind: OsrExitKind,
    pub records_exit_profile: bool,
    pub needs_recovery: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculationSemanticValidationError {
    StructureCheckMissingStructure(SpeculationCheckId),
    CallTargetDirectionMismatch(SpeculationCheckId),
    UnreachableDirectionMismatch(SpeculationCheckId),
    RecoveryWithoutFailure(SpeculationCheckId),
}

impl SpeculationCheck {
    pub const fn precondition_kind(&self) -> SpeculationPreconditionKind {
        match self.kind {
            SpeculationCheckKind::Type => SpeculationPreconditionKind::Type,
            SpeculationCheckKind::Structure => SpeculationPreconditionKind::Structure,
            SpeculationCheckKind::Cell => SpeculationPreconditionKind::Cell,
            SpeculationCheckKind::ArrayMode => SpeculationPreconditionKind::ArrayMode,
            SpeculationCheckKind::Bounds => SpeculationPreconditionKind::Bounds,
            SpeculationCheckKind::NonNull => SpeculationPreconditionKind::NonNull,
            SpeculationCheckKind::Int32Overflow => SpeculationPreconditionKind::ArithmeticRange,
            SpeculationCheckKind::DoubleToInt => SpeculationPreconditionKind::NumericConversion,
            SpeculationCheckKind::CallTarget => SpeculationPreconditionKind::CallTarget,
            SpeculationCheckKind::Watchpoint => SpeculationPreconditionKind::Watchpoint,
            SpeculationCheckKind::Unreachable => SpeculationPreconditionKind::ControlReachability,
        }
    }

    pub const fn failure_semantics(&self) -> SpeculationFailureSemantics {
        let exit_kind = match self.kind {
            SpeculationCheckKind::Type | SpeculationCheckKind::ArrayMode => OsrExitKind::BadType,
            SpeculationCheckKind::Structure | SpeculationCheckKind::CallTarget => {
                OsrExitKind::BadStructure
            }
            SpeculationCheckKind::Cell | SpeculationCheckKind::NonNull => OsrExitKind::BadCell,
            SpeculationCheckKind::Bounds => OsrExitKind::BoundsCheck,
            SpeculationCheckKind::Int32Overflow | SpeculationCheckKind::DoubleToInt => {
                OsrExitKind::Overflow
            }
            SpeculationCheckKind::Watchpoint => OsrExitKind::Watchpoint,
            SpeculationCheckKind::Unreachable => OsrExitKind::Unreachable,
        };

        SpeculationFailureSemantics {
            precondition: self.precondition_kind(),
            exit_kind,
            records_exit_profile: !matches!(exit_kind, OsrExitKind::Uncountable),
            needs_recovery: !matches!(self.kind, SpeculationCheckKind::Unreachable),
        }
    }

    pub fn validate_semantics(&self) -> Result<(), SpeculationSemanticValidationError> {
        if self.kind == SpeculationCheckKind::Structure && self.structure.is_none() {
            return Err(
                SpeculationSemanticValidationError::StructureCheckMissingStructure(self.id),
            );
        }
        if self.kind == SpeculationCheckKind::CallTarget
            && self.direction != SpeculationDirection::GuardCallTarget
        {
            return Err(SpeculationSemanticValidationError::CallTargetDirectionMismatch(self.id));
        }
        if self.kind == SpeculationCheckKind::Unreachable
            && self.direction != SpeculationDirection::GuardControlFlow
        {
            return Err(SpeculationSemanticValidationError::UnreachableDirectionMismatch(self.id));
        }
        if self.kind == SpeculationCheckKind::Unreachable && self.recovery.is_some() {
            return Err(SpeculationSemanticValidationError::RecoveryWithoutFailure(
                self.id,
            ));
        }

        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    fn site() -> SpeculationSite {
        SpeculationSite {
            owner: CodeBlockId(CellId(1)),
            node: None,
            edge: None,
            bytecode_index: Some(0),
        }
    }

    #[test]
    fn structure_precondition_requires_structure_identity() {
        let check = SpeculationCheck {
            id: SpeculationCheckId(3),
            site: site(),
            kind: SpeculationCheckKind::Structure,
            direction: SpeculationDirection::GuardHeapState,
            predicted: SpeculatedType::Object,
            representation: DfgValueRep::Object,
            source: PredictionSource::Watchpoint,
            structure: None,
            recovery: None,
        };

        assert_eq!(
            check.validate_semantics(),
            Err(
                SpeculationSemanticValidationError::StructureCheckMissingStructure(
                    SpeculationCheckId(3)
                )
            )
        );
    }

    #[test]
    fn overflow_precondition_maps_to_recoverable_osr_exit() {
        let check = SpeculationCheck {
            id: SpeculationCheckId(4),
            site: site(),
            kind: SpeculationCheckKind::Int32Overflow,
            direction: SpeculationDirection::ProveOutput,
            predicted: SpeculatedType::Int32,
            representation: DfgValueRep::Int32,
            source: PredictionSource::AbstractInterpreter,
            structure: None,
            recovery: None,
        };

        assert_eq!(check.validate_semantics(), Ok(()));
        assert_eq!(check.failure_semantics().exit_kind, OsrExitKind::Overflow);
        assert!(check.failure_semantics().needs_recovery);
    }
}
