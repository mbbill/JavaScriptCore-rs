use crate::bytecode::code_block::{
    CodeFeatures, CodeGenerationModeSet, CodeKind, ExecutableInfo, ParseMode, SourceProvenance,
    UnlinkedCodeBlock, UnlinkedCodeBlockPhase, UnlinkedConstantPool, UnlinkedSideTables,
};
use crate::bytecode::gc::{build_p6_no_js_call_helper_root_maps, BytecodeRootMapId};
use crate::bytecode::instruction::{
    InstructionBuilder, InstructionLinker, LabelRef, PackedInstructionStream,
};
use crate::bytecode::register::{
    RegisterFrameShape, SpecialRegisters, TemporaryLifetime, VirtualRegister,
};
use crate::syntax::ParserIdentifier;

/// Bytecode-generation orchestration boundary.
///
/// Future codegen traverses AST or a lowered IR and mutates registers, labels,
/// scopes, handlers, constants, and metadata before producing an
/// `UnlinkedCodeBlock`. This skeleton records the inputs and output ownership
/// surfaces only; it does not traverse syntax or emit JavaScript operations.
#[derive(Debug)]
pub struct BytecodeGenerator {
    plan: GenerationPlan,
    instructions: InstructionBuilder,
    registers: RegisterAllocator,
    labels: LabelArena,
    constants: UnlinkedConstantPool,
    side_tables: UnlinkedSideTables,
    diagnostics: Vec<GenerationDiagnostic>,
}

impl BytecodeGenerator {
    pub fn new(plan: GenerationPlan) -> Self {
        Self {
            registers: RegisterAllocator::new(plan.registers),
            plan,
            instructions: InstructionBuilder::new(),
            labels: LabelArena::default(),
            constants: UnlinkedConstantPool::default(),
            side_tables: UnlinkedSideTables::default(),
            diagnostics: Vec::new(),
        }
    }

    pub fn plan(&self) -> &GenerationPlan {
        &self.plan
    }

    pub fn instructions_mut(&mut self) -> &mut InstructionBuilder {
        &mut self.instructions
    }

    pub fn registers_mut(&mut self) -> &mut RegisterAllocator {
        &mut self.registers
    }

    pub fn labels_mut(&mut self) -> &mut LabelArena {
        &mut self.labels
    }

    pub fn constants_mut(&mut self) -> &mut UnlinkedConstantPool {
        &mut self.constants
    }

    pub fn side_tables_mut(&mut self) -> &mut UnlinkedSideTables {
        &mut self.side_tables
    }

    pub fn diagnostics(&self) -> &[GenerationDiagnostic] {
        &self.diagnostics
    }

    pub fn finish(self) -> GenerationOutput {
        let mut diagnostics = self.diagnostics;
        let staged_stream: PackedInstructionStream = self.instructions.finalize();
        let link_output = InstructionLinker::link_schema_stream(&staged_stream);
        let stream = match link_output.linked {
            Some(stream) => stream,
            None => {
                diagnostics.push(GenerationDiagnostic {
                    phase: GenerationPhase::LabelResolution,
                    message: "unresolved labels left in instruction stream",
                });
                staged_stream
            }
        };
        // POST-GENERATION ENCODER PASS (ratified serial decision, G4-Unit-1):
        // try to ALSO encode this now-frozen function/program's declarations
        // into a real packed byte stream, so `dfg::parser` — which reads
        // ONLY `raw_bytes()` — can see real bytecompiler output. Strictly
        // additive: on decline `raw` stays `Unencoded` exactly as before this
        // pass existed, and the `declarations` domain the interpreter
        // dispatches off is untouched either way (see the method doc).
        let stream = stream.with_raw_encoded_from_declarations();
        let mut side_tables = self.side_tables;
        let first_generated_root_map_id = next_root_map_id(&side_tables);
        {
            let mut decoded_instructions = Vec::with_capacity(stream.instruction_count());
            for decoded in stream.decoded_instructions() {
                match decoded {
                    Ok(instruction) => decoded_instructions.push(instruction),
                    Err(_) => diagnostics.push(GenerationDiagnostic {
                        phase: GenerationPhase::Finalization,
                        message: "failed to decode linked instruction for helper root maps",
                    }),
                }
            }
            match build_p6_no_js_call_helper_root_maps(
                decoded_instructions,
                first_generated_root_map_id,
            ) {
                Ok(root_maps) => side_tables.root_maps.extend(root_maps),
                Err(_) => diagnostics.push(GenerationDiagnostic {
                    phase: GenerationPhase::Finalization,
                    message: "failed to generate helper root map",
                }),
            }
        }
        let code_block = UnlinkedCodeBlock::new(self.plan.kind, stream)
            .with_source(self.plan.source)
            .with_executable_info(self.plan.executable_info)
            .with_features(self.plan.features)
            .with_generation_modes(self.plan.modes)
            .with_side_tables(side_tables)
            .with_frame(self.registers.frame_shape())
            .with_phase(UnlinkedCodeBlockPhase::Finalized);

        GenerationOutput {
            code_block,
            diagnostics,
            labels: self.labels,
        }
    }
}

