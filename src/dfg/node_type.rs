//! DFG node opcodes (`NodeType`) and their per-opcode default flags.
//!
//! Faithful port of C++ `dfg/DFGNodeType.h`: one table (`for_each_dfg_op!`,
//! mirroring the `FOR_EACH_DFG_OP` X-macro at DFGNodeType.h:35) drives both the
//! `NodeType` enum (`enum NodeType : uint16_t`, DFGNodeType.h:682-687) and
//! `default_flags()` (`defaultFlags(NodeType)`, DFGNodeType.h:696-706), so an
//! opcode and its default `NodeFlags` can never drift apart.
//!
//! This is the first-parser slice of JSC's ~480 node types, kept in
//! DFGNodeType.h declaration order. New opcodes are added by extending the
//! table with the exact JSC name and flags line; per-node operand/payload data
//! lives in the node struct (as in `DFG::Node`), never in this enum.

use crate::dfg::node_flags::{
    NodeFlags, NODE_HAS_VAR_ARGS, NODE_MUST_GENERATE, NODE_RESULT_BOOLEAN, NODE_RESULT_DOUBLE,
    NODE_RESULT_JS, NODE_RESULT_NUMBER,
};

// Mirrors FOR_EACH_DFG_OP (dfg/DFGNodeType.h:35). Each row is
// `(JSC opcode name, JSC default flags)`; the cited line is the C++ macro row.
macro_rules! for_each_dfg_op {
    ($m:ident) => {
        $m! {
            // A constant in the CodeBlock's constant pool (DFGNodeType.h:37).
            (JSConstant, NODE_RESULT_JS),
            // Constant with a specific representation (DFGNodeType.h:40).
            (DoubleConstant, NODE_RESULT_DOUBLE),
            // Local variable access; MustGenerate because it is the only
            // evidence that another block read the local (DFGNodeType.h:74).
            (GetLocal, NODE_RESULT_JS | NODE_MUST_GENERATE),
            // (DFGNodeType.h:75)
            (SetLocal, 0),
            // (DFGNodeType.h:84)
            (MovHint, NODE_MUST_GENERATE),
            // Exit state is intact; safe to exit to the exit origin
            // (DFGNodeType.h:86).
            (ExitOK, NODE_MUST_GENERATE),
            // (DFGNodeType.h:87)
            (Phantom, NODE_MUST_GENERATE),
            // Type check without liveness (DFGNodeType.h:88).
            (Check, NODE_MUST_GENERATE),
            // (DFGNodeType.h:90)
            (Upsilon, 0),
            // (DFGNodeType.h:91)
            (Phi, 0),
            // (DFGNodeType.h:92)
            (Flush, NODE_MUST_GENERATE),
            // Bytecode's preferred OSR point; must survive DCE
            // (DFGNodeType.h:100).
            (LoopHint, NODE_MUST_GENERATE),
            // Argument set at the function prologue (DFGNodeType.h:114).
            (SetArgumentDefinitely, 0),
            // (DFGNodeType.h:163)
            (ArithAdd, NODE_RESULT_NUMBER | NODE_MUST_GENERATE),
            // (DFGNodeType.h:165)
            (ArithSub, NODE_RESULT_NUMBER | NODE_MUST_GENERATE),
            // (DFGNodeType.h:167)
            (ArithMul, NODE_RESULT_NUMBER | NODE_MUST_GENERATE),
            // Arithmetic or string concatenation (DFGNodeType.h:192).
            (ValueAdd, NODE_RESULT_JS | NODE_MUST_GENERATE),
            // (DFGNodeType.h:194)
            (ValueSub, NODE_RESULT_JS | NODE_MUST_GENERATE),
            // (DFGNodeType.h:195)
            (ValueMul, NODE_RESULT_JS | NODE_MUST_GENERATE),
            // (DFGNodeType.h:232)
            (GetById, NODE_RESULT_JS | NODE_MUST_GENERATE),
            // (DFGNodeType.h:239)
            (PutById, NODE_MUST_GENERATE),
            // (DFGNodeType.h:291)
            (GetScope, NODE_RESULT_JS),
            // (DFGNodeType.h:388)
            (CompareLess, NODE_RESULT_BOOLEAN | NODE_MUST_GENERATE),
            // (DFGNodeType.h:400)
            (Call, NODE_RESULT_JS | NODE_MUST_GENERATE | NODE_HAS_VAR_ARGS),
            // Block terminals (DFGNodeType.h:562-566).
            (Jump, NODE_MUST_GENERATE),
            (Branch, NODE_MUST_GENERATE),
            (Return, NODE_MUST_GENERATE),
            // (DFGNodeType.h:571)
            (Unreachable, NODE_MUST_GENERATE),
            // (DFGNodeType.h:572)
            (Throw, NODE_MUST_GENERATE),
            // Pseudo-terminal: execution falls out of the DFG here
            // (DFGNodeType.h:584).
            (ForceOSRExit, NODE_MUST_GENERATE),
            // VM traps check (DFGNodeType.h:592).
            (CheckTraps, NODE_MUST_GENERATE),
        }
    };
}

macro_rules! define_node_type {
    ($(($op:ident, $flags:expr)),* $(,)?) => {
        /// DFG opcode. Mirrors `enum NodeType : uint16_t`
        /// (dfg/DFGNodeType.h:682-687); exact JSC names.
        ///
        /// CAUTION: the numeric discriminants are declaration order over this
        /// 31-op SUBSET, not JSC's build-generated NodeType ids — never
        /// serialize or compare the raw values across engines.
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        #[repr(u16)]
        pub enum NodeType {
            $($op),*
        }

        /// Default `NodeFlags` for each opcode. Mirrors `defaultFlags(NodeType)`
        /// (dfg/DFGNodeType.h:696-706), driven by the same table as the enum.
        pub const fn default_flags(op: NodeType) -> NodeFlags {
            match op {
                $(NodeType::$op => $flags),*
            }
        }
    };
}

for_each_dfg_op!(define_node_type);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dfg::node_flags::NODE_RESULT_MASK;

    // Spot checks against the C++ FOR_EACH_DFG_OP table rows cited above.
    #[test]
    fn default_flags_match_dfg_node_type_table() {
        // DFGNodeType.h:163
        assert_eq!(
            default_flags(NodeType::ArithAdd),
            NODE_RESULT_NUMBER | NODE_MUST_GENERATE
        );
        // DFGNodeType.h:192
        assert_eq!(
            default_flags(NodeType::ValueAdd),
            NODE_RESULT_JS | NODE_MUST_GENERATE
        );
        // DFGNodeType.h:40
        assert_eq!(default_flags(NodeType::DoubleConstant), NODE_RESULT_DOUBLE);
        // DFGNodeType.h:74-75
        assert_eq!(
            default_flags(NodeType::GetLocal),
            NODE_RESULT_JS | NODE_MUST_GENERATE
        );
        assert_eq!(default_flags(NodeType::SetLocal), 0);
        // DFGNodeType.h:400
        assert_eq!(
            default_flags(NodeType::Call),
            NODE_RESULT_JS | NODE_MUST_GENERATE | NODE_HAS_VAR_ARGS
        );
        // Pure markers carry no result bits (DFGNodeType.h:84-92).
        assert_eq!(default_flags(NodeType::MovHint) & NODE_RESULT_MASK, 0);
        assert_eq!(default_flags(NodeType::Phantom) & NODE_RESULT_MASK, 0);
        assert_eq!(default_flags(NodeType::Return) & NODE_RESULT_MASK, 0);
    }
}
