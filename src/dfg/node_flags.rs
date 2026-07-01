//! DFG node flags: the per-node `NodeFlags` bitset.
//!
//! Faithful port of C++ `dfg/DFGNodeFlags.h`. The low three bits encode the
//! node's result representation (`NodeResultMask`); the remaining bits carry
//! DCE/varargs markers, profiled arithmetic behavior (overflow/neg-zero), and
//! backwards-propagated bytecode use information. Every DFG node starts from
//! the per-`NodeType` default flags table in `node_type.rs`
//! (`defaultFlags(NodeType)`, dfg/DFGNodeType.h:696).

/// `typedef uint32_t NodeFlags` (DFGNodeFlags.h:84).
pub type NodeFlags = u32;

// Result representation bits. These are a 3-bit enumeration, not independent
// flags: `result() == flags & NodeResultMask` (DFGNodeFlags.h:37-44).
pub const NODE_RESULT_MASK: NodeFlags = 0x0007;
pub const NODE_RESULT_JS: NodeFlags = 0x0001;
pub const NODE_RESULT_NUMBER: NodeFlags = 0x0002;
pub const NODE_RESULT_DOUBLE: NodeFlags = 0x0003;
pub const NODE_RESULT_INT32: NodeFlags = 0x0004;
pub const NODE_RESULT_INT52: NodeFlags = 0x0005;
pub const NODE_RESULT_BOOLEAN: NodeFlags = 0x0006;
pub const NODE_RESULT_STORAGE: NodeFlags = 0x0007;

/// Set on nodes that have side effects and may not trivially be removed by
/// DCE (DFGNodeFlags.h:46).
pub const NODE_MUST_GENERATE: NodeFlags = 0x0008;
/// The node's children live in the varargs area (DFGNodeFlags.h:47).
pub const NODE_HAS_VAR_ARGS: NodeFlags = 0x0010;

// Profiled arithmetic behavior (DFGNodeFlags.h:49-58).
pub const NODE_MAY_HAVE_DOUBLE_RESULT: NodeFlags = 0x00020;
pub const NODE_MAY_OVERFLOW_INT52: NodeFlags = 0x00040;
pub const NODE_MAY_OVERFLOW_INT32_IN_BASELINE: NodeFlags = 0x00080;
pub const NODE_MAY_OVERFLOW_INT32_IN_DFG: NodeFlags = 0x00100;
pub const NODE_MAY_NEG_ZERO_IN_BASELINE: NodeFlags = 0x00200;
pub const NODE_MAY_NEG_ZERO_IN_DFG: NodeFlags = 0x00400;
pub const NODE_MAY_HAVE_BIG_INT32_RESULT: NodeFlags = 0x00800;
pub const NODE_MAY_HAVE_HEAP_BIG_INT_RESULT: NodeFlags = 0x01000;
pub const NODE_MAY_HAVE_NON_NUMERIC_RESULT: NodeFlags = 0x02000;
/// (DFGNodeFlags.h:59)
pub const NODE_BEHAVIOR_MASK: NodeFlags = NODE_MAY_HAVE_DOUBLE_RESULT
    | NODE_MAY_OVERFLOW_INT52
    | NODE_MAY_OVERFLOW_INT32_IN_BASELINE
    | NODE_MAY_OVERFLOW_INT32_IN_DFG
    | NODE_MAY_NEG_ZERO_IN_BASELINE
    | NODE_MAY_NEG_ZERO_IN_DFG
    | NODE_MAY_HAVE_BIG_INT32_RESULT
    | NODE_MAY_HAVE_HEAP_BIG_INT_RESULT
    | NODE_MAY_HAVE_NON_NUMERIC_RESULT;
/// (DFGNodeFlags.h:60)
pub const NODE_MAY_HAVE_NON_INT_RESULT: NodeFlags = NODE_MAY_HAVE_DOUBLE_RESULT
    | NODE_MAY_HAVE_NON_NUMERIC_RESULT
    | NODE_MAY_HAVE_BIG_INT32_RESULT
    | NODE_MAY_HAVE_HEAP_BIG_INT_RESULT;