fn next_root_map_id(side_tables: &UnlinkedSideTables) -> BytecodeRootMapId {
    let next = side_tables
        .root_maps
        .iter()
        .map(|root_map| root_map.id.0)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1);
    BytecodeRootMapId(next)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationPlan {
    pub kind: CodeKind,
    pub parse_mode: ParseMode,
    pub source: SourceProvenance,
    pub executable_info: ExecutableInfo,
    pub features: CodeFeatures,
    pub modes: CodeGenerationModeSet,
    pub registers: RegisterFrameShape,
    pub environment: GenerationEnvironment,
    pub generator_state: GeneratorStatePlan,
    pub mutation_authority: GenerationMutationAuthority,
    pub root: GenerationRoot,
}

impl GenerationPlan {
    pub fn new(kind: CodeKind, parse_mode: ParseMode) -> Self {
        Self {
            kind,
            parse_mode,
            source: SourceProvenance::default(),
            executable_info: ExecutableInfo {
                parse_mode: Some(parse_mode),
                ..ExecutableInfo::default()
            },
            features: CodeFeatures::default(),
            modes: CodeGenerationModeSet::default(),
            registers: RegisterFrameShape::default(),
            environment: GenerationEnvironment::default(),
            generator_state: GeneratorStatePlan::default(),
            mutation_authority: GenerationMutationAuthority::BytecodeGenerator,
            root: GenerationRoot::Unspecified,
        }
    }

    pub fn validate(&self) -> GenerationValidationReport {
        let mut findings = Vec::new();
        if self.executable_info.parse_mode != Some(self.parse_mode) {
            findings.push(GenerationValidationFinding::ExecutableParseModeMismatch {
                expected: self.parse_mode,
                actual: self.executable_info.parse_mode,
            });
        }
        if self.registers.num_callee_locals
            < self
                .registers
                .num_vars
                .saturating_add(self.registers.num_temporaries)
        {
            findings.push(GenerationValidationFinding::FrameShapeTooSmall {
                num_callee_locals: self.registers.num_callee_locals,
                required: self
                    .registers
                    .num_vars
                    .saturating_add(self.registers.num_temporaries),
            });
        }
        validate_special_registers(self.registers.special, &mut findings);
        validate_environment(&self.environment, &mut findings);
        validate_generator_state(&self.generator_state, &mut findings);
        GenerationValidationReport { findings }
    }
}

fn validate_special_registers(
    special: SpecialRegisters,
    findings: &mut Vec<GenerationValidationFinding>,
) {
    if !special.this_register.is_valid() {
        findings.push(GenerationValidationFinding::InvalidSpecialRegister {
            register: SpecialRegisterKind::This,
        });
    }
    if !special.scope_register.is_valid() {
        findings.push(GenerationValidationFinding::InvalidSpecialRegister {
            register: SpecialRegisterKind::Scope,
        });
    }
    for (kind, register) in [
        (SpecialRegisterKind::Arguments, special.arguments_register),
        (SpecialRegisterKind::NewTarget, special.new_target_register),
        (SpecialRegisterKind::Generator, special.generator_register),
        (SpecialRegisterKind::Promise, special.promise_register),
    ] {
        if matches!(register, Some(register) if !register.is_valid()) {
            findings.push(GenerationValidationFinding::InvalidSpecialRegister { register: kind });
        }
    }
}

