use crate::bytecode::code_block::{
    BytecodeIndex, CallSiteIndex, Checkpoint, ConstantCellIndex, IdentifierSetIndex,
    LinkTimeConstant, BYTECODE_INDEX_CHECKPOINTS,
};
use crate::bytecode::instruction_stream::{self as raw_stream, RawInstructionDecodeError};
use crate::bytecode::opcode::{
    CoreOpcode, Opcode, OpcodeSchemaVersion, OperandKind, OperandSpec, OperandWidth,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

impl Operand {
    pub const fn kind(&self) -> OperandKind {
        match self {
            Self::Register(_) | Self::EncodedRegister(_) => OperandKind::VirtualRegister,
            Self::SignedImmediate(_) => OperandKind::SignedImmediate,
            Self::UnsignedImmediate(_) | Self::SchemaReserved(_) => OperandKind::UnsignedImmediate,
            Self::ConstantPoolIndex(_) | Self::ConstantCell(_) => OperandKind::ConstantPoolIndex,
            Self::IdentifierIndex(_) => OperandKind::IdentifierIndex,
            Self::IdentifierSet(_) => OperandKind::UnsignedImmediate,
            Self::FunctionDeclIndex(_) => OperandKind::FunctionDeclIndex,
            Self::FunctionExprIndex(_) => OperandKind::FunctionExprIndex,
            Self::BytecodeIndex(_) => OperandKind::BytecodeIndex,
            Self::Label(_) => OperandKind::BoundLabel,
            Self::JumpTableIndex(_) => OperandKind::JumpTableIndex,
            Self::MetadataIndex(_) => OperandKind::MetadataIndex,
            Self::InlineCacheIndex(_) => OperandKind::InlineCacheIndex,
            Self::ProfileIndex(_) => OperandKind::ProfileIndex,
            Self::Checkpoint(_) => OperandKind::UnsignedImmediate,
            Self::CallSite(_) => OperandKind::UnsignedImmediate,
            Self::LinkTimeConstant(_) => OperandKind::LinkTimeConstant,
            Self::RuntimeType(_) => OperandKind::RuntimeType,
        }
    }

    pub const fn as_register(self) -> Option<VirtualRegister> {
        match self {
            Self::Register(register) => Some(register),
            Self::EncodedRegister(encoded) => Some(encoded.register),
            _ => None,
        }
    }

    pub const fn as_signed_immediate(self) -> Option<i32> {
        match self {
            Self::SignedImmediate(value) => Some(value),
            _ => None,
        }
    }

    pub const fn as_unsigned_immediate(self) -> Option<u32> {
        match self {
            Self::UnsignedImmediate(value) | Self::SchemaReserved(value) => Some(value),
            _ => None,
        }
    }

    pub const fn as_bytecode_index(self) -> Option<BytecodeIndex> {
        match self {
            Self::BytecodeIndex(index) => Some(index),
            _ => None,
        }
    }

    pub const fn as_metadata_index(self) -> Option<u32> {
        match self {
            Self::MetadataIndex(index) => Some(index),
            _ => None,
        }
    }
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
    state: InstructionBuilderState,
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

    pub fn bind_label(&mut self, reference: LabelRef, bytecode_index: BytecodeIndex) -> bool {
        let Some(label) = self
            .labels
            .get_mut(usize::try_from(reference.0).unwrap_or(usize::MAX))
        else {
            return false;
        };
        if label.reference != reference || !bytecode_index.is_valid() {
            return false;
        }
        label.binding = LabelBinding::Bound(bytecode_index);
        self.state = InstructionBuilderState::LabelsBeingBound;
        true
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

    pub fn state(&self) -> InstructionBuilderState {
        self.state
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InstructionBuilderState {
    #[default]
    OpenForDeclarations,
    LabelsBeingBound,
    ReadyForPacking,
    Packed,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedInstruction<'a> {
    pub opcode: Opcode,
    pub width: OperandWidth,
    pub bytecode_index: BytecodeIndex,
    pub operands: &'a [Operand],
    pub schema: Option<InstructionSchemaRef>,
    pub source: DecodedInstructionSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedDecodedInstruction {
    pub opcode: Opcode,
    pub width: OperandWidth,
    pub bytecode_index: BytecodeIndex,
    pub next_bytecode_index: Option<BytecodeIndex>,
    pub operands: Vec<Operand>,
    pub source: DecodedInstructionSource,
}

impl<'a> DecodedInstruction<'a> {
    pub fn operand(&self, index: usize) -> Result<Operand, OperandAccessError> {
        self.operands
            .get(index)
            .copied()
            .ok_or(OperandAccessError::MissingOperand {
                opcode: self.opcode,
                index: index as u32,
            })
    }

    pub fn register_operand(&self, index: usize) -> Result<VirtualRegister, OperandAccessError> {
        let operand = self.operand(index)?;
        operand
            .as_register()
            .filter(|register| register.is_valid())
            .ok_or(OperandAccessError::UnexpectedOperandKind {
                opcode: self.opcode,
                index: index as u32,
                expected: OperandKind::VirtualRegister,
                actual: operand.kind(),
            })
    }

    pub fn signed_immediate_operand(&self, index: usize) -> Result<i32, OperandAccessError> {
        let operand = self.operand(index)?;
        operand
            .as_signed_immediate()
            .ok_or(OperandAccessError::UnexpectedOperandKind {
                opcode: self.opcode,
                index: index as u32,
                expected: OperandKind::SignedImmediate,
                actual: operand.kind(),
            })
    }

    pub fn unsigned_immediate_operand(&self, index: usize) -> Result<u32, OperandAccessError> {
        let operand = self.operand(index)?;
        operand
            .as_unsigned_immediate()
            .ok_or(OperandAccessError::UnexpectedOperandKind {
                opcode: self.opcode,
                index: index as u32,
                expected: OperandKind::UnsignedImmediate,
                actual: operand.kind(),
            })
    }

    /// Read an `IdentifierIndex` operand (a `get_by_id`/`put_by_id` property
    /// identifier — the index into the host's interned identifier table the
    /// interpreter resolves with `identifier_property_key`). Mirrors
    /// `register_operand` etc.; used by the baseline property-IC lowering to bake
    /// the property name the DataIC slow-path bridge re-resolves.
    pub fn identifier_index_operand(&self, index: usize) -> Result<u32, OperandAccessError> {
        let operand = self.operand(index)?;
        match operand {
            Operand::IdentifierIndex(value) => Ok(value),
            _ => Err(OperandAccessError::UnexpectedOperandKind {
                opcode: self.opcode,
                index: index as u32,
                expected: OperandKind::IdentifierIndex,
                actual: operand.kind(),
            }),
        }
    }

    pub fn bytecode_index_operand(
        &self,
        index: usize,
    ) -> Result<BytecodeIndex, OperandAccessError> {
        let operand = self.operand(index)?;
        operand
            .as_bytecode_index()
            .filter(|index| index.is_valid())
            .ok_or(OperandAccessError::UnexpectedOperandKind {
                opcode: self.opcode,
                index: index as u32,
                expected: OperandKind::BytecodeIndex,
                actual: operand.kind(),
            })
    }

    pub fn metadata_index_operand(&self, index: usize) -> Result<u32, OperandAccessError> {
        let operand = self.operand(index)?;
        operand
            .as_metadata_index()
            .ok_or(OperandAccessError::UnexpectedOperandKind {
                opcode: self.opcode,
                index: index as u32,
                expected: OperandKind::MetadataIndex,
                actual: operand.kind(),
            })
    }

    pub fn named_operand(
        &self,
        specs: &[OperandSpec],
        name: &str,
    ) -> Result<Operand, OperandAccessError> {
        let Some((index, _)) = specs.iter().enumerate().find(|(_, spec)| spec.name == name) else {
            return Err(OperandAccessError::UnknownOperandName {
                opcode: self.opcode,
            });
        };
        self.operand(index)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DecodedInstructionSource {
    Declaration,
    TypedPlaceholder,
    RawPackedBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperandAccessError {
    MissingOperand {
        opcode: Opcode,
        index: u32,
    },
    UnexpectedOperandKind {
        opcode: Opcode,
        index: u32,
        expected: OperandKind,
        actual: OperandKind,
    },
    UnknownOperandName {
        opcode: Opcode,
    },
}

/// Owner of immutable bytecode declaration tables.
///
/// These tables describe generated bytecode records before an encoder exists.
/// The bytecompiler may consume them by reference, but table replacement is
/// owned by generated bytecode schema data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum BytecodeDeclarationOwner {
    #[default]
    GeneratedBytecodeSchema,
    BytecompilerFrontend,
    TestFixture,
}

/// Immutable declaration for an instruction shape at a bytecode position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticInstructionDeclaration {
    pub opcode: Opcode,
    pub width: OperandWidth,
    pub operands: &'static [OperandSpec],
    pub schema: Option<InstructionSchemaRef>,
    pub bytecode_index: Option<BytecodeIndex>,
}

impl StaticInstructionDeclaration {
    pub const fn operands(self) -> &'static [OperandSpec] {
        self.operands
    }
}

/// Immutable declaration for a label known to generated bytecode metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StaticLabelDeclaration {
    pub reference: LabelRef,
    pub name: Option<&'static str>,
    pub binding: LabelBinding,
}

/// Immutable bytecode declaration table.
///
/// It records existing static metadata only. It does not allocate labels,
/// resolve jumps, pack bytes, or validate operand compatibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeDeclarationTable {
    pub schema_version: OpcodeSchemaVersion,
    pub owner: BytecodeDeclarationOwner,
    pub instructions: &'static [StaticInstructionDeclaration],
    pub labels: &'static [StaticLabelDeclaration],
    pub checkpoints: &'static [CheckpointSpec],
}

impl BytecodeDeclarationTable {
    pub const fn instructions(self) -> &'static [StaticInstructionDeclaration] {
        self.instructions
    }

    pub const fn labels(self) -> &'static [StaticLabelDeclaration] {
        self.labels
    }

    pub const fn checkpoints(self) -> &'static [CheckpointSpec] {
        self.checkpoints
    }

    pub fn first_declaration_for_opcode(
        self,
        opcode: Opcode,
    ) -> Option<&'static StaticInstructionDeclaration> {
        self.instructions
            .iter()
            .find(|declaration| declaration.opcode == opcode)
    }

    pub fn validate(self) -> VerificationReport {
        let mut findings = Vec::new();
        validate_static_declarations(self, &mut findings);
        VerificationReport { findings }
    }
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

