use crate::bytecode::code_block::{
    CodeFeatures, CodeGenerationModeSet, CodeKind, ExecutableInfo, ParseMode, SourceProvenance,
    UnlinkedCodeBlock, UnlinkedConstantPool, UnlinkedSideTables,
};
use crate::bytecode::instruction::{InstructionBuilder, LabelRef, PackedInstructionStream};
use crate::bytecode::register::{
    RegisterFrameShape, SpecialRegisters, TemporaryLifetime, VirtualRegister,
};

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
        let stream: PackedInstructionStream = self.instructions.finalize();
        let code_block = UnlinkedCodeBlock::new(self.plan.kind, stream)
            .with_source(self.plan.source)
            .with_executable_info(self.plan.executable_info)
            .with_features(self.plan.features)
            .with_generation_modes(self.plan.modes)
            .with_frame(self.registers.frame_shape());

        GenerationOutput {
            code_block,
            diagnostics: self.diagnostics,
            labels: self.labels,
        }
    }
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
            root: GenerationRoot::Unspecified,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GenerationEnvironment {
    pub strict_mode: bool,
    pub variables_under_tdz: Vec<EnvironmentSlot>,
    pub private_names: Vec<EnvironmentSlot>,
    pub captured_variables: Vec<EnvironmentSlot>,
    pub allows_direct_eval_cache: bool,
}

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
            next_temporary: 0,
            frame,
        }
    }

    pub fn reserve_local(&mut self) -> VirtualRegister {
        let register = VirtualRegister::local(self.next_local);
        self.next_local = self.next_local.saturating_add(1);
        self.frame.num_vars = self.frame.num_vars.max(self.next_local);
        register
    }

    pub fn reserve_temporary(&mut self, _lifetime: TemporaryLifetime) -> VirtualRegister {
        let index = self.frame.num_vars.saturating_add(self.next_temporary);
        let register = VirtualRegister::local(index);
        self.next_temporary = self.next_temporary.saturating_add(1);
        self.frame.num_temporaries = self.frame.num_temporaries.max(self.next_temporary);
        self.frame.num_callee_locals = self.frame.num_callee_locals.max(
            self.frame
                .num_vars
                .saturating_add(self.frame.num_temporaries),
        );
        register
    }

    pub fn set_special_registers(&mut self, special: SpecialRegisters) {
        self.frame.special = special;
    }

    pub fn frame_shape(&self) -> RegisterFrameShape {
        self.frame
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