fn validate_environment(
    environment: &GenerationEnvironment,
    findings: &mut Vec<GenerationValidationFinding>,
) {
    validate_slots(
        &environment.variables_under_tdz,
        EnvironmentSlotList::Tdz,
        findings,
    );
    validate_slots(
        &environment.private_names,
        EnvironmentSlotList::PrivateNames,
        findings,
    );
    validate_slots(
        &environment.captured_variables,
        EnvironmentSlotList::CapturedVariables,
        findings,
    );
}

fn validate_slots(
    slots: &[EnvironmentSlot],
    list: EnvironmentSlotList,
    findings: &mut Vec<GenerationValidationFinding>,
) {
    for (index, slot) in slots.iter().enumerate() {
        if slots[..index]
            .iter()
            .any(|candidate| candidate.index == slot.index && candidate.kind == slot.kind)
        {
            findings
                .push(GenerationValidationFinding::DuplicateEnvironmentSlot { list, slot: *slot });
        }
    }
}

fn validate_generator_state(
    state: &GeneratorStatePlan,
    findings: &mut Vec<GenerationValidationFinding>,
) {
    if state.needs_generatorification
        && (state.state_register.is_none()
            || state.value_register.is_none()
            || state.resume_mode_register.is_none()
            || state.frame_register.is_none())
    {
        findings.push(GenerationValidationFinding::GeneratorificationMissingRegisters);
    }
    if !state.needs_generatorification && !state.yield_points.is_empty() {
        findings.push(GenerationValidationFinding::YieldPointsWithoutGeneratorification);
    }
    for register in [
        state.state_register,
        state.value_register,
        state.resume_mode_register,
        state.frame_register,
        state.promise_register,
    ]
    .into_iter()
    .flatten()
    {
        if !register.is_valid() {
            findings.push(GenerationValidationFinding::InvalidGeneratorRegister { register });
        }
    }
    for (index, point) in state.yield_points.iter().enumerate() {
        if state.yield_points[..index]
            .iter()
            .any(|candidate| candidate.state == point.state)
        {
            findings.push(GenerationValidationFinding::DuplicateYieldState { state: point.state });
        }
    }
}

