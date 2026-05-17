use crate::bytecode::code_block::{
    BytecodeIndex, CallSiteIndex, Checkpoint, ConstantCellIndex, IdentifierSetIndex,
    LinkTimeConstant,
};
use crate::bytecode::opcode::{
    Opcode, OpcodeSchemaVersion, OperandKind, OperandSpec, OperandWidth,
};
use crate::bytecode::register::{RegisterOperandEncoding, VirtualRegister};

/// Safe generated instruction view.
///
/// This is a schema-level representation for validators, generators, and tests.
/// It is not the runtime byte layout consumed by LLInt or JIT code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedInstruction {
    pub opcode: Opcode,
    pub width: OperandWidth,
    pub operands: Vec<Operand>,
    pub schema: Option<InstructionSchemaRef>,
    pub bytecode_index: Option<BytecodeIndex>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operand {
    Register(VirtualRegister),
    EncodedRegister(RegisterOperandEncoding),
    SignedImmediate(i32),
    UnsignedImmediate(u32),
    ConstantPoolIndex(u32),
    ConstantCell(ConstantCellIndex),
    IdentifierIndex(u32),
    IdentifierSet(IdentifierSetIndex),
    FunctionDeclIndex(u32),
    FunctionExprIndex(u32),
    BytecodeIndex(BytecodeIndex),
    Label(LabelRef),
    JumpTableIndex(u32),
    MetadataIndex(u32),
    InlineCacheIndex(u32),
    ProfileIndex(u32),
    Checkpoint(Checkpoint),
    CallSite(CallSiteIndex),
    LinkTimeConstant(LinkTimeConstant),
    RuntimeType(RuntimeTypeRef),
    SchemaReserved(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct LabelRef(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct RuntimeTypeRef(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct InstructionSchemaRef {
    pub opcode: Opcode,
    pub operand_start: u32,
    pub operand_count: u16,
}

/// Mutable schema staging boundary.
///
/// JSC's real bytecode writer packs bytes, seeks, rewinds, and patches labels.
/// This Rust type only records requested instruction shapes and unresolved
/// labels. A future generated encoder should consume this state and produce the
/// packed byte stream.
#[derive(Debug, Default)]
pub struct InstructionBuilder {
    declarations: Vec<InstructionDeclaration>,
    labels: Vec<LabelDeclaration>,
    checkpoints: Vec<CheckpointSpec>,
}

impl InstructionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn declare_instruction(
        &mut self,
        opcode: Opcode,
        width: OperandWidth,
        operands: Vec<Operand>,
    ) -> InstructionDeclarationRef {
        let reference = InstructionDeclarationRef(self.declarations.len() as u32);
        self.declarations.push(InstructionDeclaration {
            opcode,
            width,
            operands,
            bytecode_index: None,
        });
        reference
    }

    pub fn declare_label(&mut self, name: Option<&'static str>) -> LabelRef {
        let reference = LabelRef(self.labels.len() as u32);
        self.labels.push(LabelDeclaration {
            reference,
            name,
            binding: LabelBinding::Unbound,
        });
        reference
    }

    pub fn record_checkpoint(&mut self, spec: CheckpointSpec) {
        self.checkpoints.push(spec);
    }

    pub fn declarations(&self) -> &[InstructionDeclaration] {
        &self.declarations
    }

    pub fn labels(&self) -> &[LabelDeclaration] {
        &self.labels
    }

    pub fn checkpoints(&self) -> &[CheckpointSpec] {
        &self.checkpoints
    }

    pub fn finalize(self) -> PackedInstructionStream {
        PackedInstructionStream::from_schema_staging(
            InstructionStreamLayout::default(),
            self.declarations,
            self.labels,
            self.checkpoints,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct InstructionDeclarationRef(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionDeclaration {
    pub opcode: Opcode,
    pub width: OperandWidth,
    pub operands: Vec<Operand>,
    pub bytecode_index: Option<BytecodeIndex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LabelDeclaration {
    pub reference: LabelRef,
    pub name: Option<&'static str>,
    pub binding: LabelBinding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LabelBinding {
    Unbound,
    Bound(BytecodeIndex),
    OutOfLine(i32),
}

/// Frozen bytecode representation.
///
/// Unsafe packed decoding, alignment, width prefixes, and JIT/LLInt ABI layout
/// must be confined to this representation once real bytecode exists. Until the
/// generated encoder lands, the stream carries schema staging records only.
#[derive(Clone, Debug, Default)]
pub struct PackedInstructionStream {
    layout: InstructionStreamLayout,
    raw: PackedByteStorage,
    typed_placeholder: Vec<TypedInstruction>,
    declarations: Vec<InstructionDeclaration>,
    labels: Vec<LabelDeclaration>,
    checkpoints: Vec<CheckpointSpec>,
}

impl PackedInstructionStream {
    pub fn from_typed_placeholder(typed_placeholder: Vec<TypedInstruction>) -> Self {
        Self {
            typed_placeholder,
            ..Self::default()
        }
    }

    pub fn from_schema_staging(
        layout: InstructionStreamLayout,
        declarations: Vec<InstructionDeclaration>,
        labels: Vec<LabelDeclaration>,
        checkpoints: Vec<CheckpointSpec>,
    ) -> Self {
        Self {
            layout,
            declarations,
            labels,
            checkpoints,
            raw: PackedByteStorage::Unencoded,
            typed_placeholder: Vec::new(),
        }
    }

    pub fn layout(&self) -> InstructionStreamLayout {
        self.layout
    }

    pub fn raw(&self) -> &PackedByteStorage {
        &self.raw
    }

    pub fn typed_placeholder(&self) -> &[TypedInstruction] {
        &self.typed_placeholder
    }

    pub fn declarations(&self) -> &[InstructionDeclaration] {
        &self.declarations
    }

    pub fn labels(&self) -> &[LabelDeclaration] {
        &self.labels
    }

    pub fn checkpoints(&self) -> &[CheckpointSpec] {
        &self.checkpoints
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct InstructionStreamLayout {
    pub schema_version: OpcodeSchemaVersion,
    pub opcode_id_width: OpcodeIdWidth,
    pub default_operand_width: OperandWidth,
    pub width_prefixes: WidthPrefixPolicy,
    pub byte_order: BytecodeByteOrder,
}

impl Default for InstructionStreamLayout {
    fn default() -> Self {
        Self {
            schema_version: OpcodeSchemaVersion::default(),
            opcode_id_width: OpcodeIdWidth::Narrow,
            default_operand_width: OperandWidth::Narrow,
            width_prefixes: WidthPrefixPolicy::WidePrefixes,
            byte_order: BytecodeByteOrder::Native,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OpcodeIdWidth {
    Narrow,
    Wide16,
    Wide32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WidthPrefixPolicy {
    None,
    WidePrefixes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BytecodeByteOrder {
    Native,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PackedByteStorage {
    #[default]
    Unencoded,
    Owned(Vec<u8>),
    External(PackedInstructionBytesRef),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct PackedInstructionBytesRef(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointSpec {
    pub bytecode_index: BytecodeIndex,
    pub checkpoint: Checkpoint,
    pub name: Option<&'static str>,
    pub value_profile_operand: Option<OperandSpec>,
}

#[derive(Clone, Debug, Default)]
pub struct BytecodeVerifier {
    schema_version: OpcodeSchemaVersion,
}

impl BytecodeVerifier {
    pub fn new(schema_version: OpcodeSchemaVersion) -> Self {
        Self { schema_version }
    }

    pub fn verify_schema_only(&self, stream: &PackedInstructionStream) -> VerificationReport {
        let mut findings = Vec::new();
        if stream.layout.schema_version != self.schema_version {
            findings.push(VerificationFinding::SchemaVersionMismatch {
                expected: self.schema_version,
                actual: stream.layout.schema_version,
            });
        }

        for declaration in &stream.declarations {
            for operand in &declaration.operands {
                if let Some(kind) = operand.kind_hint() {
                    findings.push(VerificationFinding::OperandBoundaryNamed {
                        opcode: declaration.opcode,
                        kind,
                    });
                }
            }
        }

        VerificationReport { findings }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VerificationReport {
    pub findings: Vec<VerificationFinding>,
}

impl VerificationReport {
    pub fn status(&self) -> VerificationResult {
        if self.findings.is_empty() {
            VerificationResult::DeferredClean
        } else {
            VerificationResult::DeferredWithFindings
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationFinding {
    SchemaVersionMismatch {
        expected: OpcodeSchemaVersion,
        actual: OpcodeSchemaVersion,
    },
    OperandBoundaryNamed {
        opcode: Opcode,
        kind: OperandKind,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationResult {
    DeferredClean,
    DeferredWithFindings,
}

impl Operand {
    fn kind_hint(&self) -> Option<OperandKind> {
        Some(match self {
            Self::Register(_) | Self::EncodedRegister(_) => OperandKind::VirtualRegister,
            Self::SignedImmediate(_) => OperandKind::SignedImmediate,
            Self::UnsignedImmediate(_) => OperandKind::UnsignedImmediate,
            Self::ConstantPoolIndex(_) | Self::ConstantCell(_) => OperandKind::ConstantPoolIndex,
            Self::IdentifierIndex(_) => OperandKind::IdentifierIndex,
            Self::IdentifierSet(_) => return None,
            Self::FunctionDeclIndex(_) => OperandKind::FunctionDeclIndex,
            Self::FunctionExprIndex(_) => OperandKind::FunctionExprIndex,
            Self::BytecodeIndex(_) => OperandKind::BytecodeIndex,
            Self::Label(_) => OperandKind::BoundLabel,
            Self::JumpTableIndex(_) => OperandKind::JumpTableIndex,
            Self::MetadataIndex(_) => OperandKind::MetadataIndex,
            Self::InlineCacheIndex(_) => OperandKind::InlineCacheIndex,
            Self::ProfileIndex(_) => OperandKind::ProfileIndex,
            Self::Checkpoint(_) => return None,
            Self::CallSite(_) => return None,
            Self::LinkTimeConstant(_) => OperandKind::LinkTimeConstant,
            Self::RuntimeType(_) => OperandKind::RuntimeType,
            Self::SchemaReserved(_) => return None,
        })
    }
}