// Backwards-propagated bytecode use information (DFGNodeFlags.h:62-72).
pub const NODE_BYTECODE_USE_BOTTOM: NodeFlags = 0x00000;
pub const NODE_BYTECODE_USES_AS_NUMBER: NodeFlags = 0x04000;
pub const NODE_BYTECODE_NEEDS_NEG_ZERO: NodeFlags = 0x08000;
pub const NODE_BYTECODE_NEEDS_NAN_OR_INFINITY: NodeFlags = 0x10000;
pub const NODE_BYTECODE_USES_AS_OTHER: NodeFlags = 0x20000;
pub const NODE_BYTECODE_USES_AS_INT: NodeFlags = 0x40000;
pub const NODE_BYTECODE_PREFERS_ARRAY_INDEX: NodeFlags = 0x80000;
/// (DFGNodeFlags.h:69)
pub const NODE_BYTECODE_USES_AS_ARRAY_INDEX: NodeFlags = NODE_BYTECODE_USES_AS_NUMBER
    | NODE_BYTECODE_NEEDS_NAN_OR_INFINITY
    | NODE_BYTECODE_USES_AS_OTHER
    | NODE_BYTECODE_USES_AS_INT
    | NODE_BYTECODE_PREFERS_ARRAY_INDEX;
/// (DFGNodeFlags.h:70)
pub const NODE_BYTECODE_USES_AS_VALUE: NodeFlags = NODE_BYTECODE_USES_AS_NUMBER
    | NODE_BYTECODE_NEEDS_NEG_ZERO
    | NODE_BYTECODE_NEEDS_NAN_OR_INFINITY
    | NODE_BYTECODE_USES_AS_OTHER;
/// (DFGNodeFlags.h:71)
pub const NODE_BYTECODE_BACK_PROP_MASK: NodeFlags = NODE_BYTECODE_USES_AS_NUMBER
    | NODE_BYTECODE_NEEDS_NEG_ZERO
    | NODE_BYTECODE_NEEDS_NAN_OR_INFINITY
    | NODE_BYTECODE_USES_AS_OTHER
    | NODE_BYTECODE_USES_AS_INT
    | NODE_BYTECODE_PREFERS_ARRAY_INDEX;

/// (DFGNodeFlags.h:74)
pub const NODE_ARITH_FLAGS_MASK: NodeFlags = NODE_BEHAVIOR_MASK | NODE_BYTECODE_BACK_PROP_MASK;

/// Computed by CPSRethreadingPhase: which local nodes are backwards-reachable
/// from a Flush (DFGNodeFlags.h:76).
pub const NODE_IS_FLUSHED: NodeFlags = 0x100000;

pub const NODE_MISC_FLAG1: NodeFlags = 0x200000;
pub const NODE_MISC_FLAG2: NodeFlags = 0x400000;

/// `bytecodeUsesAsNumber` (DFGNodeFlags.h:86-89).
pub const fn bytecode_uses_as_number(flags: NodeFlags) -> bool {
    flags & NODE_BYTECODE_USES_AS_NUMBER != 0
}

/// `bytecodeCanTruncateInteger` (DFGNodeFlags.h:91-94).
pub const fn bytecode_can_truncate_integer(flags: NodeFlags) -> bool {
    !bytecode_uses_as_number(flags)
}

/// `bytecodeCanIgnoreNegativeZero` (DFGNodeFlags.h:96-99).
pub const fn bytecode_can_ignore_negative_zero(flags: NodeFlags) -> bool {
    flags & NODE_BYTECODE_NEEDS_NEG_ZERO == 0
}

/// `bytecodeCanIgnoreNaNAndInfinity` (DFGNodeFlags.h:101-104).
pub const fn bytecode_can_ignore_nan_and_infinity(flags: NodeFlags) -> bool {
    flags & NODE_BYTECODE_NEEDS_NAN_OR_INFINITY == 0
}
