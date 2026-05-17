/// ABI-visible opcode identity.
///
/// JSC's concrete opcode order is generated from `BytecodeList.rb` and feeds
/// opcode names, lengths, metadata tables, checkpoint counts, and LLInt/JIT
/// dispatch. The Rust skeleton keeps a small typed handle instead of hand
/// listing opcodes, so later generation can replace the backing table without
/// changing consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct OpcodeId(u16);

impl OpcodeId {
    pub const fn from_generated_index(index: u16) -> Self {
        Self(index)
    }

    pub const fn generated_index(self) -> u16 {
        self.0
    }
}

/// Opcode reference used by hand-written skeleton code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum Opcode {
    /// Placeholder used before generated opcode descriptors are available.
    Reserved,
    /// Width-prefix sentinel. Real decoding treats the next opcode as wide16.
    Wide16Prefix,
    /// Width-prefix sentinel. Real decoding treats the next opcode as wide32.
    Wide32Prefix,
    /// A generated JavaScript bytecode opcode.
    Generated(OpcodeId),
    /// LLInt helper or extension opcode outside the core bytecode section.
    RuntimeExtension(OpcodeId),
}

/// Generated schema contract for one opcode.
///
/// This is descriptive data only. It does not encode, decode, dispatch, or
/// validate an instruction stream. A future generator should produce a static
/// table of these descriptors from `BytecodeList.rb`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpcodeDescriptor {
    pub opcode: Opcode,
    pub name: &'static str,
    pub category: OpcodeCategory,
    pub operands: Vec<OperandSpec>,
    pub metadata: MetadataShape,
    pub checkpoints: CheckpointShape,
    pub temporaries: Vec<TemporarySpec>,
    pub effects: OpcodeEffects,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum OpcodeCategory {
    Prefix,
    Load,
    Store,
    Arithmetic,
    PropertyAccess,
    Call,
    Construct,
    ControlFlow,
    Scope,
    Exception,
    Debug,
    Profiling,
    Runtime,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct OpcodeEffects {
    pub may_read_heap: bool,
    pub may_write_heap: bool,
    pub may_call: bool,
    pub may_throw: bool,
    pub is_branch: bool,
    pub is_terminal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum OperandKind {
    VirtualRegister,
    OptionalVirtualRegister,
    SignedImmediate,
    UnsignedImmediate,
    ConstantPoolIndex,
    IdentifierIndex,
    FunctionDeclIndex,
    FunctionExprIndex,
    BytecodeIndex,
    BoundLabel,
    JumpTableIndex,
    MetadataIndex,
    InlineCacheIndex,
    ProfileIndex,
    RuntimeType,
    LinkTimeConstant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct OperandSpec {
    pub name: &'static str,
    pub kind: OperandKind,
    pub width: OperandWidth,
    pub role: OperandRole,
    pub presence: OperandPresence,
}

impl OperandSpec {
    pub const fn required(
        name: &'static str,
        kind: OperandKind,
        width: OperandWidth,
        role: OperandRole,
    ) -> Self {
        Self {
            name,
            kind,
            width,
            role,
            presence: OperandPresence::Required,
        }
    }

    pub const fn optional(
        name: &'static str,
        kind: OperandKind,
        width: OperandWidth,
        role: OperandRole,
    ) -> Self {
        Self {
            name,
            kind,
            width,
            role,
            presence: OperandPresence::Optional,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum OperandRole {
    Destination,
    Source,
    Immediate,
    Cache,
    Metadata,
    ControlFlowTarget,
    Profiling,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OperandPresence {
    Required,
    Optional,
}

/// Operand-width family selected by a narrow instruction or width prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OperandWidth {
    Narrow,
    Wide16,
    Wide32,
}

impl OperandWidth {
    pub const fn byte_width(self) -> u8 {
        match self {
            Self::Narrow => 1,
            Self::Wide16 => 2,
            Self::Wide32 => 4,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataShape {
    pub fields: Vec<MetadataFieldSpec>,
}

impl MetadataShape {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MetadataFieldSpec {
    pub name: &'static str,
    pub kind: MetadataFieldKind,
    pub mutability: MetadataMutability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum MetadataFieldKind {
    ValueProfile,
    MinimalValueProfile,
    ArgumentValueProfile,
    ArrayProfile,
    ArithProfile,
    ObjectAllocationProfile,
    CallLinkInfo,
    StructureStubInfo,
    GetByIdMode,
    PutByIdMode,
    IterationMode,
    ResolveMode,
    EnumeratorMode,
    ExceptionHandler,
    BasicBlockLocation,
    WriteBarrierCell,
    OpaqueGenerated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MetadataMutability {
    FrozenUnlinked,
    LinkedMutable,
    MainThreadOnly,
    ConcurrentWithLock,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckpointShape {
    pub checkpoints: Vec<CheckpointDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CheckpointDescriptor {
    pub ordinal: u8,
    pub name: &'static str,
    pub records_value_profile: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TemporarySpec {
    pub name: &'static str,
    pub kind: TemporaryKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum TemporaryKind {
    JsValue,
    UInt32,
    Pointer,
    OpaqueGenerated,
}

/// Whole-schema identity used by caches and serialized bytecode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct OpcodeSchemaVersion(pub u64);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpcodeSchema {
    pub version: OpcodeSchemaVersion,
    pub descriptors: Vec<OpcodeDescriptor>,
}
