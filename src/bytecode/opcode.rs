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

/// Executable core opcodes used by the Rust interpreter before generated JSC
/// bytecode tables are available.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CoreOpcode {
    LoadEmpty,
    LoadUndefined,
    LoadNull,
    LoadBool,
    LoadInt32,
    LoadDouble,
    LoadString,
    LoadBigInt,
    LoadFunction,
    LoadCallee,
    LoadObjectConstructor,
    LoadArrayConstructor,
    LoadFunctionConstructor,
    LoadMathObject,
    LoadJsonObject,
    LoadReflectObject,
    LoadArrayBufferConstructor,
    LoadUint8ArrayConstructor,
    LoadInt8ArrayConstructor,
    LoadUint8ClampedArrayConstructor,
    LoadInt16ArrayConstructor,
    LoadUint16ArrayConstructor,
    LoadInt32ArrayConstructor,
    LoadUint32ArrayConstructor,
    LoadFloat32ArrayConstructor,
    LoadFloat64ArrayConstructor,
    LoadDataViewConstructor,
    LoadProxyConstructor,
    LoadStringConstructor,
    LoadNumberConstructor,
    LoadBooleanConstructor,
    LoadErrorConstructor,
    LoadTypeErrorConstructor,
    LoadReferenceErrorConstructor,
    LoadMapConstructor,
    LoadSetConstructor,
    LoadWeakMapConstructor,
    LoadWeakSetConstructor,
    LoadRegExpConstructor,
    LoadPromiseConstructor,
    LoadDateConstructor,
    LoadBigIntConstructor,
    LoadSymbolConstructor,
    LoadCapture,
    Move,
    ToNumber,
    ToString,
    NegateNumber,
    BitNotInt32,
    LogicalNot,
    TypeOf,
    Void,
    AddInt32,
    SubInt32,
    MulInt32,
    DivNumber,
    ModNumber,
    PowNumber,
    BitOrInt32,
    BitXorInt32,
    BitAndInt32,
    LeftShiftInt32,
    RightShiftInt32,
    UnsignedRightShiftInt32,
    LessThanInt32,
    LessEqualInt32,
    GreaterThanInt32,
    GreaterEqualInt32,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    InstanceOf,
    InById,
    InByVal,
    LoopHint,
    Jump,
    JumpIfFalse,
    JumpIfNotNullish,
    NewObject,
    NewArray,
    NewRegExp,
    SetPrototype,
    SetFunctionSuper,
    SetDefaultDerivedConstructor,
    AddInstanceField,
    AddInstanceFieldByValue,
    ArrayAppend,
    ArrayAppendSpread,
    ArrayAppendRest,
    ArrayLength,
    ForInKeys,
    CopyObjectRest,
    CreateRestParameter,
    CreateArgumentsObject,
    DefineGetter,
    DefineSetter,
    DefineGetterByValue,
    DefineSetterByValue,
    GetSuperByName,
    ConstructSuper,
    NewClosureCell,
    GetClosureCell,
    PutClosureCell,
    GetGlobalLexical,
    InitializeGlobalLexical,
    PutGlobalLexical,
    GetGlobalObjectProperty,
    ResolveGlobalObjectProperty,
    PutGlobalObjectProperty,
    GetByName,
    PutByName,
    DeleteByName,
    GetByValue,
    PutByValue,
    DeleteByValue,
    GetByIndex,
    PutByIndex,
    GetLength,
    Call,
    CallWithThis,
    Construct,
    CallDirect,
    Throw,
    TakeException,
    EnsureThis,
    ReturnDerived,
    Return,
}

impl CoreOpcode {
    pub const fn opcode(self) -> Opcode {
        Opcode::RuntimeExtension(OpcodeId::from_generated_index(self.id()))
    }