/// Owner allowed to mutate generation staging state.
///
/// This does not grant permission to mutate linked runtime data. It only names
/// the component currently allowed to append instructions, labels, metadata
/// plans, constants, and unlinked side tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum GenerationMutationAuthority {
    #[default]
    BytecodeGenerator,
    BytecodeRewriter,
    UnlinkedCodeBlockGenerator,
    SchemaGenerator,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GenerationEnvironment {
    pub strict_mode: bool,
    pub variables_under_tdz: Vec<EnvironmentSlot>,
    pub private_names: Vec<EnvironmentSlot>,
    pub captured_variables: Vec<EnvironmentSlot>,
    pub parent_private_names: Vec<ParserIdentifier>,
    pub parent_tdz_environment: Option<ParentEnvironmentRef>,
    pub allows_direct_eval_cache: bool,
    pub needs_full_activation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ParentEnvironmentRef(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct EnvironmentSlot {
    pub index: u32,
    pub kind: EnvironmentSlotKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EnvironmentSlotKind {
    Var,
    Lexical,
    PrivateName,
    Module,
    DirectArgumentsObject,
}

/// Generator and async-function resume-state contract.
///
/// JSC generatorification reserves parameter slots and hidden fields for
/// resume mode, yielded value, frame storage, promises, and async-generator
/// suspend reasons. Rust keeps those slots explicit and separate from ordinary
/// temporaries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GeneratorStatePlan {
    pub state_register: Option<VirtualRegister>,
    pub value_register: Option<VirtualRegister>,
    pub resume_mode_register: Option<VirtualRegister>,
    pub frame_register: Option<VirtualRegister>,
    pub promise_register: Option<VirtualRegister>,
    pub yield_points: Vec<YieldPointPlan>,
    pub needs_generatorification: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct YieldPointPlan {
    pub label: Label,
    pub state: i32,
    pub kind: YieldPointKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum YieldPointKind {
    Yield,
    Await,
    DelegateYield,
    AsyncGeneratorYield,
    AsyncGeneratorAwait,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum GenerationRoot {
    #[default]
    Unspecified,
    ProgramNode,
    EvalNode,
    FunctionNode,
    ModuleProgramNode,
    LoweredIr,
}

#[derive(Clone, Debug)]
pub struct GenerationOutput {
    pub code_block: UnlinkedCodeBlock,
    pub diagnostics: Vec<GenerationDiagnostic>,
    pub labels: LabelArena,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationDiagnostic {
    pub phase: GenerationPhase,
    pub message: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GenerationPhase {
    Planning,
    RegisterAllocation,
    LabelResolution,
    MetadataLayout,
    Finalization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct Label(pub u32);

/// Register allocator state owned by bytecode generation only.
#[derive(Clone, Debug)]
pub struct RegisterAllocator {
    next_local: u32,
    next_temporary: u32,
    frame: RegisterFrameShape,
}

impl RegisterAllocator {
    pub fn new(frame: RegisterFrameShape) -> Self {
        Self {
            next_local: frame.num_vars,
            next_temporary: frame.num_temporaries,
            frame,
        }
    }

    pub fn reserve_local(&mut self) -> VirtualRegister {
        let register = VirtualRegister::local(self.next_local);
        self.next_local = self.next_local.saturating_add(1);
        self.frame.num_vars = self.frame.num_vars.max(self.next_local);
        self.update_frame_capacity();
        register
    }

    pub fn reserve_temporary(&mut self, _lifetime: TemporaryLifetime) -> VirtualRegister {
        let register =
            VirtualRegister::local(self.frame.num_vars.saturating_add(self.next_temporary));
        self.next_temporary = self.next_temporary.saturating_add(1);
        self.frame.num_temporaries = self.frame.num_temporaries.max(self.next_temporary);
        self.update_frame_capacity();
        register
    }

    pub fn set_special_registers(&mut self, special: SpecialRegisters) {
        self.frame.special = special;
    }

    pub fn frame_shape(&self) -> RegisterFrameShape {
        self.frame
    }

    fn update_frame_capacity(&mut self) {
        self.frame.num_callee_locals = self.frame.num_callee_locals.max(
            self.frame
                .num_vars
                .saturating_add(self.frame.num_temporaries),
        );
    }
}

impl Default for RegisterAllocator {
    fn default() -> Self {
        Self::new(RegisterFrameShape::default())
    }
}

#[derive(Clone, Debug, Default)]
pub struct LabelArena {
    labels: Vec<LabelRecord>,
}

impl LabelArena {
    pub fn allocate(&mut self, name: Option<&'static str>) -> LabelRef {
        let reference = LabelRef(self.labels.len() as u32);
        self.labels.push(LabelRecord {
            reference,
            name,
            state: LabelState::Unbound,
        });
        reference
    }

    pub fn labels(&self) -> &[LabelRecord] {
        &self.labels
    }

    pub fn validate(&self) -> GenerationValidationReport {
        let mut findings = Vec::new();
        for (index, label) in self.labels.iter().enumerate() {
            if usize::try_from(label.reference.0).ok() != Some(index) {
                findings.push(GenerationValidationFinding::LabelReferenceMismatch {
                    expected: LabelRef(index as u32),
                    actual: label.reference,
                });
            }
            if self.labels[..index]
                .iter()
                .any(|candidate| candidate.reference == label.reference)
            {
                findings.push(GenerationValidationFinding::DuplicateLabel {
                    label: label.reference,
                });
            }
        }
        GenerationValidationReport { findings }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LabelRecord {
    pub reference: LabelRef,
    pub name: Option<&'static str>,
    pub state: LabelState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LabelState {
    Unbound,
    BoundToInstruction(u32),
    OutOfLine,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GenerationValidationReport {
    pub findings: Vec<GenerationValidationFinding>,
}

impl GenerationValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationValidationFinding {
    ExecutableParseModeMismatch {
        expected: ParseMode,
        actual: Option<ParseMode>,
    },
    FrameShapeTooSmall {
        num_callee_locals: u32,
        required: u32,
    },
    InvalidSpecialRegister {
        register: SpecialRegisterKind,
    },
    DuplicateEnvironmentSlot {
        list: EnvironmentSlotList,
        slot: EnvironmentSlot,
    },
    GeneratorificationMissingRegisters,
    YieldPointsWithoutGeneratorification,
    InvalidGeneratorRegister {
        register: VirtualRegister,
    },
    DuplicateYieldState {
        state: i32,
    },
    LabelReferenceMismatch {
        expected: LabelRef,
        actual: LabelRef,
    },
    DuplicateLabel {
        label: LabelRef,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialRegisterKind {
    This,
    Scope,
    Arguments,
    NewTarget,
    Generator,
    Promise,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentSlotList {
    Tdz,
    PrivateNames,
    CapturedVariables,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{
        BytecodeIndex, BytecodeRootSlotKind, BytecodeRootSlotStorage, CoreOpcode, Operand,
        OperandWidth,
    };

    #[test]
    fn generation_plan_validation_accepts_explicit_frame_and_registers() {
        let mut plan = GenerationPlan::new(CodeKind::Function, ParseMode::NormalFunction);
        plan.registers = RegisterFrameShape {
            num_vars: 1,
            num_temporaries: 1,
            num_callee_locals: 2,
            special: SpecialRegisters {
                this_register: VirtualRegister::argument_or_header(0),
                scope_register: VirtualRegister::local(0),
                ..SpecialRegisters::default()
            },
            ..RegisterFrameShape::default()
        };

        assert!(plan.validate().is_valid());
    }

    #[test]
    fn generation_plan_validation_reports_missing_generator_registers() {
        let mut plan = GenerationPlan::new(CodeKind::Function, ParseMode::GeneratorBody);
        plan.registers.special.this_register = VirtualRegister::argument_or_header(0);
        plan.registers.special.scope_register = VirtualRegister::local(0);
        plan.generator_state.needs_generatorification = true;
        plan.generator_state.yield_points.push(YieldPointPlan {
            label: Label(0),
            state: 1,
            kind: YieldPointKind::Yield,
        });
        plan.generator_state.yield_points.push(YieldPointPlan {
            label: Label(1),
            state: 1,
            kind: YieldPointKind::Yield,
        });

        let findings = plan.validate().findings;
        assert!(findings.contains(&GenerationValidationFinding::GeneratorificationMissingRegisters));
        assert!(findings.contains(&GenerationValidationFinding::DuplicateYieldState { state: 1 }));
    }

    #[test]
    fn register_allocator_temporary_does_not_extend_var_prefix() {
        let mut allocator = RegisterAllocator::new(RegisterFrameShape {
            num_vars: 1,
            num_callee_locals: 1,
            special: SpecialRegisters {
                this_register: VirtualRegister::argument_or_header(0),
                scope_register: VirtualRegister::local(0),
                ..SpecialRegisters::default()
            },
            ..RegisterFrameShape::default()
        });

        let temporary = allocator.reserve_temporary(TemporaryLifetime::Expression);
        let frame = allocator.frame_shape();

        assert_eq!(temporary.to_local_index(), Some(1));
        assert_eq!(frame.num_vars, 1);
        assert_eq!(frame.num_temporaries, 1);
        assert_eq!(frame.num_callee_locals, 2);
        let mut plan = GenerationPlan::new(CodeKind::Function, ParseMode::NormalFunction);
        plan.registers = frame;
        assert!(plan.validate().is_valid());
    }

    #[test]
    fn register_allocator_local_prefix_precedes_temporary_window() {
        let mut allocator = RegisterAllocator::new(RegisterFrameShape {
            num_vars: 1,
            num_callee_locals: 1,
            special: SpecialRegisters {
                this_register: VirtualRegister::argument_or_header(0),
                scope_register: VirtualRegister::local(0),
                ..SpecialRegisters::default()
            },
            ..RegisterFrameShape::default()
        });

        let local = allocator.reserve_local();
        let temporary = allocator.reserve_temporary(TemporaryLifetime::Expression);
        let frame = allocator.frame_shape();

        assert_eq!(local.to_local_index(), Some(1));
        assert_eq!(temporary.to_local_index(), Some(2));
        assert_eq!(frame.num_vars, 2);
        assert_eq!(frame.num_temporaries, 1);
        assert_eq!(frame.num_callee_locals, 3);
    }

    #[test]
    fn bytecode_generator_finishes_finalized_unlinked_block() {
        let mut plan = GenerationPlan::new(CodeKind::Program, ParseMode::Program);
        plan.registers.special.this_register = VirtualRegister::argument_or_header(0);
        plan.registers.special.scope_register = VirtualRegister::local(0);
        let mut generator = BytecodeGenerator::new(plan);
        let label = generator.instructions_mut().declare_label(Some("done"));
        assert!(generator
            .instructions_mut()
            .bind_label(label, crate::bytecode::BytecodeIndex::from_offset(0)));
        generator.instructions_mut().declare_instruction(
            crate::bytecode::Opcode::Reserved,
            crate::bytecode::OperandWidth::Narrow,
            vec![crate::bytecode::Operand::Label(label)],
        );

        let output = generator.finish();

        assert!(output.diagnostics.is_empty());
        assert_eq!(output.code_block.phase(), UnlinkedCodeBlockPhase::Finalized);
        assert_eq!(
            output.code_block.instructions().lifecycle(),
            crate::bytecode::PackedInstructionLifecycle::Linked
        );
    }

    #[test]
    fn bytecode_generator_finalization_emits_ownerless_helper_root_maps_at_linked_indices() {
        let mut plan = GenerationPlan::new(CodeKind::Program, ParseMode::Program);
        plan.registers.special.this_register = VirtualRegister::argument_or_header(0);
        plan.registers.special.scope_register = VirtualRegister::local(0);
        let mut generator = BytecodeGenerator::new(plan);
        let destination = VirtualRegister::local(0);
        let source = VirtualRegister::argument_or_header(5);
        generator.instructions_mut().declare_instruction(
            CoreOpcode::LoadUndefined.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination)],
        );
        generator.instructions_mut().declare_instruction(
            CoreOpcode::TypeOf.opcode(),
            OperandWidth::Narrow,
            vec![Operand::Register(destination), Operand::Register(source)],
        );

        let output = generator.finish();
        let root_maps = &output.code_block.side_tables().root_maps;

        assert!(output.diagnostics.is_empty());
        assert_eq!(root_maps.len(), 1);
        assert_eq!(root_maps[0].owner, None);
        assert_eq!(
            root_maps[0].bytecode_range_start,
            BytecodeIndex::from_offset(1)
        );
        assert_eq!(
            root_maps[0].bytecode_range_end,
            BytecodeIndex::from_offset(1)
        );
        assert_eq!(root_maps[0].slots.len(), 2);
        assert_eq!(
            root_maps[0].slots[0].storage,
            BytecodeRootSlotStorage::Register(destination)
        );
        assert_eq!(
            root_maps[0].slots[0].kind,
            BytecodeRootSlotKind::VirtualRegister
        );
        assert_eq!(
            root_maps[0].slots[1].storage,
            BytecodeRootSlotStorage::Register(source)
        );
        assert_eq!(root_maps[0].slots[1].kind, BytecodeRootSlotKind::Argument);
    }
}