/// Authority allowed to patch bytecode labels or instruction bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InstructionPatchAuthority {
    #[default]
    InstructionBuilder,
    BytecodeRewriter,
    Linker,
    JitThunkGenerator,
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
    lifecycle: PackedInstructionLifecycle,
}

impl PackedInstructionStream {
    pub fn from_typed_placeholder(typed_placeholder: Vec<TypedInstruction>) -> Self {
        Self {
            typed_placeholder,
            lifecycle: PackedInstructionLifecycle::SchemaStaged,
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
            lifecycle: PackedInstructionLifecycle::SchemaStaged,
        }
    }

    pub fn from_raw_packed_bytes(bytes: Vec<u8>) -> Self {
        Self {
            raw: PackedByteStorage::Owned(bytes),
            lifecycle: PackedInstructionLifecycle::Encoded,
            ..Self::default()
        }
    }

    /// Encoded stream whose bytes live behind an external reference. No byte
    /// access exists yet, so every decode surface reports
    /// `RawBytesRequireGeneratedDecoder` (present-but-unreadable, never
    /// silently empty).
    pub fn from_external_packed_bytes(reference: PackedInstructionBytesRef) -> Self {
        Self {
            raw: PackedByteStorage::External(reference),
            lifecycle: PackedInstructionLifecycle::Encoded,
            ..Self::default()
        }
    }