    /// Resolve a packed opcode ID (JSC's real generated value) to its
    /// executable `CoreOpcode`.
    ///
    /// Delegates to the ONE canonical opcode table
    /// (`instruction_stream::OPCODE_TABLE`, the `BytecodeList.rb` mirror) via
    /// its `core` bridge field, so there is no second id->opcode mapping that
    /// could drift from the table. `None` = the wedge does not execute this raw
    /// opcode (everything except mov/ret).
    pub fn from_packed_opcode_id(opcode_id: u8) -> Option<Self> {
        crate::bytecode::instruction_stream::descriptor_for(opcode_id)
            .and_then(|descriptor| descriptor.core)
    }

    pub const fn from_opcode(opcode: Opcode) -> Option<Self> {
        let Opcode::RuntimeExtension(id) = opcode else {
            return None;
        };
        match id.generated_index() {
            108 => Some(Self::LoadEmpty),
            1 => Some(Self::LoadUndefined),
            2 => Some(Self::LoadNull),
            3 => Some(Self::LoadBool),
            4 => Some(Self::LoadInt32),
            32 => Some(Self::LoadDouble),
            22 => Some(Self::LoadString),
            98 => Some(Self::LoadBigInt),
            27 => Some(Self::LoadFunction),
            113 => Some(Self::LoadCallee),
            69 => Some(Self::LoadObjectConstructor),
            70 => Some(Self::LoadArrayConstructor),
            119 => Some(Self::LoadFunctionConstructor),
            71 => Some(Self::LoadMathObject),
            72 => Some(Self::LoadJsonObject),
            93 => Some(Self::LoadReflectObject),
            94 => Some(Self::LoadArrayBufferConstructor),
            95 => Some(Self::LoadUint8ArrayConstructor),
            121 => Some(Self::LoadInt8ArrayConstructor),
            122 => Some(Self::LoadUint8ClampedArrayConstructor),
            123 => Some(Self::LoadInt16ArrayConstructor),
            124 => Some(Self::LoadUint16ArrayConstructor),
            125 => Some(Self::LoadInt32ArrayConstructor),
            126 => Some(Self::LoadUint32ArrayConstructor),
            127 => Some(Self::LoadFloat32ArrayConstructor),
            128 => Some(Self::LoadFloat64ArrayConstructor),
            96 => Some(Self::LoadDataViewConstructor),
            99 => Some(Self::LoadProxyConstructor),
            73 => Some(Self::LoadStringConstructor),
            74 => Some(Self::LoadNumberConstructor),
            75 => Some(Self::LoadBooleanConstructor),
            76 => Some(Self::LoadErrorConstructor),
            77 => Some(Self::LoadTypeErrorConstructor),
            114 => Some(Self::LoadReferenceErrorConstructor),
            78 => Some(Self::LoadMapConstructor),
            79 => Some(Self::LoadSetConstructor),
            100 => Some(Self::LoadWeakMapConstructor),
            101 => Some(Self::LoadWeakSetConstructor),
            88 => Some(Self::LoadRegExpConstructor),
            90 => Some(Self::LoadPromiseConstructor),
            91 => Some(Self::LoadDateConstructor),
            97 => Some(Self::LoadBigIntConstructor),
            92 => Some(Self::LoadSymbolConstructor),
            85 => Some(Self::LoadCapture),
            5 => Some(Self::Move),
            33 => Some(Self::ToNumber),
            107 => Some(Self::ToString),
            34 => Some(Self::NegateNumber),
            43 => Some(Self::BitNotInt32),
            35 => Some(Self::LogicalNot),
            36 => Some(Self::TypeOf),
            37 => Some(Self::Void),
            6 => Some(Self::AddInt32),
            7 => Some(Self::SubInt32),
            8 => Some(Self::MulInt32),
            44 => Some(Self::DivNumber),
            45 => Some(Self::ModNumber),
            46 => Some(Self::PowNumber),
            47 => Some(Self::BitOrInt32),
            48 => Some(Self::BitXorInt32),
            49 => Some(Self::BitAndInt32),
            50 => Some(Self::LeftShiftInt32),
            51 => Some(Self::RightShiftInt32),
            52 => Some(Self::UnsignedRightShiftInt32),
            9 => Some(Self::LessThanInt32),
            10 => Some(Self::LessEqualInt32),
            11 => Some(Self::GreaterThanInt32),
            12 => Some(Self::GreaterEqualInt32),
            102 => Some(Self::Equal),
            103 => Some(Self::NotEqual),
            13 => Some(Self::StrictEqual),
            14 => Some(Self::StrictNotEqual),
            53 => Some(Self::InstanceOf),
            116 => Some(Self::InById),
            117 => Some(Self::InByVal),
            120 => Some(Self::LoopHint),
            15 => Some(Self::Jump),
            16 => Some(Self::JumpIfFalse),
            38 => Some(Self::JumpIfNotNullish),
            17 => Some(Self::NewObject),
            23 => Some(Self::NewArray),
            89 => Some(Self::NewRegExp),
            54 => Some(Self::SetPrototype),
            55 => Some(Self::SetFunctionSuper),
            58 => Some(Self::SetDefaultDerivedConstructor),
            59 => Some(Self::AddInstanceField),
            64 => Some(Self::AddInstanceFieldByValue),
            80 => Some(Self::ArrayAppend),
            81 => Some(Self::ArrayAppendSpread),
            83 => Some(Self::ArrayAppendRest),
            82 => Some(Self::ArrayLength),
            118 => Some(Self::ForInKeys),
            84 => Some(Self::CopyObjectRest),
            86 => Some(Self::CreateRestParameter),
            87 => Some(Self::CreateArgumentsObject),
            60 => Some(Self::DefineGetter),
            61 => Some(Self::DefineSetter),
            65 => Some(Self::DefineGetterByValue),
            66 => Some(Self::DefineSetterByValue),
            56 => Some(Self::GetSuperByName),
            57 => Some(Self::ConstructSuper),
            29 => Some(Self::NewClosureCell),
            30 => Some(Self::GetClosureCell),
            31 => Some(Self::PutClosureCell),
            104 => Some(Self::GetGlobalLexical),
            105 => Some(Self::InitializeGlobalLexical),
            106 => Some(Self::PutGlobalLexical),
            111 => Some(Self::GetGlobalObjectProperty),
            115 => Some(Self::ResolveGlobalObjectProperty),
            112 => Some(Self::PutGlobalObjectProperty),
            18 => Some(Self::GetByName),
            19 => Some(Self::PutByName),
            67 => Some(Self::DeleteByName),
            62 => Some(Self::GetByValue),
            63 => Some(Self::PutByValue),
            68 => Some(Self::DeleteByValue),
            24 => Some(Self::GetByIndex),
            25 => Some(Self::PutByIndex),
            26 => Some(Self::GetLength),
            28 => Some(Self::Call),
            39 => Some(Self::CallWithThis),
            40 => Some(Self::Construct),
            20 => Some(Self::CallDirect),
            41 => Some(Self::Throw),
            42 => Some(Self::TakeException),
            109 => Some(Self::EnsureThis),
            110 => Some(Self::ReturnDerived),
            21 => Some(Self::Return),
            _ => None,
        }
    }

