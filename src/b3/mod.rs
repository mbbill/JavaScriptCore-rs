//! B3 and Air optimizer contracts.
//!
//! B3/Air are compiler IR layers used beneath FTL and some Wasm paths. This
//! module records graph, value, block, and lowering contracts without an
//! optimizer, register allocator, or instruction selector.

use crate::assembler::{AssemblerBufferId, AssemblerLabel};
use crate::jit::{CallBoundaryId, EffectSummary, JitType, MachineCodeHandle};
use crate::runtime::CodeBlockId;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3ProcedureId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3ValueId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3BlockId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3VariableId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct B3StackSlotId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B3Type {
    Void,
    Int32,
    Int64,
    Float,
    Double,
    V128,
    Tuple,
    Pointer,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B3MutationAuthority {
    FrontendBuilder,
    OptimizationPhase,
    LoweringPhase,
    ValidationOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B3ProcedureState {
    Building,
    CfgAvailable,
    Optimizing,
    LoweringToAir,
    Generated,
    Invalidated,
}

/// Effect summary mirrored from `B3::Effects`. Mutation belongs to the phase
/// that owns the value being described; analysis users must treat this as
/// read-only input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct B3Effects {
    pub terminal: bool,
    pub exits_sideways: bool,
    pub control_dependent: bool,
    pub reads_local_state: bool,
    pub writes_local_state: bool,
    pub reads_pinned: bool,
    pub writes_pinned: bool,
    pub fence: bool,
    pub reads_heap: bool,
    pub writes_heap: bool,
}

impl B3Effects {
    pub const fn none() -> Self {
        Self {
            terminal: false,
            exits_sideways: false,
            control_dependent: false,
            reads_local_state: false,
            writes_local_state: false,
            reads_pinned: false,
            writes_pinned: false,
            fence: false,
            reads_heap: false,
            writes_heap: false,
        }
    }

    pub const fn for_call() -> Self {
        Self {
            terminal: false,
            exits_sideways: true,
            control_dependent: true,
            reads_local_state: false,
            writes_local_state: false,
            reads_pinned: true,
            writes_pinned: true,
            fence: true,
            reads_heap: true,
            writes_heap: true,
        }
    }

    pub const fn for_check() -> Self {
        Self {
            terminal: false,
            exits_sideways: true,
            control_dependent: false,
            reads_local_state: false,
            writes_local_state: false,
            reads_pinned: false,
            writes_pinned: false,
            fence: false,
            reads_heap: true,
            writes_heap: false,
        }
    }

    pub const fn effect_summary(self) -> EffectSummary {
        EffectSummary {
            reads_heap: self.reads_heap,
            writes_heap: self.writes_heap,
            allocates: false,
            may_call_js: self.reads_pinned && self.writes_pinned && self.fence,
            may_throw: self.exits_sideways,
            may_exit: self.exits_sideways,
            terminates: self.terminal,
            reads_local_state: self.reads_local_state,
            writes_local_state: self.writes_local_state,
            reads_pinned: self.reads_pinned,
            writes_pinned: self.writes_pinned,
            fence: self.fence,
        }
    }

    pub const fn must_execute(self) -> bool {
        self.effect_summary().must_preserve_order()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B3ValueDescriptor {
    pub id: B3ValueId,
    pub kind: B3ValueKind,
    pub value_type: B3Type,
    pub children: Vec<B3ValueId>,
    pub owner_block: Option<B3BlockId>,
    pub effects: B3Effects,
    pub mutation_authority: B3MutationAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B3BlockDescriptor {
    pub id: B3BlockId,
    pub values: Vec<B3ValueId>,
    pub predecessors: Vec<B3BlockId>,
    pub successors: Vec<B3BlockId>,
    pub frequency_class: Option<B3FrequencyClass>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum B3FrequencyClass {
    Rare,
    Normal,
    Frequent,
    Entry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B3ProcedureDescriptor {
    pub id: B3ProcedureId,
    pub owner: Option<CodeBlockId>,
    pub state: B3ProcedureState,
    pub blocks: Vec<B3BlockId>,
    pub values: Vec<B3ValueId>,
    pub variables: Vec<B3VariableId>,
    pub stack_slots: Vec<B3StackSlotId>,
    pub requires_stackmap: bool,
    pub has_quirks: bool,
    /// Procedure and Air code are paired facades in C++; this field records the
    /// Rust-side counterpart without giving either side mutation authority over
    /// the other.
    pub paired_air_code: Option<AirCodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B3DominanceSet {
    pub block: B3BlockId,
    pub dominators: Vec<B3BlockId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum B3ValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicatePlanName(&'static str),
    EmptyAllowedValueKinds(&'static str),
    DuplicateBlockId(B3BlockId),
    DuplicateValueId(B3ValueId),
    MissingBlock(B3BlockId),
    MissingValue(B3ValueId),
    ValueOwnerMissing(B3ValueId),
    ValueChildMissing(B3ValueId),
    TerminalValueNotLast(B3BlockId),
    ProcedureAirMismatch,
    AirLoweringMissingSource,
    AirLoweringMissingOutput,
    PatchpointOriginMissing(AirInstructionId),
    ValueCycle(B3ValueId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct B3ProcedureBuilder {
    procedure: B3ProcedureDescriptor,
    blocks: Vec<B3BlockDescriptor>,
    values: Vec<B3ValueDescriptor>,
}

impl B3ProcedureBuilder {
    pub fn new(id: B3ProcedureId) -> Self {
        Self {
            procedure: B3ProcedureDescriptor {
                id,
                owner: None,
                state: B3ProcedureState::Building,
                blocks: Vec::new(),
                values: Vec::new(),
                variables: Vec::new(),
                stack_slots: Vec::new(),
                requires_stackmap: false,
                has_quirks: false,
                paired_air_code: None,
            },
            blocks: Vec::new(),
            values: Vec::new(),
        }
    }

    pub fn owner(mut self, owner: CodeBlockId) -> Self {
        self.procedure.owner = Some(owner);
        self
    }

    pub fn state(mut self, state: B3ProcedureState) -> Self {
        self.procedure.state = state;
        self
    }

    pub fn block(mut self, block: B3BlockDescriptor) -> Self {
        self.procedure.blocks.push(block.id);
        self.blocks.push(block);
        self
    }

    pub fn value(mut self, value: B3ValueDescriptor) -> Self {
        self.procedure.values.push(value.id);
        self.values.push(value);
        self
    }

    pub fn build(
        self,
    ) -> Result<
        (
            B3ProcedureDescriptor,
            Vec<B3BlockDescriptor>,
            Vec<B3ValueDescriptor>,
        ),
        B3ValidationError,
    > {
        validate_b3_procedure_parts(&self.procedure, &self.blocks, &self.values)?;
        Ok((self.procedure, self.blocks, self.values))
    }
}

impl B3ProcedureDescriptor {
    pub fn builder(id: B3ProcedureId) -> B3ProcedureBuilder {
        B3ProcedureBuilder::new(id)
    }
}

pub fn validate_b3_procedure_parts(
    procedure: &B3ProcedureDescriptor,
    blocks: &[B3BlockDescriptor],
    values: &[B3ValueDescriptor],
) -> Result<(), B3ValidationError> {
    for block_id in &procedure.blocks {
        if !blocks.iter().any(|block| block.id == *block_id) {
            return Err(B3ValidationError::MissingBlock(*block_id));
        }
    }
    for value_id in &procedure.values {
        if !values.iter().any(|value| value.id == *value_id) {
            return Err(B3ValidationError::MissingValue(*value_id));
        }
    }

    for (index, block) in blocks.iter().enumerate() {
        if blocks[index + 1..].iter().any(|other| other.id == block.id) {
            return Err(B3ValidationError::DuplicateBlockId(block.id));
        }
        block.validate(values)?;
    }

    for (index, value) in values.iter().enumerate() {
        if values[index + 1..].iter().any(|other| other.id == value.id) {
            return Err(B3ValidationError::DuplicateValueId(value.id));
        }
        value.validate(blocks, values)?;
    }

    Ok(())
}

impl B3ValueDescriptor {
    pub fn validate(
        &self,
        blocks: &[B3BlockDescriptor],
        values: &[B3ValueDescriptor],
    ) -> Result<(), B3ValidationError> {
        if let Some(owner_block) = self.owner_block {
            if !blocks.iter().any(|block| block.id == owner_block) {
                return Err(B3ValidationError::ValueOwnerMissing(self.id));
            }
        }
        for child in &self.children {
            if !values.iter().any(|value| value.id == *child) {
                return Err(B3ValidationError::ValueChildMissing(*child));
            }
        }

        Ok(())
    }
}

impl B3BlockDescriptor {
    pub fn validate(&self, values: &[B3ValueDescriptor]) -> Result<(), B3ValidationError> {
        for value in &self.values {
            if !values.iter().any(|candidate| candidate.id == *value) {
                return Err(B3ValidationError::MissingValue(*value));
            }
        }
        for (position, value_id) in self.values.iter().enumerate() {
            if let Some(value) = values.iter().find(|candidate| candidate.id == *value_id) {
                if value.effects.terminal && position + 1 != self.values.len() {
                    return Err(B3ValidationError::TerminalValueNotLast(self.id));
                }
            }
        }

        Ok(())
    }
}

pub fn b3_reverse_post_order(
    blocks: &[B3BlockDescriptor],
    entry: B3BlockId,
) -> Result<Vec<B3BlockId>, B3ValidationError> {
    if !blocks.iter().any(|block| block.id == entry) {
        return Err(B3ValidationError::MissingBlock(entry));
    }

    let mut visited = Vec::new();
    let mut postorder = Vec::new();
    visit_b3_block_postorder(blocks, entry, &mut visited, &mut postorder)?;
    postorder.reverse();
    Ok(postorder)
}

fn visit_b3_block_postorder(
    blocks: &[B3BlockDescriptor],
    block_id: B3BlockId,
    visited: &mut Vec<B3BlockId>,
    postorder: &mut Vec<B3BlockId>,
) -> Result<(), B3ValidationError> {
    if visited.contains(&block_id) {
        return Ok(());
    }
    visited.push(block_id);
    let block = blocks
        .iter()
        .find(|candidate| candidate.id == block_id)
        .ok_or(B3ValidationError::MissingBlock(block_id))?;
    for successor in &block.successors {
        visit_b3_block_postorder(blocks, *successor, visited, postorder)?;
    }
    postorder.push(block_id);
    Ok(())
}

pub fn b3_dominance_sets(
    blocks: &[B3BlockDescriptor],
    entry: B3BlockId,
) -> Result<Vec<B3DominanceSet>, B3ValidationError> {
    if !blocks.iter().any(|block| block.id == entry) {
        return Err(B3ValidationError::MissingBlock(entry));
    }

    let all_blocks: Vec<B3BlockId> = blocks.iter().map(|block| block.id).collect();
    let mut dominators: Vec<B3DominanceSet> = all_blocks
        .iter()
        .map(|block_id| B3DominanceSet {
            block: *block_id,
            dominators: if *block_id == entry {
                vec![entry]
            } else {
                all_blocks.clone()
            },
        })
        .collect();

    let mut changed = true;
    while changed {
        changed = false;
        for block in blocks {
            if block.id == entry {
                continue;
            }

            let mut next = all_blocks.clone();
            if block.predecessors.is_empty() {
                next.clear();
            }
            for predecessor in &block.predecessors {
                let predecessor_dominators = dominators
                    .iter()
                    .find(|set| set.block == *predecessor)
                    .ok_or(B3ValidationError::MissingBlock(*predecessor))?;
                next.retain(|candidate| predecessor_dominators.dominators.contains(candidate));
            }
            if !next.contains(&block.id) {
                next.push(block.id);
            }
            next.sort();

            let current = dominators
                .iter_mut()
                .find(|set| set.block == block.id)
                .ok_or(B3ValidationError::MissingBlock(block.id))?;
            if current.dominators != next {
                current.dominators = next;
                changed = true;
            }
        }
    }

    Ok(dominators)
}

pub fn b3_scheduled_values_in_block(
    block: &B3BlockDescriptor,
    values: &[B3ValueDescriptor],
) -> Result<Vec<B3ValueId>, B3ValidationError> {
    block.validate(values)?;
    let mut schedule = Vec::new();
    let mut visiting = Vec::new();
    for value in &block.values {
        schedule_b3_value(*value, block, values, &mut visiting, &mut schedule)?;
    }
    Ok(schedule)
}

fn schedule_b3_value(
    value_id: B3ValueId,
    block: &B3BlockDescriptor,
    values: &[B3ValueDescriptor],
    visiting: &mut Vec<B3ValueId>,
    schedule: &mut Vec<B3ValueId>,
) -> Result<(), B3ValidationError> {
    if schedule.contains(&value_id) {
        return Ok(());
    }
    if visiting.contains(&value_id) {
        return Err(B3ValidationError::ValueCycle(value_id));
    }
    visiting.push(value_id);

    let value = values
        .iter()
        .find(|candidate| candidate.id == value_id)
        .ok_or(B3ValidationError::MissingValue(value_id))?;
    for child in &value.children {
        if block.values.contains(child) {
            schedule_b3_value(*child, block, values, visiting, schedule)?;
        }
    }

    visiting.retain(|visiting_value| *visiting_value != value_id);
    schedule.push(value_id);
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AirCodeId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AirBlockId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AirInstructionId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AirTmpId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirRegisterBank {
    GeneralPurpose,
    FloatingPoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirStackSlotKind {
    Spill,
    Locked,
    Argument,
    CallArg,
    WasmPinned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirAllocationStage {
    PreLowering,
    Lowered,
    RegisterAllocation,
    StackAllocation,
    CodeGeneration,
    Linked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirMutationAuthority {
    LowerMacros,
    RegisterAllocator,
    StackAllocator,
    CodeGenerator,
    DiagnosticsOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AirCodeDescriptor {
    pub id: AirCodeId,
    pub procedure: B3ProcedureId,
    pub stage: AirAllocationStage,
    pub blocks: Vec<AirBlockId>,
    pub tmps: Vec<AirTmpId>,
    pub stack_slots: Vec<B3StackSlotId>,
    pub mutable_register_banks: Vec<AirRegisterBank>,
    pub pinned_register_count: u32,
    pub frame_size_bytes: Option<u32>,
    pub call_arg_area_size_bytes: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AirPatchpointDescriptor {
    pub instruction: AirInstructionId,
    pub origin_value: Option<B3ValueId>,
    pub boundary: Option<CallBoundaryId>,
    pub entry_label: Option<AssemblerLabel>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AirLoweringPlan {
    pub source: Option<B3ProcedureId>,
    pub output: Option<AirCodeId>,
    pub entry_block: Option<AirBlockId>,
    pub terminal_instructions: Vec<AirInstructionId>,
    pub patchpoints: Vec<AirPatchpointDescriptor>,
    pub preserves_patchpoints: bool,
    pub needs_stackmap_generation: bool,
    pub machine_code: Option<MachineCodeHandle>,
    pub assembler_buffer: Option<AssemblerBufferId>,
    /// After this point, Air register allocation and code generation own
    /// mutation of temporaries, stack slots, and frame-size metadata.
    pub mutation_authority: Option<AirMutationAuthority>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum B3PlanSchemaOwner {
    #[default]
    B3ProcedureRegistry,
    FtlLowering,
    AirLowering,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum B3PlanRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticB3PlanDescriptor {
    pub name: &'static str,
    pub target_tier: JitType,
    pub initial_state: B3ProcedureState,
    pub allowed_value_kinds: &'static [B3ValueKind],
    pub output_stage: AirAllocationStage,
    pub procedure_authority: B3MutationAuthority,
    pub air_authority: AirMutationAuthority,
    pub owner: B3PlanSchemaOwner,
    pub mutation_authority: B3PlanRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct B3PlanDescriptorRegistry {
    pub descriptors: &'static [StaticB3PlanDescriptor],
}

impl B3PlanDescriptorRegistry {
    pub const fn new(descriptors: &'static [StaticB3PlanDescriptor]) -> Self {
        Self { descriptors }
    }

    pub const fn descriptors(self) -> &'static [StaticB3PlanDescriptor] {
        self.descriptors
    }

    pub fn descriptor_for_name(self, name: &str) -> Option<&'static StaticB3PlanDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn validate(self) -> Result<(), B3ValidationError> {
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            descriptor.validate()?;
            if self.descriptors[index + 1..]
                .iter()
                .any(|other| other.name == descriptor.name)
            {
                return Err(B3ValidationError::DuplicatePlanName(descriptor.name));
            }
        }

        Ok(())
    }
}

impl StaticB3PlanDescriptor {
    pub fn validate(&self) -> Result<(), B3ValidationError> {
        if self.name.is_empty() {
            return Err(B3ValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(B3ValidationError::EmptyProvenance(self.name));
        }
        if self.allowed_value_kinds.is_empty() {
            return Err(B3ValidationError::EmptyAllowedValueKinds(self.name));
        }

        Ok(())
    }
}

impl AirLoweringPlan {
    pub fn validate(&self) -> Result<(), B3ValidationError> {
        if self.source.is_none() {
            return Err(B3ValidationError::AirLoweringMissingSource);
        }
        if self.output.is_none() {
            return Err(B3ValidationError::AirLoweringMissingOutput);
        }
        for patchpoint in &self.patchpoints {
            if patchpoint.origin_value.is_none() && patchpoint.boundary.is_none() {
                return Err(B3ValidationError::PatchpointOriginMissing(
                    patchpoint.instruction,
                ));
            }
        }

        Ok(())
    }
}

const B3_FTL_VALUE_KINDS: &[B3ValueKind] = &[
    B3ValueKind::Constant,
    B3ValueKind::Argument,
    B3ValueKind::Memory,
    B3ValueKind::Arithmetic,
    B3ValueKind::Check,
    B3ValueKind::Patchpoint,
    B3ValueKind::CCall,
    B3ValueKind::Control,
    B3ValueKind::Tuple,
    B3ValueKind::Upsilon,
    B3ValueKind::Phi,
    B3ValueKind::Effects,
];

pub const STATIC_B3_PLAN_DESCRIPTORS: &[StaticB3PlanDescriptor] = &[StaticB3PlanDescriptor {
    name: "ftl-b3-procedure",
    target_tier: JitType::Ftl,
    initial_state: B3ProcedureState::Building,
    allowed_value_kinds: B3_FTL_VALUE_KINDS,
    output_stage: AirAllocationStage::Lowered,
    procedure_authority: B3MutationAuthority::FrontendBuilder,
    air_authority: AirMutationAuthority::LowerMacros,
    owner: B3PlanSchemaOwner::FtlLowering,
    mutation_authority: B3PlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
    provenance: "static Rust B3/Air plan schema",
}];

pub const B3_PLAN_DESCRIPTOR_REGISTRY: B3PlanDescriptorRegistry =
    B3PlanDescriptorRegistry::new(STATIC_B3_PLAN_DESCRIPTORS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_b3_plan_registry_validates() {
        assert_eq!(B3_PLAN_DESCRIPTOR_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn b3_builder_rejects_missing_child_value() {
        let block = B3BlockDescriptor {
            id: B3BlockId(0),
            values: vec![B3ValueId(0)],
            predecessors: Vec::new(),
            successors: Vec::new(),
            frequency_class: Some(B3FrequencyClass::Entry),
        };
        let value = B3ValueDescriptor {
            id: B3ValueId(0),
            kind: B3ValueKind::Arithmetic,
            value_type: B3Type::Int32,
            children: vec![B3ValueId(1)],
            owner_block: Some(B3BlockId(0)),
            effects: B3Effects::default(),
            mutation_authority: B3MutationAuthority::FrontendBuilder,
        };

        let procedure = B3ProcedureDescriptor::builder(B3ProcedureId(1))
            .block(block)
            .value(value)
            .build();

        assert_eq!(
            procedure,
            Err(B3ValidationError::ValueChildMissing(B3ValueId(1)))
        );
    }

    #[test]
    fn b3_traversal_orders_blocks_and_dominators() {
        let entry = B3BlockDescriptor {
            id: B3BlockId(0),
            values: Vec::new(),
            predecessors: Vec::new(),
            successors: vec![B3BlockId(1)],
            frequency_class: Some(B3FrequencyClass::Entry),
        };
        let exit = B3BlockDescriptor {
            id: B3BlockId(1),
            values: Vec::new(),
            predecessors: vec![B3BlockId(0)],
            successors: Vec::new(),
            frequency_class: Some(B3FrequencyClass::Normal),
        };
        let blocks = vec![entry, exit];

        assert_eq!(
            b3_reverse_post_order(&blocks, B3BlockId(0)),
            Ok(vec![B3BlockId(0), B3BlockId(1)])
        );
        assert_eq!(
            b3_dominance_sets(&blocks, B3BlockId(0))
                .unwrap()
                .into_iter()
                .find(|set| set.block == B3BlockId(1))
                .map(|set| set.dominators),
            Some(vec![B3BlockId(0), B3BlockId(1)])
        );
    }

    #[test]
    fn b3_value_scheduler_places_children_before_parent() {
        let block = B3BlockDescriptor {
            id: B3BlockId(0),
            values: vec![B3ValueId(0), B3ValueId(1)],
            predecessors: Vec::new(),
            successors: Vec::new(),
            frequency_class: Some(B3FrequencyClass::Entry),
        };
        let child = B3ValueDescriptor {
            id: B3ValueId(0),
            kind: B3ValueKind::Argument,
            value_type: B3Type::Int32,
            children: Vec::new(),
            owner_block: Some(B3BlockId(0)),
            effects: B3Effects::default(),
            mutation_authority: B3MutationAuthority::FrontendBuilder,
        };
        let parent = B3ValueDescriptor {
            id: B3ValueId(1),
            kind: B3ValueKind::Arithmetic,
            value_type: B3Type::Int32,
            children: vec![B3ValueId(0)],
            owner_block: Some(B3BlockId(0)),
            effects: B3Effects::default(),
            mutation_authority: B3MutationAuthority::FrontendBuilder,
        };

        assert_eq!(
            b3_scheduled_values_in_block(&block, &[child, parent]),
            Ok(vec![B3ValueId(0), B3ValueId(1)])
        );
    }

    #[test]
    fn b3_call_and_check_effects_have_distinct_semantics() {
        let call = B3Effects::for_call().effect_summary();
        let check = B3Effects::for_check().effect_summary();

        assert!(call.may_call_js);
        assert!(call.mutates_world());
        assert!(check.may_exit);
        assert!(!check.mutates_world());
    }
}