    pub fn layout(&self) -> InstructionStreamLayout {
        self.layout
    }

    pub fn raw(&self) -> &PackedByteStorage {
        &self.raw
    }

    pub fn raw_bytes(&self) -> Option<&[u8]> {
        match &self.raw {
            PackedByteStorage::Owned(bytes) => Some(bytes),
            PackedByteStorage::External(_) | PackedByteStorage::Unencoded => None,
        }
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

    pub fn lifecycle(&self) -> PackedInstructionLifecycle {
        self.lifecycle
    }

    /// Number of INSTRUCTIONS in the stream (cardinality), never a byte size
    /// and never a `BytecodeIndex` offset bound.
    ///
    /// C++ has no such accessor: `InstructionStream::size()` is a BYTE count
    /// (`InstructionStream.h:183-186`) and instruction cardinality only exists
    /// by iterating. For the ordinal-domain representations
    /// (declarations/typed placeholder) this doubles as the exclusive bound of
    /// the ordinal `decoded_at` domain; for raw packed streams it is obtained
    /// by walking whole instructions (`size()` steps) and MUST NOT be compared
    /// against byte offsets — the ordinal decode APIs reject raw streams with
    /// `RawBytesRequireGeneratedDecoder` (see `decoded_at`).
    pub fn instruction_count(&self) -> usize {
        if !self.declarations.is_empty() {
            self.declarations.len()
        } else if !self.typed_placeholder.is_empty() {
            self.typed_placeholder.len()
        } else if let Some(bytes) = self.raw_bytes() {
            let mut count = 0usize;
            let mut offset = 0usize;
            while offset < bytes.len() {
                let Ok(decoded) = raw_stream::decode_raw_instruction(bytes, offset) else {
                    break;
                };
                count = count.saturating_add(1);
                offset = offset.saturating_add(decoded.size);
            }
            count
        } else {
            0
        }
    }

    pub fn decoded_raw_at(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Result<OwnedDecodedInstruction, InstructionDecodeError> {
        if !bytecode_index.is_valid() {
            return Err(InstructionDecodeError::InvalidBytecodeIndex { bytecode_index });
        }
        if !self.declarations.is_empty() || !self.typed_placeholder.is_empty() {
            return Err(InstructionDecodeError::MixedInstructionRepresentations);
        }
        let bytes = self
            .raw_bytes()
            .ok_or(InstructionDecodeError::RawBytesRequireGeneratedDecoder)?;
        let offset = usize::try_from(bytecode_index.offset())
            .map_err(|_| InstructionDecodeError::MissingInstruction { bytecode_index })?;
        // C++ InstructionStream only ever yields instruction STARTS (iteration
        // advances by size(), InstructionStream.h:154-161; at() ASSERTs bounds
        // of a trusted offset, InstructionStream.h:177-181). This safe-Rust
        // dispatch surface accepts arbitrary BytecodeIndex values, so it must
        // reject any offset that is not an instruction start — otherwise an
        // operand byte that happens to equal an opcode id would decode as a
        // bogus instruction (e.g. a RET conjured from inside a MOV's operands).
        match raw_stream::is_instruction_start(bytes, offset) {
            Ok(true) => {}
            Ok(false) => {
                return Err(InstructionDecodeError::InvalidBytecodeIndex { bytecode_index })
            }
            Err(error) => {
                return Err(InstructionDecodeError::RawInstructionDecode {
                    bytecode_index,
                    error,
                })
            }
        }
        let decoded = raw_stream::decode_raw_instruction(bytes, offset).map_err(|error| {
            InstructionDecodeError::RawInstructionDecode {
                bytecode_index,
                error,
            }
        })?;
        let opcode = CoreOpcode::from_representative_packed_opcode_id(decoded.opcode_id)
            .map(CoreOpcode::opcode)
            .ok_or(InstructionDecodeError::UnsupportedRawOpcode {
                bytecode_index,
                opcode_id: decoded.opcode_id,
            })?;
        let operands = decoded
            .operands
            .into_iter()
            .map(|operand| Operand::Register(VirtualRegister::from_raw(operand as i32)))
            .collect();
        Ok(OwnedDecodedInstruction {
            opcode,
            width: operand_width_for_raw(decoded.width),
            bytecode_index,
            next_bytecode_index: next_raw_bytecode_index(offset, decoded.size, bytes.len())?,
            operands,
            source: DecodedInstructionSource::RawPackedBytes,
        })
    }

    pub fn decoded_at(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Result<DecodedInstruction<'_>, InstructionDecodeError> {
        if !bytecode_index.is_valid() {
            return Err(InstructionDecodeError::InvalidBytecodeIndex { bytecode_index });
        }
        if !self.declarations.is_empty() && !self.typed_placeholder.is_empty() {
            return Err(InstructionDecodeError::MixedInstructionRepresentations);
        }
        if matches!(
            self.raw,
            PackedByteStorage::Owned(_) | PackedByteStorage::External(_)
        ) && self.declarations.is_empty()
            && self.typed_placeholder.is_empty()
        {
            return Err(InstructionDecodeError::RawBytesRequireGeneratedDecoder);
        }

        let ordinal = usize::try_from(bytecode_index.offset())
            .map_err(|_| InstructionDecodeError::MissingInstruction { bytecode_index })?;
        if !self.declarations.is_empty() {
            let declaration = self
                .declarations
                .get(ordinal)
                .ok_or(InstructionDecodeError::MissingInstruction { bytecode_index })?;
            return Ok(DecodedInstruction {
                opcode: declaration.opcode,
                width: declaration.width,
                bytecode_index: declaration.bytecode_index.unwrap_or(bytecode_index),
                operands: &declaration.operands,
                schema: None,
                source: DecodedInstructionSource::Declaration,
            });
        }

        let instruction = self
            .typed_placeholder
            .get(ordinal)
            .ok_or(InstructionDecodeError::MissingInstruction { bytecode_index })?;
        Ok(DecodedInstruction {
            opcode: instruction.opcode,
            width: instruction.width,
            bytecode_index: instruction.bytecode_index.unwrap_or(bytecode_index),
            operands: &instruction.operands,
            schema: instruction.schema,
            source: DecodedInstructionSource::TypedPlaceholder,
        })
    }

    /// Iterate the ORDINAL decode domain (declarations / typed placeholder).
    ///
    /// Raw packed byte streams have no ordinal domain — their positions are
    /// BYTE OFFSETS (`BytecodeIndex.h:48-90`) — so iterating one yields exactly
    /// one `RawBytesRequireGeneratedDecoder` error instead of silently mixing
    /// the two index domains (the same error `decoded_at` returns for raw
    /// streams).
    pub fn decoded_instructions(&self) -> InstructionDecodeIter<'_> {
        InstructionDecodeIter {
            stream: self,
            next_ordinal: 0,
        }
    }

    /// True when the only representation is packed bytes (owned or external),
    /// i.e. the ordinal decode APIs cannot serve this stream.
    fn is_raw_bytes_only(&self) -> bool {
        self.declarations.is_empty()
            && self.typed_placeholder.is_empty()
            && matches!(
                self.raw,
                PackedByteStorage::Owned(_) | PackedByteStorage::External(_)
            )
    }
}

fn operand_width_for_raw(width: raw_stream::OpcodeSize) -> OperandWidth {
    match width {
        raw_stream::OpcodeSize::Narrow => OperandWidth::Narrow,
        raw_stream::OpcodeSize::Wide16 => OperandWidth::Wide16,
        raw_stream::OpcodeSize::Wide32 => OperandWidth::Wide32,
    }
}

/// `BytecodeIndex` packs `offset << checkpointShift | checkpoint`
/// (`BytecodeIndex.h:88-93`), so the largest representable offset is
/// `u32::MAX >> 2`. C++ ASSERTs the pack round-trips
/// (`ASSERT((bytecodeIndex << checkpointShift) >> checkpointShift ==
/// bytecodeIndex)`); the safe-Rust port checks it and returns an error instead
/// of silently truncating the offset.
fn next_raw_bytecode_index(
    offset: usize,
    size: usize,
    len: usize,
) -> Result<Option<BytecodeIndex>, InstructionDecodeError> {
    let next = offset
        .checked_add(size)
        .ok_or(InstructionDecodeError::BytecodeOffsetOverflow { offset })?;
    if next >= len {
        return Ok(None);
    }
    let next = u32::try_from(next)
        .ok()
        .filter(|next| *next <= u32::MAX >> 2)
        .ok_or(InstructionDecodeError::BytecodeOffsetOverflow { offset: next })?;
    Ok(Some(BytecodeIndex::from_offset(next)))
}

#[derive(Clone, Debug)]
pub struct InstructionDecodeIter<'a> {
    stream: &'a PackedInstructionStream,
    next_ordinal: usize,
}