    const fn id(self) -> u16 {
        match self {
            Self::LoadEmpty => 108,
            Self::LoadUndefined => 1,
            Self::LoadNull => 2,
            Self::LoadBool => 3,
            Self::LoadInt32 => 4,
            Self::LoadDouble => 32,
            Self::LoadString => 22,
            Self::LoadBigInt => 98,
            Self::LoadFunction => 27,
            Self::LoadCallee => 113,
            Self::LoadObjectConstructor => 69,
            Self::LoadArrayConstructor => 70,
            Self::LoadFunctionConstructor => 119,
            Self::LoadMathObject => 71,
            Self::LoadJsonObject => 72,
            Self::LoadReflectObject => 93,
            Self::LoadArrayBufferConstructor => 94,
            Self::LoadUint8ArrayConstructor => 95,
            Self::LoadInt8ArrayConstructor => 121,
            Self::LoadUint8ClampedArrayConstructor => 122,
            Self::LoadInt16ArrayConstructor => 123,
            Self::LoadUint16ArrayConstructor => 124,
            Self::LoadInt32ArrayConstructor => 125,
            Self::LoadUint32ArrayConstructor => 126,
            Self::LoadFloat32ArrayConstructor => 127,
            Self::LoadFloat64ArrayConstructor => 128,
            Self::LoadDataViewConstructor => 96,
            Self::LoadProxyConstructor => 99,
            Self::LoadStringConstructor => 73,
            Self::LoadNumberConstructor => 74,
            Self::LoadBooleanConstructor => 75,
            Self::LoadErrorConstructor => 76,
            Self::LoadTypeErrorConstructor => 77,
            Self::LoadReferenceErrorConstructor => 114,
            Self::LoadMapConstructor => 78,
            Self::LoadSetConstructor => 79,
            Self::LoadWeakMapConstructor => 100,
            Self::LoadWeakSetConstructor => 101,
            Self::LoadRegExpConstructor => 88,
            Self::LoadPromiseConstructor => 90,
            Self::LoadDateConstructor => 91,
            Self::LoadBigIntConstructor => 97,
            Self::LoadSymbolConstructor => 92,
            Self::LoadCapture => 85,
            Self::Move => 5,
            Self::ToNumber => 33,
            Self::ToString => 107,
            Self::NegateNumber => 34,
            Self::BitNotInt32 => 43,
            Self::LogicalNot => 35,
            Self::TypeOf => 36,
            Self::Void => 37,
            Self::AddInt32 => 6,
            Self::SubInt32 => 7,
            Self::MulInt32 => 8,
            Self::DivNumber => 44,
            Self::ModNumber => 45,
            Self::PowNumber => 46,
            Self::BitOrInt32 => 47,
            Self::BitXorInt32 => 48,
            Self::BitAndInt32 => 49,
            Self::LeftShiftInt32 => 50,
            Self::RightShiftInt32 => 51,
            Self::UnsignedRightShiftInt32 => 52,
            Self::LessThanInt32 => 9,
            Self::LessEqualInt32 => 10,
            Self::GreaterThanInt32 => 11,
            Self::GreaterEqualInt32 => 12,
            Self::Equal => 102,
            Self::NotEqual => 103,
            Self::StrictEqual => 13,
            Self::StrictNotEqual => 14,
            Self::InstanceOf => 53,
            Self::InById => 116,
            Self::InByVal => 117,
            Self::LoopHint => 120,
            Self::Jump => 15,
            Self::JumpIfFalse => 16,
            Self::JumpIfNotNullish => 38,
            Self::NewObject => 17,
            Self::NewArray => 23,
            Self::NewRegExp => 89,
            Self::SetPrototype => 54,
            Self::SetFunctionSuper => 55,
            Self::SetDefaultDerivedConstructor => 58,
            Self::AddInstanceField => 59,
            Self::AddInstanceFieldByValue => 64,
            Self::ArrayAppend => 80,
            Self::ArrayAppendSpread => 81,
            Self::ArrayAppendRest => 83,
            Self::ArrayLength => 82,
            Self::ForInKeys => 118,
            Self::CopyObjectRest => 84,
            Self::CreateRestParameter => 86,
            Self::CreateArgumentsObject => 87,
            Self::DefineGetter => 60,
            Self::DefineSetter => 61,
            Self::DefineGetterByValue => 65,
            Self::DefineSetterByValue => 66,
            Self::GetSuperByName => 56,
            Self::ConstructSuper => 57,
            Self::NewClosureCell => 29,
            Self::GetClosureCell => 30,
            Self::PutClosureCell => 31,
            Self::GetGlobalLexical => 104,
            Self::InitializeGlobalLexical => 105,
            Self::PutGlobalLexical => 106,
            Self::GetGlobalObjectProperty => 111,
            Self::ResolveGlobalObjectProperty => 115,
            Self::PutGlobalObjectProperty => 112,
            Self::GetByName => 18,
            Self::PutByName => 19,
            Self::DeleteByName => 67,
            Self::GetByValue => 62,
            Self::PutByValue => 63,
            Self::DeleteByValue => 68,
            Self::GetByIndex => 24,
            Self::PutByIndex => 25,
            Self::GetLength => 26,
            Self::Call => 28,
            Self::CallWithThis => 39,
            Self::Construct => 40,
            Self::CallDirect => 20,
            Self::Throw => 41,
            Self::TakeException => 42,
            Self::EnsureThis => 109,
            Self::ReturnDerived => 110,
            Self::Return => 21,
        }
    }
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

/// Component that owns an immutable opcode schema table.
///
/// Generated opcode data is authored by the bytecode generator from
/// `BytecodeList.rb`. Hand-written Rust may borrow these descriptors, but
/// mutation authority stays with the generator output that created the table.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum OpcodeSchemaOwner {
    #[default]
    BytecodeGenerator,
    BytecodeRuntime,
    TestFixture,
}

