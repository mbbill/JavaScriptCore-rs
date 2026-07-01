//! Per-variable data shared by all DFG local-access nodes of one variable.
//!
//! Faithful port of C++ `dfg/DFGVariableAccessData.h`. Any two nodes in the
//! same Phi graph share one `VariableAccessData` and therefore share
//! predictions (DFGNodeType.h:60-63 commentary). This slice ports the payload
//! the bytecode parser needs: `m_prediction`, `m_operand`, `m_flags`
//! (DFGVariableAccessData.h:214-218). JSC's remaining fields —
//! `m_argumentAwarePrediction`, `m_machineLocal`, the unboxing/double-format
//! vote state, and the `UnionFind` base used for variable unification — belong
//! to the prediction-propagation and fixup phases and land with those ports.

use crate::bytecode::speculated_type::{merge_speculation, SpeculatedType, SPEC_NONE};
use crate::bytecode::VirtualRegister;
use crate::dfg::node_flags::NodeFlags;

/// One variable's shared access payload.
///
/// C++ hands out stable `VariableAccessData*` into a graph-owned
/// `SegmentedVector` arena (dfg/DFGGraph.h:1413); safe Rust replaces the
/// pointer with `DfgVariableAccessDataId` indices into the graph-owned vector,
/// which is append-only during parsing so indices stay stable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VariableAccessData {
    /// `SpeculatedType m_prediction` (DFGVariableAccessData.h:214).
    prediction: SpeculatedType,
    /// `Operand m_operand` (DFGVariableAccessData.h:216). The Rust bytecode
    /// port's operand namespace is `VirtualRegister`.
    operand: VirtualRegister,
    /// `NodeFlags m_flags` (DFGVariableAccessData.h:218).
    flags: NodeFlags,
}

impl VariableAccessData {
    /// `VariableAccessData(Operand)`: prediction starts at `SpecNone`, flags at
    /// zero (DFGVariableAccessData.cpp:47-59).
    pub const fn new(operand: VirtualRegister) -> Self {
        Self {
            prediction: SPEC_NONE,
            operand,
            flags: 0,
        }
    }

    /// `operand()` (DFGVariableAccessData.h:56-60).
    pub const fn operand(&self) -> VirtualRegister {
        self.operand
    }

    /// `prediction()` (DFGVariableAccessData.h:138-141). Without the deferred
    /// UnionFind unification this reads the local prediction directly
    /// (`nonUnifiedPrediction()`, h:133-136, is the same value until variables
    /// are unified).
    pub const fn prediction(&self) -> SpeculatedType {
        self.prediction
    }

    /// `predict(SpeculatedType)` (DFGVariableAccessData.h:131): merges the new
    /// speculation into the prediction, returning whether it changed. C++ also
    /// merges into `m_argumentAwarePrediction` (DFGVariableAccessData.cpp:75-76);
    /// that field is deferred with the rest of the prediction-propagation state
    /// (module doc) — when it lands, this fn must gain that merge in lockstep.
    pub fn predict(&mut self, prediction: SpeculatedType) -> bool {
        merge_speculation(&mut self.prediction, prediction)
    }

    /// `flags()` (DFGVariableAccessData.h:190).
    pub const fn flags(&self) -> NodeFlags {
        self.flags
    }

    /// `mergeFlags(NodeFlags)` (DFGVariableAccessData.h:192-195): bitwise-or
    /// merge, returning whether the flags changed.
    pub fn merge_flags(&mut self, new_flags: NodeFlags) -> bool {
        let merged = self.flags | new_flags;
        let changed = merged != self.flags;
        self.flags = merged;
        changed
    }
}
