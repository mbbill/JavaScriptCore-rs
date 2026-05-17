//! B3 and Air optimizer contracts.
//!
//! B3/Air are compiler IR layers used beneath FTL and some Wasm paths. This
//! module records graph, value, block, and lowering contracts without an
//! optimizer, register allocator, or instruction selector.

use crate::runtime::CodeBlockId;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3ProcedureId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3ValueId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3BlockId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B3ValueKind {
    Constant,
    Argument,
    Memory,
    Arithmetic,
    Check,
    Patchpoint,
    CCall,
    Control,
    Tuple,
    Upsilon,
    Phi,
    Effects,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B3ProcedureDescriptor {
    pub id: B3ProcedureId,
    pub owner: Option<CodeBlockId>,
    pub blocks: Vec<B3BlockId>,
    pub values: Vec<B3ValueId>,
    pub requires_stackmap: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AirCodeId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AirBlockId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AirInstructionId(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AirLoweringPlan {
    pub source: Option<B3ProcedureId>,
    pub output: Option<AirCodeId>,
    pub entry_block: Option<AirBlockId>,
    pub terminal_instructions: Vec<AirInstructionId>,
    pub preserves_patchpoints: bool,
    pub needs_stackmap_generation: bool,
}