/// Authority allowed to replace a published opcode registry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum OpcodeRegistryMutationAuthority {
    #[default]
    GeneratedDataRefresh,
    CrateInitialization,
}

/// Provenance for a generated opcode schema snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct OpcodeSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: OpcodeSchemaVersion,
}

impl OpcodeSchemaProvenance {
    pub const fn new(
        generator: &'static str,
        source: &'static str,
        revision: OpcodeSchemaVersion,
    ) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Borrowed metadata field shape for a generated opcode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticMetadataShape {
    pub fields: &'static [MetadataFieldSpec],
}

impl StaticMetadataShape {
    pub const fn new(fields: &'static [MetadataFieldSpec]) -> Self {
        Self { fields }
    }

    pub const fn is_empty(self) -> bool {
        self.fields.is_empty()
    }
}

/// Borrowed checkpoint shape for a generated opcode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticCheckpointShape {
    pub checkpoints: &'static [CheckpointDescriptor],
}

impl StaticCheckpointShape {
    pub const fn new(checkpoints: &'static [CheckpointDescriptor]) -> Self {
        Self { checkpoints }
    }
}

/// Immutable generated schema contract for one opcode.
///
/// This is a borrowed counterpart to `OpcodeDescriptor`: it can live in static
/// generated tables and does not allocate. It still only describes bytecode and
/// performs no decoding, validation, dispatch, or execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticOpcodeDescriptor {
    pub opcode: Opcode,
    pub name: &'static str,
    pub category: OpcodeCategory,
    pub operands: &'static [OperandSpec],
    pub metadata: StaticMetadataShape,
    pub checkpoints: StaticCheckpointShape,
    pub temporaries: &'static [TemporarySpec],
    pub effects: OpcodeEffects,
}