impl<'a> Iterator for InstructionDecodeIter<'a> {
    type Item = Result<DecodedInstruction<'a>, InstructionDecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stream.is_raw_bytes_only() {
            // Raw packed streams are byte-offset indexed, not ordinal indexed:
            // report the ordinal API's raw-stream rejection ONCE and stop
            // instead of repeating it `instruction_count()` times.
            if self.next_ordinal == 0 {
                self.next_ordinal = usize::MAX;
                return Some(Err(InstructionDecodeError::RawBytesRequireGeneratedDecoder));
            }
            return None;
        }
        if self.next_ordinal >= self.stream.instruction_count() {
            return None;
        }
        let bytecode_index = BytecodeIndex::from_offset(self.next_ordinal as u32);
        self.next_ordinal += 1;
        Some(self.stream.decoded_at(bytecode_index))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionDecodeError {
    InvalidBytecodeIndex {
        bytecode_index: BytecodeIndex,
    },
    MissingInstruction {
        bytecode_index: BytecodeIndex,
    },
    MixedInstructionRepresentations,
    RawBytesRequireGeneratedDecoder,
    RawInstructionDecode {
        bytecode_index: BytecodeIndex,
        error: RawInstructionDecodeError,
    },
    UnsupportedRawOpcode {
        bytecode_index: BytecodeIndex,
        opcode_id: u8,
    },
    /// A byte offset that cannot be packed into a `BytecodeIndex`
    /// (`BytecodeIndex.h:88-93` pack round-trip ASSERT).
    BytecodeOffsetOverflow {
        offset: usize,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InstructionLinker;

impl InstructionLinker {
    pub fn link_schema_stream(stream: &PackedInstructionStream) -> InstructionLinkOutput {
        let mut findings = Vec::new();
        let mut declarations = stream.declarations().to_vec();
        for (index, declaration) in declarations.iter_mut().enumerate() {
            declaration
                .bytecode_index
                .get_or_insert_with(|| BytecodeIndex::from_offset(index as u32));
            for operand in &mut declaration.operands {
                if let Operand::Label(label) = *operand {
                    match stream
                        .labels()
                        .get(usize::try_from(label.0).unwrap_or(usize::MAX))
                    {
                        Some(declaration) if declaration.reference == label => {
                            match declaration.binding {
                                LabelBinding::Bound(index) => {
                                    *operand = Operand::BytecodeIndex(index);
                                }
                                LabelBinding::OutOfLine(offset) => {
                                    findings.push(InstructionLinkFinding::OutOfLineLabel {
                                        label,
                                        offset,
                                    });
                                }
                                LabelBinding::Unbound => {
                                    findings.push(InstructionLinkFinding::UnboundLabel { label });
                                }
                            }
                        }
                        _ => findings.push(InstructionLinkFinding::UnknownLabel { label }),
                    }
                }
            }
        }

        let mut typed_placeholder = stream.typed_placeholder().to_vec();
        for instruction in &mut typed_placeholder {
            for operand in &mut instruction.operands {
                if let Operand::Label(label) = *operand {
                    match stream
                        .labels()
                        .get(usize::try_from(label.0).unwrap_or(usize::MAX))
                    {
                        Some(declaration) if declaration.reference == label => {
                            match declaration.binding {
                                LabelBinding::Bound(index) => {
                                    *operand = Operand::BytecodeIndex(index);
                                }
                                LabelBinding::OutOfLine(offset) => {
                                    findings.push(InstructionLinkFinding::OutOfLineLabel {
                                        label,
                                        offset,
                                    });
                                }
                                LabelBinding::Unbound => {
                                    findings.push(InstructionLinkFinding::UnboundLabel { label });
                                }
                            }
                        }
                        _ => findings.push(InstructionLinkFinding::UnknownLabel { label }),
                    }
                }
            }
        }

        let linked = if findings.is_empty() {
            Some(PackedInstructionStream {
                layout: stream.layout(),
                raw: PackedByteStorage::Unencoded,
                typed_placeholder,
                declarations,
                labels: stream.labels().to_vec(),
                checkpoints: stream.checkpoints().to_vec(),
                lifecycle: PackedInstructionLifecycle::Linked,
            })
        } else {
            None
        };

        InstructionLinkOutput { linked, findings }
    }
}

#[derive(Clone, Debug, Default)]
pub struct InstructionLinkOutput {
    pub linked: Option<PackedInstructionStream>,
    pub findings: Vec<InstructionLinkFinding>,
}

impl InstructionLinkOutput {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionLinkFinding {
    UnknownLabel { label: LabelRef },
    UnboundLabel { label: LabelRef },
    OutOfLineLabel { label: LabelRef, offset: i32 },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PackedInstructionLifecycle {
    #[default]
    Empty,
    SchemaStaged,
    Encoded,
    Linked,
    Detached,
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

        validate_stream_lifecycle(stream, &mut findings);
        validate_labels(stream.labels(), &mut findings);
        validate_checkpoints(stream.checkpoints(), &mut findings);
        validate_instruction_declarations(stream.declarations(), stream.labels(), &mut findings);
        validate_typed_instructions(stream.typed_placeholder(), stream.labels(), &mut findings);

        VerificationReport { findings }
    }
}

fn validate_stream_lifecycle(
    stream: &PackedInstructionStream,
    findings: &mut Vec<VerificationFinding>,
) {
    let has_schema_records =
        !stream.declarations().is_empty() || !stream.typed_placeholder().is_empty();
    if stream.lifecycle() == PackedInstructionLifecycle::Empty && has_schema_records {
        findings.push(VerificationFinding::LifecycleStorageMismatch {
            lifecycle: stream.lifecycle(),
            storage: stream.raw().clone(),
        });
    }
    if matches!(
        stream.lifecycle(),
        PackedInstructionLifecycle::Encoded | PackedInstructionLifecycle::Linked
    ) && matches!(stream.raw(), PackedByteStorage::Unencoded)
    {
        findings.push(VerificationFinding::LifecycleStorageMismatch {
            lifecycle: stream.lifecycle(),
            storage: stream.raw().clone(),
        });
    }
    if !stream.declarations().is_empty() && !stream.typed_placeholder().is_empty() {
        findings.push(VerificationFinding::MixedInstructionRepresentations);
    }
}

fn validate_labels(labels: &[LabelDeclaration], findings: &mut Vec<VerificationFinding>) {
    for (index, label) in labels.iter().enumerate() {
        if usize::try_from(label.reference.0).ok() != Some(index) {
            findings.push(VerificationFinding::LabelReferenceMismatch {
                expected: LabelRef(index as u32),
                actual: label.reference,
            });
        }
        if labels[..index]
            .iter()
            .any(|candidate| candidate.reference == label.reference)
        {
            findings.push(VerificationFinding::DuplicateLabel {
                label: label.reference,
            });
        }
        if let LabelBinding::Bound(index) = label.binding {
            validate_bytecode_index(index, findings);
        }
    }
}

fn validate_checkpoints(checkpoints: &[CheckpointSpec], findings: &mut Vec<VerificationFinding>) {
    for checkpoint in checkpoints {
        validate_bytecode_index(checkpoint.bytecode_index, findings);
        validate_checkpoint(checkpoint.checkpoint, findings);
        if checkpoint.bytecode_index.checkpoint() != checkpoint.checkpoint {
            findings.push(VerificationFinding::CheckpointIndexMismatch {
                bytecode_index: checkpoint.bytecode_index,
                checkpoint: checkpoint.checkpoint,
            });
        }
    }
}

fn validate_instruction_declarations(
    declarations: &[InstructionDeclaration],
    labels: &[LabelDeclaration],
    findings: &mut Vec<VerificationFinding>,
) {
    for declaration in declarations {
        if let Some(index) = declaration.bytecode_index {
            validate_bytecode_index(index, findings);
        }
        for operand in &declaration.operands {
            validate_operand_boundary(declaration.opcode, operand, labels, findings);
        }
    }
}

fn validate_typed_instructions(
    instructions: &[TypedInstruction],
    labels: &[LabelDeclaration],
    findings: &mut Vec<VerificationFinding>,
) {
    for instruction in instructions {
        if let Some(index) = instruction.bytecode_index {
            validate_bytecode_index(index, findings);
        }
        if let Some(schema) = instruction.schema {
            if schema.opcode != instruction.opcode {
                findings.push(VerificationFinding::InstructionSchemaOpcodeMismatch {
                    instruction: instruction.opcode,
                    schema: schema.opcode,
                });
            }
            if usize::from(schema.operand_count) != instruction.operands.len() {
                findings.push(VerificationFinding::OperandCountMismatch {
                    opcode: instruction.opcode,
                    expected: usize::from(schema.operand_count),
                    actual: instruction.operands.len(),
                });
            }
        }
        for operand in &instruction.operands {
            validate_operand_boundary(instruction.opcode, operand, labels, findings);
        }
    }
}

fn validate_static_declarations(
    table: BytecodeDeclarationTable,
    findings: &mut Vec<VerificationFinding>,
) {
    validate_static_labels(table.labels(), findings);
    for declaration in table.instructions() {
        if let Some(index) = declaration.bytecode_index {
            validate_bytecode_index(index, findings);
        }
        if let Some(schema) = declaration.schema {
            if schema.opcode != declaration.opcode {
                findings.push(VerificationFinding::InstructionSchemaOpcodeMismatch {
                    instruction: declaration.opcode,
                    schema: schema.opcode,
                });
            }
            if usize::from(schema.operand_count) != declaration.operands.len() {
                findings.push(VerificationFinding::OperandCountMismatch {
                    opcode: declaration.opcode,
                    expected: usize::from(schema.operand_count),
                    actual: declaration.operands.len(),
                });
            }
        }
    }
    for checkpoint in table.checkpoints() {
        validate_bytecode_index(checkpoint.bytecode_index, findings);
        validate_checkpoint(checkpoint.checkpoint, findings);
    }
}

fn validate_static_labels(
    labels: &[StaticLabelDeclaration],
    findings: &mut Vec<VerificationFinding>,
) {
    for (index, label) in labels.iter().enumerate() {
        if labels[..index]
            .iter()
            .any(|candidate| candidate.reference == label.reference)
        {
            findings.push(VerificationFinding::DuplicateLabel {
                label: label.reference,
            });
        }
        if let LabelBinding::Bound(index) = label.binding {
            validate_bytecode_index(index, findings);
        }
    }
}

fn validate_operand_boundary(
    opcode: Opcode,
    operand: &Operand,
    labels: &[LabelDeclaration],
    findings: &mut Vec<VerificationFinding>,
) {
    match operand {
        Operand::Register(register) if !register.is_valid() => {
            findings.push(VerificationFinding::InvalidRegisterOperand { opcode });
        }
        Operand::EncodedRegister(encoded) if !encoded.register.is_valid() => {
            findings.push(VerificationFinding::InvalidRegisterOperand { opcode });
        }
        Operand::BytecodeIndex(index) => validate_bytecode_index(*index, findings),
        Operand::Label(label) => match labels.get(usize::try_from(label.0).unwrap_or(usize::MAX)) {
            Some(declaration) if declaration.reference == *label => {
                if declaration.binding == LabelBinding::Unbound {
                    findings.push(VerificationFinding::UnboundLabel { label: *label });
                }
            }
            _ => findings.push(VerificationFinding::UnknownLabel { label: *label }),
        },
        Operand::Checkpoint(checkpoint) => validate_checkpoint(*checkpoint, findings),
        _ => {}
    }
}

fn validate_bytecode_index(index: BytecodeIndex, findings: &mut Vec<VerificationFinding>) {
    if !index.is_valid() {
        findings.push(VerificationFinding::InvalidBytecodeIndex { index });
    }
    validate_checkpoint(index.checkpoint(), findings);
}

fn validate_checkpoint(checkpoint: Checkpoint, findings: &mut Vec<VerificationFinding>) {
    if checkpoint.0 >= BYTECODE_INDEX_CHECKPOINTS {
        findings.push(VerificationFinding::InvalidCheckpoint { checkpoint });
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
    OperandKindRecognized {
        opcode: Opcode,
        kind: OperandKind,
    },
    LifecycleStorageMismatch {
        lifecycle: PackedInstructionLifecycle,
        storage: PackedByteStorage,
    },
    MixedInstructionRepresentations,
    DuplicateLabel {
        label: LabelRef,
    },
    LabelReferenceMismatch {
        expected: LabelRef,
        actual: LabelRef,
    },
    UnknownLabel {
        label: LabelRef,
    },
    UnboundLabel {
        label: LabelRef,
    },
    InvalidBytecodeIndex {
        index: BytecodeIndex,
    },
    InvalidCheckpoint {
        checkpoint: Checkpoint,
    },
    CheckpointIndexMismatch {
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
    },
    InvalidRegisterOperand {
        opcode: Opcode,
    },
    InstructionSchemaOpcodeMismatch {
        instruction: Opcode,
        schema: Opcode,
    },
    OperandCountMismatch {
        opcode: Opcode,
        expected: usize,
        actual: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationResult {
    DeferredClean,
    DeferredWithFindings,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_accepts_bound_label_stream() {
        let mut builder = InstructionBuilder::new();
        let label = builder.declare_label(Some("target"));
        let index = BytecodeIndex::from_offset(1);
        assert!(builder.bind_label(label, index));
        builder.declare_instruction(
            Opcode::Reserved,
            OperandWidth::Narrow,
            vec![Operand::Label(label)],
        );
        let stream = builder.finalize();

        assert!(BytecodeVerifier::new(OpcodeSchemaVersion::default())
            .verify_schema_only(&stream)
            .findings
            .is_empty());
    }

    #[test]
    fn verifier_reports_unbound_and_invalid_operands() {
        let mut builder = InstructionBuilder::new();
        let label = builder.declare_label(Some("target"));
        builder.declare_instruction(
            Opcode::Reserved,
            OperandWidth::Narrow,
            vec![
                Operand::Label(label),
                Operand::BytecodeIndex(BytecodeIndex::INVALID),
                Operand::Register(VirtualRegister::INVALID),
            ],
        );
        let stream = builder.finalize();

        let findings = BytecodeVerifier::new(OpcodeSchemaVersion::default())
            .verify_schema_only(&stream)
            .findings;
        assert!(findings.contains(&VerificationFinding::UnboundLabel { label }));
        assert!(
            findings.contains(&VerificationFinding::InvalidBytecodeIndex {
                index: BytecodeIndex::INVALID,
            })
        );
        assert!(
            findings.contains(&VerificationFinding::InvalidRegisterOperand {
                opcode: Opcode::Reserved,
            })
        );
    }

    #[test]
    fn instruction_linker_rewrites_bound_label_operands() {
        let mut builder = InstructionBuilder::new();
        let label = builder.declare_label(Some("target"));
        let target = BytecodeIndex::from_offset(9);
        assert!(builder.bind_label(label, target));
        builder.declare_instruction(
            Opcode::Reserved,
            OperandWidth::Narrow,
            vec![Operand::Label(label)],
        );
        let stream = builder.finalize();

        let output = InstructionLinker::link_schema_stream(&stream);
        let linked = output.linked.as_ref().expect("linked stream");

        assert!(output.is_valid());
        assert_eq!(
            linked.declarations()[0].operands,
            vec![Operand::BytecodeIndex(target)]
        );
        assert_eq!(linked.lifecycle(), PackedInstructionLifecycle::Linked);
    }

    #[test]
    fn instruction_linker_reports_unresolved_labels() {
        let mut builder = InstructionBuilder::new();
        let label = builder.declare_label(Some("target"));
        builder.declare_instruction(
            Opcode::Reserved,
            OperandWidth::Narrow,
            vec![Operand::Label(label)],
        );
        let stream = builder.finalize();

        let output = InstructionLinker::link_schema_stream(&stream);

        assert_eq!(
            output.findings,
            vec![InstructionLinkFinding::UnboundLabel { label }]
        );
        assert!(output.linked.is_none());
    }

    #[test]
    fn decoded_instruction_exposes_typed_operands() {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(
            Opcode::Reserved,
            OperandWidth::Narrow,
            vec![
                Operand::Register(VirtualRegister::local(0)),
                Operand::SignedImmediate(-7),
                Operand::MetadataIndex(3),
            ],
        );
        let stream = builder.finalize();

        let decoded = stream
            .decoded_at(BytecodeIndex::from_offset(0))
            .expect("decoded instruction");

        assert_eq!(decoded.source, DecodedInstructionSource::Declaration);
        assert_eq!(decoded.register_operand(0), Ok(VirtualRegister::local(0)));
        assert_eq!(decoded.signed_immediate_operand(1), Ok(-7));
        assert_eq!(decoded.metadata_index_operand(2), Ok(3));
        assert_eq!(
            decoded.unsigned_immediate_operand(1),
            Err(OperandAccessError::UnexpectedOperandKind {
                opcode: Opcode::Reserved,
                index: 1,
                expected: OperandKind::UnsignedImmediate,
                actual: OperandKind::SignedImmediate,
            })
        );
    }

    #[test]
    fn decoded_instruction_iterator_reports_missing_instruction() {
        let stream = PackedInstructionStream::default();

        assert_eq!(
            stream.decoded_at(BytecodeIndex::from_offset(0)),
            Err(InstructionDecodeError::MissingInstruction {
                bytecode_index: BytecodeIndex::from_offset(0),
            })
        );
        assert_eq!(stream.decoded_instructions().count(), 0);
    }

    /// JSC-derived byte FIXTURE: hand-encoded per the C++ layout
    /// (`Instruction.h:181-198` narrow form `[opcode][operands...]`, opcode
    /// always one byte per `Opcode.h:86-87`; `Fits<VirtualRegister, Narrow>`
    /// constant band start 16 per `Fits.h:118-156`/`BytecodeConventions.h:35`),
    /// so this proves JSC's encoding rather than the Rust writer's:
    ///   offset 0: mov local0, constant0 = [MOV, 0xff, 0x10]  (size 3)
    ///   offset 3: ret local0            = [RET, 0xff]        (size 2)
    #[test]
    fn raw_decoder_bridge_decodes_only_mov_and_ret() {
        let constant0 = VirtualRegister::constant(0);
        let local0 = VirtualRegister::local(0);
        let (mov_at, ret_at) = (0usize, 3usize);
        let stream = PackedInstructionStream::from_raw_packed_bytes(vec![
            raw_stream::opcode_id::MOV,
            0xff, // local(0) = -1, two's complement
            0x10, // constant(0) -> FirstConstantRegisterIndex8 (16) + 0
            raw_stream::opcode_id::RET,
            0xff, // local(0)
        ]);

        let mov = stream
            .decoded_raw_at(BytecodeIndex::from_offset(mov_at as u32))
            .expect("raw mov decodes");
        assert_eq!(mov.opcode, CoreOpcode::Move.opcode());
        assert_eq!(mov.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(
            mov.next_bytecode_index,
            Some(BytecodeIndex::from_offset(ret_at as u32))
        );
        assert_eq!(
            mov.operands,
            vec![Operand::Register(local0), Operand::Register(constant0)]
        );
        assert_eq!(mov.source, DecodedInstructionSource::RawPackedBytes);

        let ret = stream
            .decoded_raw_at(BytecodeIndex::from_offset(ret_at as u32))
            .expect("raw ret decodes");
        assert_eq!(ret.opcode, CoreOpcode::Return.opcode());
        assert_eq!(ret.next_bytecode_index, None);
        assert_eq!(ret.operands, vec![Operand::Register(local0)]);

        // op_add fixture: [ADD, dst, lhs, rhs, profileIndex, operandTypes]
        // (BytecodeList.rb:1276-1291) — decodes structurally but the wedge
        // refuses to execute it.
        let unsupported = PackedInstructionStream::from_raw_packed_bytes(vec![
            raw_stream::opcode_id::ADD,
            0xff,
            0xff,
            0xff,
            0x00,
            0x00,
        ]);
        assert_eq!(
            unsupported.decoded_raw_at(BytecodeIndex::from_offset(0)),
            Err(InstructionDecodeError::UnsupportedRawOpcode {
                bytecode_index: BytecodeIndex::from_offset(0),
                opcode_id: raw_stream::opcode_id::ADD,
            })
        );
    }

    /// CRITICAL: byte offsets that are not instruction STARTS must never
    /// decode. Offset 1 inside `mov local0, ret-shaped-operand` points at an
    /// operand byte whose value equals the RET opcode id; C++ can never observe
    /// such an offset (InstructionStream iteration only advances by size(),
    /// InstructionStream.h:154-161), so the safe-Rust surface must reject it
    /// rather than conjure a bogus RET from inside the MOV's operands.
    #[test]
    fn raw_decoder_rejects_mid_instruction_offsets() {
        // mov local0, argument_or_header(8): src byte 0x08 == RET's opcode id.
        let bytes = vec![
            raw_stream::opcode_id::MOV,
            0xff,                       // local(0)
            raw_stream::opcode_id::RET, // register 8 (argument namespace), NOT an opcode
            raw_stream::opcode_id::RET,
            0xff,
        ];
        let stream = PackedInstructionStream::from_raw_packed_bytes(bytes);

        // The real instruction starts decode fine.
        assert!(stream.decoded_raw_at(BytecodeIndex::from_offset(0)).is_ok());
        assert!(stream.decoded_raw_at(BytecodeIndex::from_offset(3)).is_ok());

        // Every non-start offset (1, 2 inside mov; 4 inside ret; past the end)
        // is rejected as an invalid bytecode index, never decoded.
        for offset in [1u32, 2, 4, 5, 6] {
            let bytecode_index = BytecodeIndex::from_offset(offset);
            assert_eq!(
                stream.decoded_raw_at(bytecode_index),
                Err(InstructionDecodeError::InvalidBytecodeIndex { bytecode_index }),
                "offset {offset}"
            );
        }
    }

    /// Raw packed streams have no ordinal domain: the ordinal iterator reports
    /// the same `RawBytesRequireGeneratedDecoder` rejection as `decoded_at`,
    /// exactly once.
    #[test]
    fn ordinal_iterator_rejects_raw_packed_streams_once() {
        let stream = PackedInstructionStream::from_raw_packed_bytes(vec![
            raw_stream::opcode_id::MOV,
            0xff,
            0x10,
            raw_stream::opcode_id::RET,
            0xff,
        ]);
        assert_eq!(stream.instruction_count(), 2);
        let decoded: Vec<_> = stream.decoded_instructions().collect();
        assert_eq!(
            decoded,
            vec![Err(InstructionDecodeError::RawBytesRequireGeneratedDecoder)]
        );
    }
}