impl StaticOpcodeDescriptor {
    pub const fn operand_count(self) -> usize {
        self.operands.len()
    }
}

/// Immutable opcode schema snapshot published to bytecode consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticOpcodeSchema {
    pub version: OpcodeSchemaVersion,
    pub descriptors: &'static [StaticOpcodeDescriptor],
    pub owner: OpcodeSchemaOwner,
    pub mutation_authority: OpcodeRegistryMutationAuthority,
    pub provenance: OpcodeSchemaProvenance,
}

impl StaticOpcodeSchema {
    pub const fn descriptors(self) -> &'static [StaticOpcodeDescriptor] {
        self.descriptors
    }

    pub fn descriptor_for_opcode(self, opcode: Opcode) -> Option<&'static StaticOpcodeDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.opcode == opcode)
    }

    pub fn descriptor_for_name(self, name: &str) -> Option<&'static StaticOpcodeDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn validate(self) -> OpcodeValidationReport {
        let mut findings = Vec::new();
        if self.provenance.revision != self.version {
            findings.push(OpcodeValidationFinding::ProvenanceVersionMismatch {
                schema: self.version,
                provenance: self.provenance.revision,
            });
        }
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            if descriptor.name.is_empty() {
                findings.push(OpcodeValidationFinding::EmptyOpcodeName {
                    opcode: descriptor.opcode,
                });
            }
            if self.descriptors[..index]
                .iter()
                .any(|candidate| candidate.opcode == descriptor.opcode)
            {
                findings.push(OpcodeValidationFinding::DuplicateOpcode {
                    opcode: descriptor.opcode,
                });
            }
            if self.descriptors[..index]
                .iter()
                .any(|candidate| candidate.name == descriptor.name)
            {
                findings.push(OpcodeValidationFinding::DuplicateOpcodeName {
                    name: descriptor.name,
                });
            }
            validate_descriptor_shape(*descriptor, &mut findings);
        }
        OpcodeValidationReport { findings }
    }
}

/// Process-level registry of immutable opcode schema snapshots.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpcodeSchemaRegistry {
    pub schemas: &'static [StaticOpcodeSchema],
}

impl OpcodeSchemaRegistry {
    pub const fn new(schemas: &'static [StaticOpcodeSchema]) -> Self {
        Self { schemas }
    }

    pub const fn schemas(self) -> &'static [StaticOpcodeSchema] {
        self.schemas
    }

    pub fn schema_for_version(
        self,
        version: OpcodeSchemaVersion,
    ) -> Option<&'static StaticOpcodeSchema> {
        self.schemas.iter().find(|schema| schema.version == version)
    }

    pub fn validate(self) -> OpcodeValidationReport {
        let mut findings = Vec::new();
        for (index, schema) in self.schemas.iter().enumerate() {
            if self.schemas[..index]
                .iter()
                .any(|candidate| candidate.version == schema.version)
            {
                findings.push(OpcodeValidationFinding::DuplicateSchemaVersion {
                    version: schema.version,
                });
            }
            findings.extend(schema.validate().findings);
        }
        OpcodeValidationReport { findings }
    }
}

fn validate_descriptor_shape(
    descriptor: StaticOpcodeDescriptor,
    findings: &mut Vec<OpcodeValidationFinding>,
) {
    for (index, operand) in descriptor.operands.iter().enumerate() {
        if operand.name.is_empty() {
            findings.push(OpcodeValidationFinding::EmptyOperandName {
                opcode: descriptor.opcode,
            });
        }
        if descriptor.operands[..index]
            .iter()
            .any(|candidate| candidate.name == operand.name)
        {
            findings.push(OpcodeValidationFinding::DuplicateOperandName {
                opcode: descriptor.opcode,
                name: operand.name,
            });
        }
    }
    for (index, field) in descriptor.metadata.fields.iter().enumerate() {
        if field.name.is_empty() {
            findings.push(OpcodeValidationFinding::EmptyMetadataFieldName {
                opcode: descriptor.opcode,
            });
        }
        if descriptor.metadata.fields[..index]
            .iter()
            .any(|candidate| candidate.name == field.name)
        {
            findings.push(OpcodeValidationFinding::DuplicateMetadataFieldName {
                opcode: descriptor.opcode,
                name: field.name,
            });
        }
    }
    for (index, checkpoint) in descriptor.checkpoints.checkpoints.iter().enumerate() {
        if checkpoint.name.is_empty() {
            findings.push(OpcodeValidationFinding::EmptyCheckpointName {
                opcode: descriptor.opcode,
            });
        }
        if descriptor.checkpoints.checkpoints[..index]
            .iter()
            .any(|candidate| candidate.ordinal == checkpoint.ordinal)
        {
            findings.push(OpcodeValidationFinding::DuplicateCheckpointOrdinal {
                opcode: descriptor.opcode,
                ordinal: checkpoint.ordinal,
            });
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpcodeValidationReport {
    pub findings: Vec<OpcodeValidationFinding>,
}

impl OpcodeValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpcodeValidationFinding {
    ProvenanceVersionMismatch {
        schema: OpcodeSchemaVersion,
        provenance: OpcodeSchemaVersion,
    },
    DuplicateSchemaVersion {
        version: OpcodeSchemaVersion,
    },
    EmptyOpcodeName {
        opcode: Opcode,
    },
    DuplicateOpcode {
        opcode: Opcode,
    },
    DuplicateOpcodeName {
        name: &'static str,
    },
    EmptyOperandName {
        opcode: Opcode,
    },
    DuplicateOperandName {
        opcode: Opcode,
        name: &'static str,
    },
    EmptyMetadataFieldName {
        opcode: Opcode,
    },
    DuplicateMetadataFieldName {
        opcode: Opcode,
        name: &'static str,
    },
    EmptyCheckpointName {
        opcode: Opcode,
    },
    DuplicateCheckpointOrdinal {
        opcode: Opcode,
        ordinal: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPERAND: OperandSpec = OperandSpec::required(
        "dst",
        OperandKind::VirtualRegister,
        OperandWidth::Narrow,
        OperandRole::Destination,
    );
    const DESCRIPTOR: StaticOpcodeDescriptor = StaticOpcodeDescriptor {
        opcode: Opcode::Reserved,
        name: "reserved",
        category: OpcodeCategory::Runtime,
        operands: &[OPERAND],
        metadata: StaticMetadataShape::new(&[]),
        checkpoints: StaticCheckpointShape::new(&[]),
        temporaries: &[],
        effects: OpcodeEffects {
            may_read_heap: false,
            may_write_heap: false,
            may_call: false,
            may_throw: false,
            is_branch: false,
            is_terminal: false,
        },
    };

    #[test]
    fn opcode_schema_validation_accepts_unique_descriptor() {
        let schema = StaticOpcodeSchema {
            version: OpcodeSchemaVersion(1),
            descriptors: &[DESCRIPTOR],
            owner: OpcodeSchemaOwner::TestFixture,
            mutation_authority: OpcodeRegistryMutationAuthority::GeneratedDataRefresh,
            provenance: OpcodeSchemaProvenance::new(
                "test",
                "BytecodeList.rb",
                OpcodeSchemaVersion(1),
            ),
        };

        assert!(schema.validate().is_valid());
    }

    #[test]
    fn opcode_schema_validation_reports_duplicates() {
        let schema = StaticOpcodeSchema {
            version: OpcodeSchemaVersion(1),
            descriptors: &[DESCRIPTOR, DESCRIPTOR],
            owner: OpcodeSchemaOwner::TestFixture,
            mutation_authority: OpcodeRegistryMutationAuthority::GeneratedDataRefresh,
            provenance: OpcodeSchemaProvenance::new(
                "test",
                "BytecodeList.rb",
                OpcodeSchemaVersion(2),
            ),
        };

        let findings = schema.validate().findings;
        assert!(
            findings.contains(&OpcodeValidationFinding::ProvenanceVersionMismatch {
                schema: OpcodeSchemaVersion(1),
                provenance: OpcodeSchemaVersion(2),
            })
        );
        assert!(
            findings.contains(&OpcodeValidationFinding::DuplicateOpcode {
                opcode: Opcode::Reserved,
            })
        );
        assert!(
            findings.contains(&OpcodeValidationFinding::DuplicateOpcodeName { name: "reserved" })
        );
    }
}
