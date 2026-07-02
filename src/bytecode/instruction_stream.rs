//! Faithful packed bytecode stream core.
//!
//! C++ ground truth (inspect FIRST):
//!   - `bytecode/OpcodeSize.h`           width model (`enum OpcodeSize`).
//!   - `bytecode/Instruction.h`          `BaseInstruction` decode + `size()`.
//!   - `bytecode/InstructionStream.h`    byte-stream storage, writer, `Ref`/cursor.
//!   - `bytecode/BytecodeIndex.h`        byte-offset index semantics.
//!   - `bytecode/Fits.h` / `generator/*` operand width selection + layout.
//!   - `bytecode/BytecodeList.rb`        opcode descriptors + `opcodeLengths`.
//!
//! This module is the faithful replacement-in-waiting for the DIVERGENT typed
//! representation in `instruction.rs` (`TypedInstruction` +
//! `PackedInstructionStream` whose `typed_placeholder` walks a `Vec` by ORDINAL
//! index) and `opcode.rs` (the type-specialized `CoreOpcode`: `AddInt32`/etc.).
//!
//! THE DIVERGENCE THIS CORRECTS: JSC bytecode is a FLAT PACKED BYTE STREAM
//! (`InstructionStream.h:51` `InstructionBuffer = Vector<uint8_t>`;
//! `Instruction.h:202` `sizeof(JSInstruction) == 1`) consumed identically by
//! LLInt/Baseline/DFG/FTL. The program counter advances by instruction SIZE
//! (`InstructionStream.h:87-90` `next() = index + ptr()->size()`) and
//! `BytecodeIndex` is a BYTE OFFSET (`BytecodeIndex.h:48-90`), NOT an ordinal.
//! The optimizing JIT cannot lower from a typed-`Vec`-by-ordinal; it needs this
//! packed stream. Landed ADDITIVE and UNWIRED behind `#![allow(dead_code)]`;
//! the cutover of the live interpreter/dispatch is a SERIAL step the
//! orchestrator owns (see the module-level serial-coupling notes below).
//!
//! Serial couplings flagged for the orchestrator (NOT decided here):
//!   1. Cut the live interpreter dispatch (`instruction.rs`/`opcode.rs`/LLInt)
//!      over to this packed stream; freeze the type-specialized `CoreOpcode`.
//!   2. The metadata table (`UnlinkedMetadataTable`/`MetadataTable`): opcodes
//!      with metadata add ONE operand slot to `opcodeLengths` (the metadataID).
//!      The representative subset has no metadata; wiring it is shared work.
#![allow(dead_code)]

use crate::bytecode::code_block::BytecodeIndex;
use crate::bytecode::opcode::CoreOpcode;
use crate::bytecode::register::{
    VirtualRegister, FIRST_CONSTANT_REGISTER_INDEX16, FIRST_CONSTANT_REGISTER_INDEX8,
};

/// Opcode-ID width in bytes. For JS bytecode the opcode is ALWAYS one byte,
/// even inside wide16/wide32 instructions: `Opcode.h:86-87`
/// (`static_assert(NUMBER_OF_BYTECODE_IDS < 255)` and
/// `maxJSOpcodeIDWidth = OpcodeSize::Narrow`). This matches the rejected-alt
/// `bytecode-wide-instruction-opcode-same-width` (move `24b088b7`): the opcode
/// is narrow in every form so wide instructions only widen operand fields.
pub const OPCODE_ID_BYTES: usize = 1;

/// Per-instruction operand width family.
///
/// Faithful to `enum OpcodeSize { Narrow = 1, Wide16 = 2, Wide32 = 4 }`
/// (`OpcodeSize.h:33-37`). One width is shared by EVERY operand field of an
/// instruction (`instruction-format.md` line 3); a single narrow opcode plus a
/// 1-byte prefix selects wide16/wide32.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum OpcodeSize {
    Narrow = 1,
    Wide16 = 2,
    Wide32 = 4,
}

impl OpcodeSize {
    /// Bytes per operand field. `1 << sizeShiftAmount` in `Instruction.h:142`.
    pub const fn operand_bytes(self) -> usize {
        self as usize
    }

    /// `sizeShiftAmount()` (`Instruction.h:115-122`): Narrow=0, Wide16=1,
    /// Wide32=2.
    pub const fn size_shift_amount(self) -> u32 {
        match self {
            Self::Narrow => 0,
            Self::Wide16 => 1,
            Self::Wide32 => 2,
        }
    }

    /// Width-prefix byte count. `Instruction.h:141` `prefixSize = sizeShiftAmount
    /// ? 1 : 0`, matching `PaddingBySize` (`OpcodeSize.h:63-76`): 0 for Narrow,
    /// 1 for Wide16/Wide32.
    pub const fn prefix_bytes(self) -> usize {
        match self {
            Self::Narrow => 0,
            Self::Wide16 | Self::Wide32 => 1,
        }
    }

    /// Inclusive signed minimum for this width, used by the `Fits<integral>`
    /// checks (`Fits.h:66-85`).
    const fn signed_min(self) -> i64 {
        -(1i64 << (8 * self.operand_bytes() - 1))
    }
    /// Inclusive signed maximum for this width (`Fits.h:66-85`).
    const fn signed_max(self) -> i64 {
        (1i64 << (8 * self.operand_bytes() - 1)) - 1
    }
    /// Inclusive unsigned maximum for this width (`Fits.h:66-85`).
    const fn unsigned_max(self) -> u64 {
        (1u64 << (8 * self.operand_bytes())) - 1
    }
}

/// Symbolic names for JSC's generated JS opcode IDs (`Opcode.h:66`
/// `FOR_EACH_OPCODE_ID(OPCODE_ID_ENUM)`), RE-DERIVED at compile time by name
/// from [`GENERATED_OPCODE_TABLE`] so the generator-produced table is the ONE
/// home of every numeric id — there is no second hand-pinned list to drift.
/// The test `opcode_ids_match_jsc_generated_values` still pins the expected
/// numeric values against the local build's `Bytecodes.h`.
///
/// ID-ASSIGNMENT RULE (owned by JSC's generator, which produced the table):
/// the `:Bytecode` section is declared `preserve_order: true`
/// (`BytecodeList.rb:79-87`), so `DSL.end_section` SKIPS `Section#sort!` and
/// only validates the required [checkpoint ops][metadata ops][plain ops]
/// declaration ordering (`generator/DSL.rb:43-56`,
/// `generator/Section.rb:72-97`); `create_ids!` then numbers every opcode
/// sequentially from 0 in EXACT declaration order
/// (`generator/Section.rb:99-101`; `generator/Opcode.rb:41-47,59-61` class
/// counter), with `op_group` members appended inline in listed order
/// (`generator/Section.rb:50-54`). An opcode's ID is therefore fixed by its
/// position in `BytecodeList.rb` ALONE.
pub mod opcode_id {
    /// Compile-time by-name lookup in the generated table. C++ needs no such
    /// helper — the generator emits the `OpcodeID` enum directly; this is the
    /// same "name -> generated id" binding expressed over the emitted table.
    /// Fails the build if the name is not a generated opcode.
    const fn generated_id(name: &str) -> u8 {
        let needle = name.as_bytes();
        let mut i = 0;
        while i < super::GENERATED_OPCODE_TABLE.len() {
            if bytes_eq(super::GENERATED_OPCODE_TABLE[i].name.as_bytes(), needle) {
                return super::GENERATED_OPCODE_TABLE[i].id;
            }
            i += 1;
        }
        panic!("opcode name not present in GENERATED_OPCODE_TABLE")
    }

    /// Const-context `==` for byte strings (`str` equality is not yet const).
    const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// `op :jmp` (`BytecodeList.rb:933`).
    pub const JMP: u8 = generated_id("jmp");
    /// `op :jtrue` (`BytecodeList.rb:938`).
    pub const JTRUE: u8 = generated_id("jtrue");
    /// `op :ret` (`BytecodeList.rb:1040`).
    pub const RET: u8 = generated_id("ret");
    /// Width-prefix opcode `op :wide16` (`BytecodeList.rb:1174`):
    /// `narrow()->opcodeID() == Traits::wide16` selects the wide16 form
    /// (`Instruction.h:40,81-84`). NOT id 0: the wide prefixes sit LATE in
    /// declaration order, interleaved with the super-sampler ops
    /// (`op_nop`=126, `op_super_sampler_begin`=127, wide16=128,
    /// `op_super_sampler_end`=129, wide32=130).
    pub const WIDE16: u8 = generated_id("wide16");
    /// Width-prefix opcode `op :wide32` (`BytecodeList.rb:1178`);
    /// `Instruction.h:41,86-89`.
    pub const WIDE32: u8 = generated_id("wide32");
    /// `op :enter` (`BytecodeList.rb:1180`).
    pub const ENTER: u8 = generated_id("enter");
    /// `op :mov` (`BytecodeList.rb:1248-1252`).
    pub const MOV: u8 = generated_id("mov");
    /// First member `:eq` of `op_group :BinaryOp` (`BytecodeList.rb:1254-1274`).
    pub const EQ: u8 = generated_id("eq");
    /// `op_group :ProfiledBinaryOpWithOperandTypes` members in group order
    /// `[:add, :mul, :div, :sub, :bitand, :bitor, :bitxor]`
    /// (`BytecodeList.rb:1276-1292`): add=158, mul=159, div=160, sub=161.
    pub const ADD: u8 = generated_id("add");
    /// See [`ADD`]: second ProfiledBinaryOpWithOperandTypes member.
    pub const MUL: u8 = generated_id("mul");
    /// See [`ADD`]: fourth member; `div` (160) sits between `mul` and `sub`.
    pub const SUB: u8 = generated_id("sub");
}

/// Operand classes of the packed stream: ONE variant per C++ STREAM TYPE
/// appearing in a `BytecodeList.rb` `args:` block of the `:Bytecode` section,
/// named as the C++ type CamelCased (`unsigned` -> `UnsignedImmediate`,
/// `int` -> `SignedImmediate`) so a generated table can emit
/// `OperandKind::<Type>` mechanically from the declared arg type.
/// `metadata:` blocks are NOT stream operand types — a metadata opcode adds
/// exactly ONE trailing `unsigned` m_metadataID slot (see
/// [`OpcodeDescriptor::has_metadata`]).
///
/// Census of ALL `args:` blocks in the 193-opcode `:Bytecode` section
/// (`BytecodeList.rb:79-1395`), expanded per opcode and verified by
/// reproducing every generated `macro(op_*, length)` of the local build's
/// `WebKitBuild/Release/DerivedSources/JavaScriptCore/Bytecodes.h`:
/// VirtualRegister 423, unsigned 113, BoundLabel 23, ECMAMode 8, int 7,
/// OperandTypes 7, IndexingType 2, SymbolTableOrScopeDepth 2, ResolveType 2,
/// GetPutInfo 2, PutByIdFlags 1, ProfileTypeBytecodeFlag 1,
/// PrivateFieldPutKind 1, ErrorTypeWithExtension 1, DebugHookType 1,
/// ResultType 1, JSType 1. `bool` appears in NO `args:` block (only inside
/// the `metadata:` block of `op :jneq_ptr`, `BytecodeList.rb:769`).
///
/// Each variant fixes the operand's `Fits` encode/check semantics for width
/// selection and its sign/zero extension (or structural unpack) on decode;
/// the mirrored `Fits.h` specialization is cited per variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OperandKind {
    /// `VirtualRegister`: signed frame/constant slot index.
    /// `Fits<VirtualRegister, Narrow|Wide16>` constant-band remap
    /// (`Fits.h:118-156`); Wide32 is the same-size `bit_cast` fallback
    /// (`Fits.h:52-64`) storing the raw offset.
    VirtualRegister,
    /// `unsigned` (value/array-profile indices, property/argc/argv, the
    /// m_metadataID slot, ...). `Fits<unsigned, Narrow|Wide16>` unsigned
    /// range check (`Fits.h:66-85`); Wide32 same-size (`Fits.h:52-64`).
    /// Zero-extends on decode.
    UnsignedImmediate,
    /// `int` (`firstVarArg`, `get_argument`'s `index`, ...).
    /// `Fits<int, Narrow|Wide16>` signed range check (`Fits.h:66-85`);
    /// Wide32 same-size. Two's complement; sign-extends on decode.
    SignedImmediate,
    /// `bool`. NO current `args:` block uses it (metadata-only, see the enum
    /// doc); kept so the generator can express it mechanically. Narrow is the
    /// same-size `bit_cast` (`Fits.h:52-64`, sizeof(bool) == 1); wider forms
    /// go through `Fits<bool, size> : Fits<uint8_t, size>` (`Fits.h:87-103`).
    /// Stored 0/1, zero-extends.
    Bool,
    /// `OperandTypes`: a `(ResultType, ResultType)` pair. Narrow packs each
    /// type into 4 bits with unknownType <-> 0 remapped
    /// (`Fits.h:300-353`); Wide16 is the same-size `bit_cast` of the raw
    /// `bits()` (`Fits.h:52-64`) and Wide32 zero-extends them.
    OperandTypes,
    /// `BoundLabel`: SIGNED byte delta from the jump instruction's start to
    /// its target (`Label.h:73-79,146-151`).
    /// `Fits<GenericBoundLabel, size> : Fits<int, size>` (`Fits.h:355-379`).
    BoundLabel,
    /// `ECMAMode`: `value()` byte, 0 = strict / 1 = sloppy
    /// (`ECMAMode.h:39-49`). `Fits<ECMAMode, size> : Fits<uint8_t, size>`
    /// (`Fits.h:381-399`); always fits Narrow.
    ECMAMode,
    /// `IndexingType`: `typedef uint8_t IndexingType` (`IndexingType.h:63`);
    /// plain `Fits<uint8_t>` integral semantics. Always fits Narrow.
    IndexingType,
    /// `SymbolTableOrScopeDepth`: `raw()` u32
    /// (`SymbolTableOrScopeDepth.h:48-63`).
    /// `Fits<SymbolTableOrScopeDepth, size> : Fits<unsigned, size>`
    /// (`Fits.h:158-176`); Wide32 same-size.
    SymbolTableOrScopeDepth,
    /// `ResolveType`: `enum ResolveType : unsigned` (`GetPutInfo.h:59`),
    /// values 0..=13 (Dynamic). Enum `Fits` forwards to the underlying
    /// unsigned (`Fits.h:269-285`).
    ResolveType,
    /// `GetPutInfo`: carried as the C++ class's `m_operand` u32
    /// (`GetPutInfo.h:222-257`: isStrict<<30 | resolveMode<<20 |
    /// initializationMode<<10 | resolveType). Narrow/Wide16 store the
    /// COMPRESSED byte isStrict<<7 | resolveType<<3 | initializationMode<<1 |
    /// resolveMode (`Fits.h:178-232`); Wide32 is the same-size `bit_cast` of
    /// the raw m_operand (`Fits.h:52-64`).
    GetPutInfo,
    /// `PutByIdFlags`: two booleans packed as isStrict<<1 | isDirect
    /// (`Fits.h:234-267`, check always true; `PutByIdFlags.h:32-57`).
    PutByIdFlags,
    /// `ProfileTypeBytecodeFlag`: unscoped enum, values 0..=4
    /// (`ProfileTypeBytecodeFlag.h:30-36`). Clang gives non-negative unscoped
    /// enums an UNSIGNED underlying type, so enum `Fits` (`Fits.h:269-285`)
    /// forwards to `Fits<unsigned>`.
    ProfileTypeBytecodeFlag,
    /// `PrivateFieldPutKind`: `value()` byte, 0..=2
    /// (`PrivateFieldPutKind.h:40-53`).
    /// `Fits<PrivateFieldPutKind, size> : Fits<uint8_t, size>`
    /// (`Fits.h:401-419`); always fits Narrow.
    PrivateFieldPutKind,
    /// `ErrorTypeWithExtension`: `enum class : uint8_t` (`ErrorType.h:59`);
    /// enum `Fits` forwards to `Fits<uint8_t>` (`Fits.h:269-285`). Always
    /// fits Narrow.
    ErrorTypeWithExtension,
    /// `DebugHookType`: unscoped enum, values 0..=8 (`Interpreter.h:91-101`);
    /// unsigned underlying type like [`Self::ProfileTypeBytecodeFlag`].
    DebugHookType,
    /// `ResultType`: `bits()` byte (`ResultType.h:39-59,231`).
    /// `Fits<ResultType, size> : Fits<uint8_t, size>` (`Fits.h:287-298`);
    /// always fits Narrow.
    ResultType,
    /// `JSType`: `enum JSType : uint8_t` (`JSType.h:164`); enum `Fits`
    /// forwards to `Fits<uint8_t>`. Always fits Narrow.
    JSType,
}

impl OperandKind {
    /// Signed stream fields: `VirtualRegister` (`Fits.h:118-156` signed
    /// TargetType), `int`, and `BoundLabel` (`Fits.h:355-379` : Fits<int>).
    /// Everything else is unsigned-backed (`unsigned`, uint8-backed wrappers,
    /// unsigned-underlying enums, and the packed GetPutInfo/OperandTypes/
    /// PutByIdFlags encodings). Governs both the `Fits` range check and
    /// decode extension.
    pub const fn is_signed(self) -> bool {
        matches!(
            self,
            Self::VirtualRegister | Self::SignedImmediate | Self::BoundLabel
        )
    }
}

// The full 193-row generated opcode table (`GeneratedOpcodeRow`,
// `GENERATED_OPCODE_TABLE`, and the four `NUMBER_OF_`/`MAX_LENGTH_OF_`
// constants), produced by tools/bytecode-gen/generate.rb from JSC's OWN Ruby
// bytecode generator over JSC's OWN `bytecode/BytecodeList.rb` and verified
// row-by-row against the local build artifact `Bytecodes.h` (see the file's
// provenance header). Textually included so `OperandKind` above is in scope,
// mirroring how the C++ build compiles its generated `Bytecodes.h` into the
// translation units that define the operand types.
include!("generated/opcode_table.rs");

/// Faithful descriptor for one generated JS opcode.
///
/// Mirrors a `BytecodeList.rb` `op :name, args: { ... }` declaration: the
/// operand schema plus the derived `opcodeLength` that drives
/// `BaseInstruction::size()` (`Instruction.h:138-145`). Built from the pure
/// JSC data of [`GeneratedOpcodeRow`] plus the two Rust-only dispatch fields
/// ([`Self::is_wide_prefix`], [`Self::core`]) — see [`OPCODE_TABLE`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpcodeDescriptor {
    pub id: u8,
    pub name: &'static str,
    /// The `args:` block stream types, in declaration order. Does NOT include
    /// the metadataID slot — that is derived from [`Self::has_metadata`].
    pub operands: &'static [OperandKind],
    /// True when the opcode declares a `metadata:` block: the instruction
    /// then carries ONE extra TRAILING stream slot for m_metadataID
    /// (`generator/Opcode.rb:372-374` `length = args.length + (metadata ?
    /// 1 : 0)`). The slot is emitted as a plain `unsigned` operand
    /// (`generator/Metadata.rb:126-131` `Argument.new(..., :unsigned, -1)`;
    /// `generator/Opcode.rb:185-189` appends `__metadataID` to
    /// `writeOpcode`), so it is sized per-width like every other operand
    /// field. Exactly the 49 leading generated rows set it (IDs 0..49,
    /// `NUMBER_OF_BYTECODE_WITH_METADATA`; `hasMetadata()` = `opcodeID <
    /// numberOfBytecodesWithMetadata`, `Instruction.h:98-101`).
    pub has_metadata: bool,
    /// `numberOfCheckpoints` for the checkpoint-carrying ops — the leading
    /// id partition validated by `generator/Section.rb:72-97` and emitted as
    /// `bytecodeCheckpointCountTable` in the generated `Bytecodes.h`.
    /// Checkpoints subdivide one bytecode into multiple side-effect/exit
    /// sites (`BytecodeIndex.h` checkpoint bits); stored faithfully now,
    /// consumed once checkpoint-aware OSR exit lands. 0 for plain ops.
    pub num_checkpoints: u8,
    /// True for the width-prefix opcodes (`op_wide16`/`op_wide32`).
    pub is_wide_prefix: bool,
    /// Rust-only bridge to the pre-generated `CoreOpcode` dispatch surface.
    ///
    /// C++ has no such field: the generated opcode structs ARE the dispatch
    /// identities. Until the generated table replaces `CoreOpcode`, the wedge
    /// executes raw packed opcodes through the existing `CoreOpcode` arms, and
    /// keeping the id->CoreOpcode mapping HERE — in the one canonical opcode
    /// table — makes drift against `opcode_id` impossible (there is no second
    /// mapping table to fall out of sync). `None` = not executable from raw
    /// packed bytes yet (the wedge admits only mov/ret; `op_add` etc. must NOT
    /// map onto the type-specialized `AddInt32`-style arms).
    pub core: Option<CoreOpcode>,
}

impl OpcodeDescriptor {
    /// `opcodeLengths[id]` = operand-slot count.
    ///
    /// `generator/Opcode.rb:372-374` `length = args.length + (metadata ? 1 : 0)`
    /// and `generator/Section.rb:111` emits `macro(name, length)`. A metadata
    /// opcode's m_metadataID occupies one trailing slot (see
    /// [`Self::has_metadata`]).
    pub const fn opcode_length(&self) -> usize {
        self.operands.len() + self.has_metadata as usize
    }

    /// Stream-operand kind at slot `index`, INCLUDING the trailing
    /// m_metadataID slot of metadata opcodes, which is written as a plain
    /// `unsigned` (`generator/Metadata.rb:126-131`).
    pub const fn operand_kind(&self, index: usize) -> OperandKind {
        if index < self.operands.len() {
            self.operands[index]
        } else if self.has_metadata && index == self.operands.len() {
            OperandKind::UnsignedImmediate
        } else {
            panic!("operand index out of range")
        }
    }
}

/// Overlay ONE pure-JSC generated row with the two Rust-only fields of
/// [`OpcodeDescriptor`].
///
/// C++ has no such overlay step: the generated opcode structs ARE the
/// dispatch identities, and `op_wide16`/`op_wide32` are special-cased
/// directly in `Instruction.h:40-41,81-89`. Keeping the generated artifact
/// pure JSC data and adding `is_wide_prefix`/`core` here — in the one place
/// the canonical table is built — keeps the regenerable file byte-for-byte
/// the generator's output while making id/name/flag drift impossible (there
/// is no second copy of the rows to fall out of sync).
const fn overlay_generated_row(row: GeneratedOpcodeRow) -> OpcodeDescriptor {
    OpcodeDescriptor {
        id: row.id,
        name: row.name,
        operands: row.operands,
        has_metadata: row.has_metadata,
        num_checkpoints: row.num_checkpoints,
        // Exactly op_wide16/op_wide32 are width prefixes
        // (`Instruction.h:40-41,81-89`).
        is_wide_prefix: row.id == opcode_id::WIDE16 || row.id == opcode_id::WIDE32,
        // The wedge dispatch bridge admits ONLY mov/ret (see
        // [`OpcodeDescriptor::core`]); every other generated opcode is
        // decode-only until the generated dispatch cutover — mapping e.g.
        // `op_add` onto the type-specialized `AddInt32`-style arms would not
        // be faithful.
        core: if row.id == opcode_id::MOV {
            Some(CoreOpcode::Move)
        } else if row.id == opcode_id::RET {
            Some(CoreOpcode::Return)
        } else {
            None
        },
    }
}

/// The canonical opcode table: ALL 193 generated rows of JSC's `:Bytecode`
/// section, in generated-ID order (= `BytecodeList.rb` declaration order),
/// overlaid with the Rust-only dispatch fields. Models JSC's ONE untyped op
/// per operation (`op_add`, NOT `AddInt32`): type specialization is the JIT's
/// job via the profile operand (`metadata-table.md` `d1cb45f8`).
static OPCODE_TABLE: [OpcodeDescriptor; NUMBER_OF_BYTECODE_IDS] = {
    let mut table = [overlay_generated_row(GENERATED_OPCODE_TABLE[0]); NUMBER_OF_BYTECODE_IDS];
    let mut i = 1;
    while i < NUMBER_OF_BYTECODE_IDS {
        table[i] = overlay_generated_row(GENERATED_OPCODE_TABLE[i]);
        i += 1;
    }
    table
};

/// Look up a descriptor by opcode ID: a direct table index, as the C++ engine
/// indexes its generated per-opcode tables by `OpcodeID` (ids are dense
/// 0..NUMBER_OF_BYTECODE_IDS in declaration order). `None` past the end.
pub fn descriptor_for(id: u8) -> Option<&'static OpcodeDescriptor> {
    let descriptor = OPCODE_TABLE.get(id as usize)?;
    debug_assert!(descriptor.id == id, "table row ids are the dense indices");
    Some(descriptor)
}

/// A concrete operand value handed to the writer. Carries the integer plus its
/// `OperandKind` so the writer can range-check and convert via `Fits`.
///
/// Each variant carries the C++ type's canonical scalar (the same value the
/// generated struct field holds / `decode` returns); see the matching
/// [`OperandKind`] variant for the storage evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperandValue {
    /// Raw signed `VirtualRegister` offset.
    VirtualRegister(i32),
    /// `unsigned` arg (profile/property/argc/... index, or the m_metadataID
    /// slot value of a metadata opcode).
    UnsignedImmediate(u32),
    /// `int` arg.
    SignedImmediate(i32),
    /// `bool` arg (no current `args:` block uses it; see `OperandKind::Bool`).
    Bool(bool),
    /// `OperandTypes` as its `bits()` u16: low byte = first, high byte =
    /// second (little-endian `bit_cast` of `{ m_first, m_second }`,
    /// `ResultType.h:244-274`).
    OperandTypes(u16),
    /// Signed byte delta from the jump instruction start to its target.
    BoundLabel(i32),
    /// `ECMAMode::value()`: 0 = strict, 1 = sloppy (`ECMAMode.h:39-49`).
    ECMAMode(u8),
    /// `IndexingType` byte (`IndexingType.h:63`).
    IndexingType(u8),
    /// `SymbolTableOrScopeDepth::raw()` (`SymbolTableOrScopeDepth.h:48-63`).
    SymbolTableOrScopeDepth(u32),
    /// `ResolveType` enumerator value (`GetPutInfo.h:59`).
    ResolveType(u32),
    /// `GetPutInfo::m_operand` (`GetPutInfo.h:222-257`).
    GetPutInfo(u32),
    /// `PutByIdFlags` packed as isStrict<<1 | isDirect (`Fits.h:238-258`).
    PutByIdFlags(u8),
    /// `ProfileTypeBytecodeFlag` enumerator value
    /// (`ProfileTypeBytecodeFlag.h:30-36`).
    ProfileTypeBytecodeFlag(u32),
    /// `PrivateFieldPutKind::value()` (`PrivateFieldPutKind.h:40-53`).
    PrivateFieldPutKind(u8),
    /// `ErrorTypeWithExtension` enumerator value (`ErrorType.h:59`).
    ErrorTypeWithExtension(u8),
    /// `DebugHookType` enumerator value (`Interpreter.h:91-101`).
    DebugHookType(u32),
    /// `ResultType::bits()` (`ResultType.h:39-59,231`).
    ResultType(u8),
    /// `JSType` enumerator value (`JSType.h:164`).
    JSType(u8),
}

impl OperandValue {
    const fn kind(self) -> OperandKind {
        match self {
            Self::VirtualRegister(_) => OperandKind::VirtualRegister,
            Self::UnsignedImmediate(_) => OperandKind::UnsignedImmediate,
            Self::SignedImmediate(_) => OperandKind::SignedImmediate,
            Self::Bool(_) => OperandKind::Bool,
            Self::OperandTypes(_) => OperandKind::OperandTypes,
            Self::BoundLabel(_) => OperandKind::BoundLabel,
            Self::ECMAMode(_) => OperandKind::ECMAMode,
            Self::IndexingType(_) => OperandKind::IndexingType,
            Self::SymbolTableOrScopeDepth(_) => OperandKind::SymbolTableOrScopeDepth,
            Self::ResolveType(_) => OperandKind::ResolveType,
            Self::GetPutInfo(_) => OperandKind::GetPutInfo,
            Self::PutByIdFlags(_) => OperandKind::PutByIdFlags,
            Self::ProfileTypeBytecodeFlag(_) => OperandKind::ProfileTypeBytecodeFlag,
            Self::PrivateFieldPutKind(_) => OperandKind::PrivateFieldPutKind,
            Self::ErrorTypeWithExtension(_) => OperandKind::ErrorTypeWithExtension,
            Self::DebugHookType(_) => OperandKind::DebugHookType,
            Self::ResultType(_) => OperandKind::ResultType,
            Self::JSType(_) => OperandKind::JSType,
        }
    }

    /// The value-domain integer (what `Fits::convert(TargetType)` yields back
    /// after decode; for GetPutInfo/OperandTypes the STRUCTURAL wide form,
    /// not the narrow packing).
    const fn as_i64(self) -> i64 {
        match self {
            Self::VirtualRegister(value) => value as i64,
            Self::UnsignedImmediate(value) => value as i64,
            Self::SignedImmediate(value) => value as i64,
            Self::Bool(value) => value as i64,
            Self::OperandTypes(value) => value as i64,
            Self::BoundLabel(value) => value as i64,
            Self::ECMAMode(value) => value as i64,
            Self::IndexingType(value) => value as i64,
            Self::SymbolTableOrScopeDepth(value) => value as i64,
            Self::ResolveType(value) => value as i64,
            Self::GetPutInfo(value) => value as i64,
            Self::PutByIdFlags(value) => value as i64,
            Self::ProfileTypeBytecodeFlag(value) => value as i64,
            Self::PrivateFieldPutKind(value) => value as i64,
            Self::ErrorTypeWithExtension(value) => value as i64,
            Self::DebugHookType(value) => value as i64,
            Self::ResultType(value) => value as i64,
            Self::JSType(value) => value as i64,
        }
    }

    /// `Fits<T, size>::check` (`Fits.h:71-74`) with the structural
    /// specializations: the `Fits<VirtualRegister>` constant remap
    /// (`Fits.h:118-156`), the `Fits<GetPutInfo>` compressed-field check
    /// (`Fits.h:206-212`), and the `Fits<OperandTypes>` narrow nibble check
    /// (`Fits.h:311-323`). Does this operand fit the width?
    fn fits_check(self, width: OpcodeSize) -> bool {
        match self {
            Self::VirtualRegister(register) => virtual_register_fits_check(register, width),
            Self::GetPutInfo(operand) => get_put_info_fits_check(operand, width),
            Self::OperandTypes(bits) => operand_types_fits_check(bits, width),
            _ if self.kind().is_signed() => {
                let value = self.as_i64();
                value >= width.signed_min() && value <= width.signed_max()
            }
            _ => (self.as_i64() as u64) <= width.unsigned_max(),
        }
    }

    /// `Fits<T, size>::convert` (`Fits.h:76-80`) with the structural
    /// encodings: `Fits<VirtualRegister>` constant-band (`Fits.h:141-147`),
    /// `Fits<GetPutInfo>` compressed byte (`Fits.h:214-222`), and
    /// `Fits<OperandTypes>` narrow nibble pack (`Fits.h:325-338`). The result
    /// is the width-truncated little-endian bit pattern stored into the
    /// stream.
    fn fits_convert(self, width: OpcodeSize) -> u64 {
        debug_assert!(
            self.fits_check(width),
            "operand does not fit selected width"
        );
        let converted = match self {
            Self::VirtualRegister(register) => {
                virtual_register_fits_convert(register, width) as u64
            }
            Self::GetPutInfo(operand) => get_put_info_fits_convert(operand, width),
            Self::OperandTypes(bits) => operand_types_fits_convert(bits, width),
            _ => self.as_i64() as u64,
        };
        converted & width.unsigned_max()
    }
}

// The `VirtualRegister` namespace constants (`FirstConstantRegisterIndex` and
// the per-width `Fits` band starts `FirstConstantRegisterIndex8/16`) live in
// `bytecode/register.rs`, the mirror of `BytecodeConventions.h:33-37` /
// `VirtualRegister.h`. JSC has exactly ONE such named-constant scheme, so this
// packed core imports it instead of duplicating it.

const fn first_constant_for_width(width: OpcodeSize) -> Option<i32> {
    match width {
        OpcodeSize::Narrow => Some(FIRST_CONSTANT_REGISTER_INDEX8),
        OpcodeSize::Wide16 => Some(FIRST_CONSTANT_REGISTER_INDEX16),
        // `Fits<VirtualRegister, Wide32>` is the ordinary integral fallback:
        // wide32 stores the raw VirtualRegister namespace with constants at
        // `FirstConstantRegisterIndex`.
        OpcodeSize::Wide32 => None,
    }
}

fn virtual_register_fits_check(register: i32, width: OpcodeSize) -> bool {
    let register = VirtualRegister::from_raw(register);
    if let Some(first_constant) = first_constant_for_width(width) {
        if let Some(constant_index) = register.to_constant_index() {
            return first_constant.saturating_add(constant_index as i32)
                <= width.signed_max() as i32;
        }
        return (register.raw() as i64) >= width.signed_min() && register.raw() < first_constant;
    }
    (register.raw() as i64) >= width.signed_min() && (register.raw() as i64) <= width.signed_max()
}

fn virtual_register_fits_convert(register: i32, width: OpcodeSize) -> i64 {
    let register = VirtualRegister::from_raw(register);
    if let Some(first_constant) = first_constant_for_width(width) {
        if let Some(constant_index) = register.to_constant_index() {
            return i64::from(first_constant + constant_index as i32);
        }
    }
    i64::from(register.raw())
}

fn virtual_register_fits_decode(encoded: i64, width: OpcodeSize) -> i32 {
    if let Some(first_constant) = first_constant_for_width(width) {
        let encoded = encoded as i32;
        if encoded >= first_constant {
            return VirtualRegister::constant((encoded - first_constant) as u32).raw();
        }
        return encoded;
    }
    encoded as i32
}

// `GetPutInfo::m_operand` layout (`GetPutInfo.h:225-233`): 10 bits per field —
// resolveType bits 0..10, initializationMode bits 10..20, resolveMode bits
// 20..30, isStrict bit 30.
const GET_PUT_INFO_INITIALIZATION_SHIFT: u32 = 10;
const GET_PUT_INFO_MODE_SHIFT: u32 = 20;
const GET_PUT_INFO_IS_STRICT_SHIFT: u32 = 30;
const GET_PUT_INFO_TYPE_BITS: u32 = (1 << GET_PUT_INFO_INITIALIZATION_SHIFT) - 1;
const GET_PUT_INFO_FIELD_MASK: u32 = GET_PUT_INFO_TYPE_BITS;

/// Split an `m_operand` into (resolveType, initializationMode, resolveMode,
/// isStrict) per the accessors `GetPutInfo.h:248-251`.
const fn get_put_info_fields(operand: u32) -> (u32, u32, u32, u32) {
    let resolve_type = operand & GET_PUT_INFO_TYPE_BITS;
    let initialization_mode =
        (operand >> GET_PUT_INFO_INITIALIZATION_SHIFT) & GET_PUT_INFO_FIELD_MASK;
    let resolve_mode = (operand >> GET_PUT_INFO_MODE_SHIFT) & GET_PUT_INFO_FIELD_MASK;
    let is_strict = (operand >> GET_PUT_INFO_IS_STRICT_SHIFT) & 1;
    (resolve_type, initialization_mode, resolve_mode, is_strict)
}

/// `Fits<GetPutInfo, Narrow|Wide16>::check` (`Fits.h:206-212`): the
/// compressed encoding holds resolveType in 4 bits, initializationMode in 2,
/// resolveMode in 1. Wide32 is the same-size `bit_cast` of the raw
/// m_operand (`Fits.h:52-64`), which always fits.
fn get_put_info_fits_check(operand: u32, width: OpcodeSize) -> bool {
    if matches!(width, OpcodeSize::Wide32) {
        return true;
    }
    let (resolve_type, initialization_mode, resolve_mode, _) = get_put_info_fields(operand);
    resolve_type < 16 && initialization_mode < 4 && resolve_mode < 2
}

/// `Fits<GetPutInfo, Narrow|Wide16>::convert` (`Fits.h:214-222`):
/// `isStrict << 7 | resolveType << 3 | initializationMode << 1 | resolveMode`.
/// Wide32: raw m_operand.
fn get_put_info_fits_convert(operand: u32, width: OpcodeSize) -> u64 {
    if matches!(width, OpcodeSize::Wide32) {
        return operand as u64;
    }
    let (resolve_type, initialization_mode, resolve_mode, is_strict) = get_put_info_fields(operand);
    ((is_strict << 7) | (resolve_type << 3) | (initialization_mode << 1) | resolve_mode) as u64
}

/// `Fits<GetPutInfo, Narrow|Wide16>::convert(TargetType)` (`Fits.h:224-231`)
/// rebuilt into the m_operand layout the `GetPutInfo(resolveMode,
/// resolveType, initializationMode, ecmaMode)` constructor produces
/// (`GetPutInfo.h:238-240`). Wide32: raw m_operand back unchanged.
fn get_put_info_fits_decode(raw: u64, width: OpcodeSize) -> u32 {
    if matches!(width, OpcodeSize::Wide32) {
        return raw as u32;
    }
    let resolve_type = ((raw >> 3) & 0xf) as u32;
    let initialization_mode = ((raw >> 1) & 0x3) as u32;
    let resolve_mode = (raw & 0x1) as u32;
    let is_strict = ((raw >> 7) & 0x1) as u32;
    (is_strict << GET_PUT_INFO_IS_STRICT_SHIFT)
        | (resolve_mode << GET_PUT_INFO_MODE_SHIFT)
        | (initialization_mode << GET_PUT_INFO_INITIALIZATION_SHIFT)
        | resolve_type
}

/// `ResultType::unknownType().bits()` = TypeBits = 0b111_1110
/// (`ResultType.h:40-51,168-171`).
const RESULT_TYPE_UNKNOWN_BITS: u16 = 0x7e;
/// `Fits<OperandTypes>` narrow packing constants (`Fits.h:308-309`).
const OPERAND_TYPES_TYPE_WIDTH: u32 = 4;
const OPERAND_TYPES_MAX_TYPE: u16 = (1 << OPERAND_TYPES_TYPE_WIDTH) - 1;

/// Split `OperandTypes::bits()` into (first, second): the u16 is the
/// little-endian `bit_cast` of `{ uint8_t m_first; uint8_t m_second; }`
/// (`ResultType.h:244-274`), so first is the LOW byte.
const fn operand_types_first_second(bits: u16) -> (u16, u16) {
    (bits & 0xff, bits >> 8)
}

/// The narrow unknownType <-> 0 remap of `Fits<OperandTypes>`
/// (`Fits.h:313-320`): unknown is encoded as 0 so it fits 4 bits.
const fn operand_types_narrow_remap(type_bits: u16) -> u16 {
    if type_bits == RESULT_TYPE_UNKNOWN_BITS {
        0
    } else {
        type_bits
    }
}

/// `Fits<OperandTypes, Narrow>::check` (`Fits.h:311-323`): each remapped type
/// must fit 4 bits. Wide16 is the same-size `bit_cast` (`Fits.h:52-64`) and
/// Wide32 returns true (`Fits.h:322`).
fn operand_types_fits_check(bits: u16, width: OpcodeSize) -> bool {
    if !matches!(width, OpcodeSize::Narrow) {
        return true;
    }
    let (first, second) = operand_types_first_second(bits);
    operand_types_narrow_remap(first) <= OPERAND_TYPES_MAX_TYPE
        && operand_types_narrow_remap(second) <= OPERAND_TYPES_MAX_TYPE
}

/// `Fits<OperandTypes, Narrow>::convert` (`Fits.h:325-338`):
/// `(first << 4) | second` after the unknown->0 remap. Wide16/Wide32 store
/// the raw `bits()`.
fn operand_types_fits_convert(bits: u16, width: OpcodeSize) -> u64 {
    if !matches!(width, OpcodeSize::Narrow) {
        return bits as u64;
    }
    let (first, second) = operand_types_first_second(bits);
    ((operand_types_narrow_remap(first) << OPERAND_TYPES_TYPE_WIDTH)
        | operand_types_narrow_remap(second)) as u64
}

/// `Fits<OperandTypes, Narrow>::convert(TargetType)` (`Fits.h:340-352`):
/// unpack the nibbles with the 0 -> unknownType remap; wide forms truncate
/// the raw field back to the u16 `bits()` (`Fits.h:351`
/// `fromBits(static_cast<uint16_t>(types))`).
fn operand_types_fits_decode(raw: u64, width: OpcodeSize) -> u16 {
    if !matches!(width, OpcodeSize::Narrow) {
        return raw as u16;
    }
    let mut first = ((raw >> OPERAND_TYPES_TYPE_WIDTH) & OPERAND_TYPES_MAX_TYPE as u64) as u16;
    let mut second = (raw & OPERAND_TYPES_MAX_TYPE as u64) as u16;
    if first == 0 {
        first = RESULT_TYPE_UNKNOWN_BITS;
    }
    if second == 0 {
        second = RESULT_TYPE_UNKNOWN_BITS;
    }
    first | (second << 8)
}

/// Back-convert one raw width-truncated stream field to its value-domain
/// integer — the `Fits<T, size>::convert(TargetType)` direction used by the
/// generated struct accessors: constant-band remap for `VirtualRegister`,
/// structural unpack for `GetPutInfo`/`OperandTypes`, sign extension for the
/// signed kinds, zero extension for everything else.
fn fits_decode(kind: OperandKind, raw: u64, width: OpcodeSize) -> i64 {
    match kind {
        OperandKind::VirtualRegister => i64::from(virtual_register_fits_decode(
            sign_extend(raw, width.operand_bytes()),
            width,
        )),
        OperandKind::GetPutInfo => i64::from(get_put_info_fits_decode(raw, width)),
        OperandKind::OperandTypes => i64::from(operand_types_fits_decode(raw, width)),
        kind if kind.is_signed() => sign_extend(raw, width.operand_bytes()),
        _ => raw as i64,
    }
}

/// Read `width` little-endian bytes from `bytes[start..]` as an unsigned value.
fn read_unsigned_le(bytes: &[u8], start: usize, width: usize) -> u64 {
    let mut value = 0u64;
    let mut k = 0;
    while k < width {
        value |= (bytes[start + k] as u64) << (8 * k);
        k += 1;
    }
    value
}

/// Sign-extend the low `width*8` bits of `value` to `i64`.
fn sign_extend(value: u64, width: usize) -> i64 {
    let bits = width * 8;
    let shift = 64 - bits;
    ((value << shift) as i64) >> shift
}

/// Safe decoded view of one declared-subset packed instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawDecodedInstruction {
    pub offset: usize,
    pub opcode_id: u8,
    pub name: &'static str,
    pub width: OpcodeSize,
    pub size: usize,
    pub operands: Vec<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawInstructionDecodeError {
    OffsetOutOfBounds {
        offset: usize,
        len: usize,
    },
    MissingWideOpcode {
        offset: usize,
        len: usize,
    },
    UnknownOpcode {
        offset: usize,
        opcode_id: u8,
    },
    TruncatedInstruction {
        offset: usize,
        size: usize,
        len: usize,
    },
}

/// Decode an instruction header at `offset`: the operand width, descriptor, and
/// the byte offset where operands begin.
///
/// Faithful to `BaseInstruction::opcodeID()`/`width()` (`Instruction.h:67-96`)
/// and the `narrow()`/`wide16()`/`wide32()` pointer math (`Instruction.h:181-198`):
/// read the first byte; if it is the `wide32`/`wide16` prefix the real opcode is
/// the NEXT byte and operands start after it, otherwise the first byte is the
/// opcode and operands start immediately after.
fn try_decode_header(
    bytes: &[u8],
    offset: usize,
) -> Result<(OpcodeSize, &'static OpcodeDescriptor, usize), RawInstructionDecodeError> {
    let Some(&first) = bytes.get(offset) else {
        return Err(RawInstructionDecodeError::OffsetOutOfBounds {
            offset,
            len: bytes.len(),
        });
    };
    let (width, opcode_byte_index) = if first == opcode_id::WIDE32 {
        (OpcodeSize::Wide32, offset + 1)
    } else if first == opcode_id::WIDE16 {
        (OpcodeSize::Wide16, offset + 1)
    } else {
        (OpcodeSize::Narrow, offset)
    };
    let Some(&id) = bytes.get(opcode_byte_index) else {
        return Err(RawInstructionDecodeError::MissingWideOpcode {
            offset,
            len: bytes.len(),
        });
    };
    let descriptor = descriptor_for(id).ok_or(RawInstructionDecodeError::UnknownOpcode {
        offset,
        opcode_id: id,
    })?;
    // operands_start = opcode byte index + opcodeIDBytes(1). Equivalently
    // offset + prefixSize + 1 (Argument.rb setter/load_from_stream location).
    let operands_start = opcode_byte_index + OPCODE_ID_BYTES;
    Ok((width, descriptor, operands_start))
}

fn decode_header(bytes: &[u8], offset: usize) -> (OpcodeSize, &'static OpcodeDescriptor, usize) {
    try_decode_header(bytes, offset).expect("invalid opcode header in stream")
}

/// `BaseInstruction::size()` at `offset` (`Instruction.h:138-145`):
/// `opcodeIDBytes + opcodeLengths[id]*operandSize + prefixSize`, validated
/// against the buffer end. Decodes the header only — no operand values.
pub fn raw_instruction_size(
    bytes: &[u8],
    offset: usize,
) -> Result<usize, RawInstructionDecodeError> {
    let (width, descriptor, _) = try_decode_header(bytes, offset)?;
    let size =
        OPCODE_ID_BYTES + descriptor.opcode_length() * width.operand_bytes() + width.prefix_bytes();
    if offset.saturating_add(size) > bytes.len() {
        return Err(RawInstructionDecodeError::TruncatedInstruction {
            offset,
            size,
            len: bytes.len(),
        });
    }
    Ok(size)
}

/// Is `offset` the START of an instruction in this stream?
///
/// C++ `InstructionStream` never needs this check: every `BytecodeIndex` it
/// hands out originates from iteration, which only ever advances by whole
/// instruction sizes (`InstructionStream.h:154-161` `operator++ += size()`),
/// and `at(Offset)` merely ASSERTs the bounds of a trusted offset
/// (`InstructionStream.h:177-181`). The safe-Rust dispatch surface accepts
/// arbitrary `BytecodeIndex` values from requests and jump operands, so it must
/// re-derive the instruction-start property the C++ type system gets from
/// provenance: walk from 0 by `size()` and require landing exactly on `offset`.
/// Returns `Err` only if the stream itself is malformed before `offset`.
pub fn is_instruction_start(
    bytes: &[u8],
    offset: usize,
) -> Result<bool, RawInstructionDecodeError> {
    if offset >= bytes.len() {
        return Ok(false);
    }
    let mut walk = 0usize;
    while walk < offset {
        walk = walk.saturating_add(raw_instruction_size(bytes, walk)?);
    }
    Ok(walk == offset)
}

pub fn decode_raw_instruction(
    bytes: &[u8],
    offset: usize,
) -> Result<RawDecodedInstruction, RawInstructionDecodeError> {
    let (width, descriptor, operands_start) = try_decode_header(bytes, offset)?;
    let size =
        OPCODE_ID_BYTES + descriptor.opcode_length() * width.operand_bytes() + width.prefix_bytes();
    if offset.saturating_add(size) > bytes.len() {
        return Err(RawInstructionDecodeError::TruncatedInstruction {
            offset,
            size,
            len: bytes.len(),
        });
    }
    let mut operands = Vec::with_capacity(descriptor.opcode_length());
    for index in 0..descriptor.opcode_length() {
        let at = operands_start + index * width.operand_bytes();
        let raw = read_unsigned_le(bytes, at, width.operand_bytes());
        operands.push(fits_decode(descriptor.operand_kind(index), raw, width));
    }
    Ok(RawDecodedInstruction {
        offset,
        opcode_id: descriptor.id,
        name: descriptor.name,
        width,
        size,
        operands,
    })
}

/// Byte-stream writer.
///
/// Faithful to `InstructionStreamWriter` (`InstructionStream.h:207-344`): a
/// `Vec<u8>` plus a write `position`; `reserve` grows the buffer and advances
/// the position; `seek`/`rewind` reposition; `finalize` freezes the buffer.
#[derive(Clone, Debug, Default)]
pub struct InstructionStreamWriter {
    instructions: Vec<u8>,
    position: usize,
    finalized: bool,
}

impl InstructionStreamWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `position()` (`InstructionStream.h:240-243`): current write cursor (also
    /// the byte offset of the next instruction).
    pub fn position(&self) -> usize {
        self.position
    }

    /// `seek(position)` (`InstructionStream.h:234-238`).
    pub fn seek(&mut self, position: usize) {
        debug_assert!(position <= self.instructions.len());
        self.position = position;
    }

    /// `reserve<size>()` (`InstructionStream.h:254-263`): grow to
    /// `position + size` if needed, return the start index, advance `position`.
    fn reserve(&mut self, size: usize) -> usize {
        debug_assert!(!self.finalized);
        if self.position + size > self.instructions.len() {
            self.instructions.resize(self.position + size, 0);
        }
        let result = self.position;
        self.position += size;
        result
    }

    /// One unaligned integral store, advancing the cursor (the per-arg body of
    /// `write<Args...>`, `InstructionStream.h:245-252`).
    fn write_int(&mut self, value: u64, width: usize) {
        let start = self.reserve(width);
        let mut k = 0;
        while k < width {
            self.instructions[start + k] = ((value >> (8 * k)) & 0xff) as u8;
            k += 1;
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write_int(value as u64, 1);
    }

    /// Emit one instruction, choosing the narrowest width that fits every
    /// operand, then writing `[wide-prefix?][opcode][operands...]`. Returns the
    /// instruction's byte offset.
    ///
    /// Width selection mirrors the generated per-opcode emitters' `Fits` cascade
    /// (`Fits.h`): try Narrow, then Wide16, then Wide32; the chosen width is
    /// shared by every operand field (`instruction-format.md` line 3). The
    /// opcode byte is always narrow (`OPCODE_ID_BYTES`); a wide form prepends the
    /// `op_wide16`/`op_wide32` prefix byte (`Instruction.h:181-198`).
    pub fn emit(&mut self, id: u8, operands: &[OperandValue]) -> usize {
        let descriptor = descriptor_for(id).expect("unknown opcode id");
        assert_eq!(
            operands.len(),
            descriptor.opcode_length(),
            "operand count must match opcodeLengths[{}] (incl. metadataID slot)",
            descriptor.name
        );
        // C++ gets per-operand type agreement from the generated emitters'
        // typed signatures (`generator/Opcode.rb:185-189`); the raw writer
        // re-checks it dynamically. A metadata opcode's trailing slot is the
        // `unsigned` metadataID (`generator/Metadata.rb:126-131`).
        for (index, operand) in operands.iter().enumerate() {
            debug_assert!(
                operand.kind() == descriptor.operand_kind(index),
                "operand {index} of {} must be {:?}",
                descriptor.name,
                descriptor.operand_kind(index)
            );
        }
        let width = select_width(operands);
        let start = self.position;

        // Width prefix (PaddingBySize / op_wide16|op_wide32).
        match width {
            OpcodeSize::Narrow => {}
            OpcodeSize::Wide16 => self.write_u8(opcode_id::WIDE16),
            OpcodeSize::Wide32 => self.write_u8(opcode_id::WIDE32),
        }
        // Opcode ID — always one byte (Opcode.h:86-87).
        self.write_u8(id);
        // Operands, each `width.operand_bytes()` wide.
        for operand in operands {
            let raw = operand.fits_convert(width);
            self.write_int(raw, width.operand_bytes());
        }
        start
    }

    /// Patch one operand of an already-written instruction in place.
    ///
    /// Models the forward-jump resolution path (`Label::bind`,
    /// `Label.h:146-151`) and the generated `set<Field>` setter
    /// (`generator/Argument.rb:73-84`): the operand byte location is
    /// `instructionStart + index*operandSize + PaddingBySize(prefix) +
    /// opcodeIDSize`. The patched value MUST fit the instruction's already-chosen
    /// width (true here because the placeholder reserved an adequate width).
    pub fn set_operand(
        &mut self,
        instruction_offset: usize,
        operand_index: usize,
        value: OperandValue,
    ) {
        let (width, descriptor, operands_start) =
            decode_header(&self.instructions, instruction_offset);
        assert!(operand_index < descriptor.opcode_length());
        debug_assert!(
            value.kind() == descriptor.operand_kind(operand_index),
            "patched operand {operand_index} of {} must be {:?}",
            descriptor.name,
            descriptor.operand_kind(operand_index)
        );
        let at = operands_start + operand_index * width.operand_bytes();
        let raw = value.fits_convert(width);
        let mut k = 0;
        while k < width.operand_bytes() {
            self.instructions[at + k] = ((raw >> (8 * k)) & 0xff) as u8;
            k += 1;
        }
    }

    /// `finalize()` (`InstructionStream.h:272-277`): freeze the buffer into an
    /// immutable `InstructionStream`.
    pub fn finalize(mut self) -> InstructionStream {
        self.finalized = true;
        self.instructions.truncate(self.position);
        InstructionStream {
            instructions: self.instructions,
        }
    }
}

/// Smallest `OpcodeSize` in which every operand fits.
///
/// Faithful to the generated `Fits<...>::check` cascade: an empty operand list
/// (e.g. `op_enter`) selects Narrow.
fn select_width(operands: &[OperandValue]) -> OpcodeSize {
    for width in [OpcodeSize::Narrow, OpcodeSize::Wide16, OpcodeSize::Wide32] {
        if operands.iter().all(|operand| operand.fits_check(width)) {
            return width;
        }
    }
    OpcodeSize::Wide32
}

/// Immutable packed instruction stream.
///
/// Faithful to `InstructionStream<JSInstruction>` (`InstructionStream.h:44-205`):
/// the byte buffer is the program; positions are BYTE OFFSETS, never ordinals.
#[derive(Clone, Debug, Default)]
pub struct InstructionStream {
    instructions: Vec<u8>,
}

impl InstructionStream {
    /// `sizeInBytes()` / `size()` (`InstructionStream.h:53-56,183-186`).
    pub fn size_in_bytes(&self) -> usize {
        self.instructions.len()
    }

    /// Raw bytes — the LLInt/JIT consume exactly these.
    pub fn bytes(&self) -> &[u8] {
        &self.instructions
    }

    /// `at(Offset)` (`InstructionStream.h:177-181`): a `Ref` at a byte offset.
    pub fn at_offset(&self, offset: usize) -> Ref<'_> {
        debug_assert!(offset < self.instructions.len());
        Ref {
            instructions: &self.instructions,
            index: offset,
        }
    }

    /// `at(BytecodeIndex)` (`InstructionStream.h:176`): resolve the index's BYTE
    /// OFFSET (`BytecodeIndex::offset()`), NOT an ordinal.
    pub fn at(&self, index: BytecodeIndex) -> Ref<'_> {
        self.at_offset(index.offset() as usize)
    }

    /// `begin()` (`InstructionStream.h:166-169`): a `Ref` at offset 0.
    pub fn begin(&self) -> Ref<'_> {
        Ref {
            instructions: &self.instructions,
            index: 0,
        }
    }

    /// Walk the stream instruction by instruction, advancing by `size()`
    /// (`InstructionStream.h:141-163` iterator).
    pub fn cursor(&self) -> InstructionCursor<'_> {
        InstructionCursor {
            instructions: &self.instructions,
            index: 0,
        }
    }
}

/// A cursor/reference to one instruction at a byte offset.
///
/// Faithful to `InstructionStream::Ref` (`InstructionStream.h:60-114`): it holds
/// the buffer and a byte index, decoding the instruction lazily.
#[derive(Clone, Copy, Debug)]
pub struct Ref<'a> {
    instructions: &'a [u8],
    index: usize,
}

impl<'a> Ref<'a> {
    /// `offset()` (`InstructionStream.h:92`): the BYTE position of this
    /// instruction.
    pub fn offset(&self) -> usize {
        self.index
    }

    /// `index()` (`InstructionStream.h:93`): the byte offset as a
    /// `BytecodeIndex` (`BytecodeIndex(offset())`). This is the byte-offset
    /// semantic the typed-`Vec`-by-ordinal representation lacks.
    pub fn bytecode_index(&self) -> BytecodeIndex {
        BytecodeIndex::from_offset(self.index as u32)
    }

    /// `isValid()` (`InstructionStream.h:95-98`): `index < size`.
    pub fn is_valid(&self) -> bool {
        self.index < self.instructions.len()
    }

    /// The first byte — `narrow()->opcodeID()` (`Instruction.h:181-184`).
    fn first_byte(&self) -> u8 {
        self.instructions[self.index]
    }

    /// `isWide16()` (`Instruction.h:81-84`).
    pub fn is_wide16(&self) -> bool {
        self.first_byte() == opcode_id::WIDE16
    }

    /// `isWide32()` (`Instruction.h:86-89`).
    pub fn is_wide32(&self) -> bool {
        self.first_byte() == opcode_id::WIDE32
    }

    /// `width()` (`Instruction.h:91-96`).
    pub fn width(&self) -> OpcodeSize {
        decode_header(self.instructions, self.index).0
    }

    fn descriptor(&self) -> &'static OpcodeDescriptor {
        decode_header(self.instructions, self.index).1
    }

    /// `opcodeID()` (`Instruction.h:67-74`): the real opcode, resolved through
    /// the wide prefix.
    pub fn opcode_id(&self) -> u8 {
        self.descriptor().id
    }

    pub fn name(&self) -> &'static str {
        self.descriptor().name
    }

    pub fn operand_count(&self) -> usize {
        self.descriptor().opcode_length()
    }

    /// `is<T>()` (`Instruction.h:147-151`): does this instruction decode to
    /// opcode `id`?
    pub fn is(&self, id: u8) -> bool {
        self.opcode_id() == id
    }

    /// `size()` (`Instruction.h:138-145`):
    /// `opcodeIDBytes + opcodeLengths[id]*operandSize + prefixSize`.
    pub fn size(&self) -> usize {
        let (width, descriptor, _) = decode_header(self.instructions, self.index);
        OPCODE_ID_BYTES + descriptor.opcode_length() * width.operand_bytes() + width.prefix_bytes()
    }

    /// `next()` (`InstructionStream.h:87-90`): the `Ref` `size()` bytes ahead —
    /// the program counter advances by instruction SIZE, never by one ordinal.
    pub fn next(&self) -> Ref<'a> {
        Ref {
            instructions: self.instructions,
            index: self.index + self.size(),
        }
    }

    /// Decode operand `i` to its signed integer value at the instruction's
    /// width. Operand location: `operands_start + i*operandSize`
    /// (`generator/Argument.rb:60`); signedness from the operand kind.
    pub fn operand(&self, i: usize) -> i64 {
        let (width, descriptor, operands_start) = decode_header(self.instructions, self.index);
        assert!(i < descriptor.opcode_length());
        let at = operands_start + i * width.operand_bytes();
        let raw = read_unsigned_le(self.instructions, at, width.operand_bytes());
        fits_decode(descriptor.operand_kind(i), raw, width)
    }

    /// Resolve a `BoundLabel` operand to an absolute target BYTE OFFSET:
    /// `instructionStart + relativeDelta` (`Label.h:73-79,146-151`). The delta is
    /// stored relative to this instruction's start, so the absolute target is the
    /// instruction offset plus the decoded operand.
    pub fn jump_target(&self, operand_index: usize) -> usize {
        let delta = self.operand(operand_index);
        (self.index as i64 + delta) as usize
    }
}

/// Iterator that advances by `size()` (`InstructionStream.h` iterator,
/// `:141-163`).
#[derive(Clone, Debug)]
pub struct InstructionCursor<'a> {
    instructions: &'a [u8],
    index: usize,
}

impl<'a> Iterator for InstructionCursor<'a> {
    type Item = Ref<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.instructions.len() {
            return None;
        }
        let current = Ref {
            instructions: self.instructions,
            index: self.index,
        };
        self.index += current.size();
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::opcode_id::*;
    use super::*;
    use crate::bytecode::register::FIRST_CONSTANT_REGISTER_INDEX;

    /// `size()` matches the `Instruction.h:138-145` formula for each width.
    #[test]
    fn size_formula_matches_cpp_per_width() {
        // op_mov: 2 operands, no metadata -> opcodeLengths = 2.
        let narrow = OPCODE_ID_BYTES
            + 2 * OpcodeSize::Narrow.operand_bytes()
            + OpcodeSize::Narrow.prefix_bytes();
        let wide16 = OPCODE_ID_BYTES
            + 2 * OpcodeSize::Wide16.operand_bytes()
            + OpcodeSize::Wide16.prefix_bytes();
        let wide32 = OPCODE_ID_BYTES
            + 2 * OpcodeSize::Wide32.operand_bytes()
            + OpcodeSize::Wide32.prefix_bytes();
        assert_eq!(narrow, 1 + 2 + 0); // [op][a][b]
        assert_eq!(wide16, 1 + 4 + 1); // [pfx][op][aa][bb]
        assert_eq!(wide32, 1 + 8 + 1); // [pfx][op][aaaa][bbbb]
    }

    /// `opcodeLengths[id]` = operand-slot count (`generator/Opcode.rb:372-374`),
    /// matching the generated `macro(op_*, length)` lines
    /// (`WebKitBuild/.../Bytecodes.h:100-192`).
    #[test]
    fn opcode_lengths_match_bytecode_list() {
        assert_eq!(descriptor_for(ENTER).unwrap().opcode_length(), 0);
        assert_eq!(descriptor_for(MOV).unwrap().opcode_length(), 2);
        assert_eq!(descriptor_for(ADD).unwrap().opcode_length(), 5); // dst,lhs,rhs,profile,types
        assert_eq!(descriptor_for(MUL).unwrap().opcode_length(), 5); // group shape shared
        assert_eq!(descriptor_for(SUB).unwrap().opcode_length(), 5); // group shape shared
        assert_eq!(descriptor_for(EQ).unwrap().opcode_length(), 3);
        assert_eq!(descriptor_for(JMP).unwrap().opcode_length(), 1);
        assert_eq!(descriptor_for(JTRUE).unwrap().opcode_length(), 2);
        assert_eq!(descriptor_for(RET).unwrap().opcode_length(), 1);
        // Metadata rows count the +1 m_metadataID slot: `macro(op_get_by_id, 5)`
        // (id 20, 4 args), `macro(op_call, 6)` (id 25, 5 args),
        // `macro(op_jneq_ptr, 4)` (id 48, 3 args), and the longest row
        // `macro(op_iterator_next, 10)` (id 2, 9 args) — the
        // MAX_LENGTH_OF_BYTECODE_IDS witness.
        assert_eq!(descriptor_for(20).unwrap().name, "get_by_id");
        assert_eq!(descriptor_for(20).unwrap().opcode_length(), 5);
        assert_eq!(descriptor_for(25).unwrap().name, "call");
        assert_eq!(descriptor_for(25).unwrap().opcode_length(), 6);
        assert_eq!(descriptor_for(48).unwrap().name, "jneq_ptr");
        assert_eq!(descriptor_for(48).unwrap().opcode_length(), 4);
        assert_eq!(descriptor_for(2).unwrap().name, "iterator_next");
        assert_eq!(descriptor_for(2).unwrap().opcode_length(), 10);
    }

    /// FAITHFUL ID PIN: the re-derived `opcode_id` constants and the table
    /// rows carry JSC's REAL generated opcode IDs. Expected values come from
    /// the generated header the local C++ `jsc` was built from —
    /// `WebKitBuild/Release/DerivedSources/JavaScriptCore/Bytecodes.h`
    /// (`op_jmp_value_string "69"`, `op_jtrue_value_string "70"`,
    /// `op_ret_value_string "104"`, `op_wide16_value_string "128"`,
    /// `op_wide32_value_string "130"`, `op_enter_value_string "131"`,
    /// `op_mov_value_string "144"`, `op_eq_value_string "145"`,
    /// `op_add_value_string "158"`, `op_mul_value_string "159"`,
    /// `op_sub_value_string "161"`) — which the ID-assignment rule reproduces
    /// from `BytecodeList.rb` declaration order (`preserve_order: true`,
    /// `BytecodeList.rb:79-87`; `generator/DSL.rb:43-56`;
    /// `generator/Opcode.rb:41-47,59-61`). Any drift fails loudly here.
    #[test]
    fn opcode_ids_match_jsc_generated_values() {
        // The by-name-derived constants resolve to the pinned Bytecodes.h ids.
        assert_eq!(
            [JMP, JTRUE, RET, WIDE16, WIDE32, ENTER, MOV, EQ, ADD, MUL, SUB],
            [69, 70, 104, 128, 130, 131, 144, 145, 158, 159, 161]
        );
        let expected: &[(u8, &str)] = &[
            (69, "jmp"),
            (70, "jtrue"),
            (104, "ret"),
            (128, "wide16"),
            (130, "wide32"),
            (131, "enter"),
            (144, "mov"),
            (145, "eq"),
            (158, "add"),
            (159, "mul"),
            (161, "sub"),
        ];
        for &(id, name) in expected {
            let row = descriptor_for(id).unwrap();
            assert_eq!(row.id, id, "op_{name}");
            assert_eq!(row.name, name, "id {id}");
            // Exactly op_wide16/op_wide32 are width prefixes
            // (Instruction.h:40-41,81-89).
            assert_eq!(row.is_wide_prefix, id == WIDE16 || id == WIDE32);
        }
        // `div` (160) — undeclared in the old 11-row subset — is a full
        // generated row now, sharing the ProfiledBinaryOpWithOperandTypes
        // group shape (`BytecodeList.rb:1276-1292`).
        let div = descriptor_for(160).unwrap();
        assert_eq!(div.name, "div");
        assert_eq!(div.opcode_length(), 5);
        assert_eq!(div.operands, descriptor_for(ADD).unwrap().operands);
    }

    /// FULL-TABLE INVARIANTS, pinned against the generated `Bytecodes.h`
    /// constants the artifact-verified table carries: 193 dense ascending
    /// ids; the 49 metadata rows are exactly the id prefix 0..49
    /// (`hasMetadata()` = `opcodeID < numberOfBytecodesWithMetadata`,
    /// `Instruction.h:98-101`); the 7 checkpoint rows are exactly the id
    /// prefix 0..7 (`generator/Section.rb:72-97` partition validation);
    /// wide16=128/wide32=130 are the ONLY width prefixes
    /// (`Instruction.h:40-41,81-89`); max `opcodeLengths` entry is
    /// MAX_LENGTH_OF_BYTECODE_IDS.
    #[test]
    fn full_table_invariants_match_generated_constants() {
        assert_eq!(NUMBER_OF_BYTECODE_IDS, 193);
        assert_eq!(NUMBER_OF_BYTECODE_WITH_METADATA, 49);
        assert_eq!(NUMBER_OF_BYTECODE_WITH_CHECKPOINTS, 7);
        assert_eq!(MAX_LENGTH_OF_BYTECODE_IDS, 10);
        assert_eq!(OPCODE_TABLE.len(), NUMBER_OF_BYTECODE_IDS);

        let mut max_length = 0usize;
        for (index, row) in OPCODE_TABLE.iter().enumerate() {
            let name = row.name;
            // Ids are the dense, strictly ascending 0..193: position IS id.
            assert_eq!(row.id as usize, index, "op_{name}");
            assert!(
                std::ptr::eq(descriptor_for(row.id).unwrap(), row),
                "op_{name}"
            );
            // Metadata prefix partition.
            assert_eq!(
                row.has_metadata,
                (row.id as usize) < NUMBER_OF_BYTECODE_WITH_METADATA,
                "op_{name}"
            );
            // Checkpoint prefix partition.
            assert_eq!(
                row.num_checkpoints > 0,
                (row.id as usize) < NUMBER_OF_BYTECODE_WITH_CHECKPOINTS,
                "op_{name}"
            );
            // The two width prefixes and nothing else.
            assert_eq!(row.is_wide_prefix, row.id == WIDE16 || row.id == WIDE32);
            if row.opcode_length() > max_length {
                max_length = row.opcode_length();
            }
        }
        assert_eq!(
            OPCODE_TABLE.iter().filter(|row| row.has_metadata).count(),
            NUMBER_OF_BYTECODE_WITH_METADATA
        );
        assert_eq!(
            OPCODE_TABLE
                .iter()
                .filter(|row| row.num_checkpoints > 0)
                .count(),
            NUMBER_OF_BYTECODE_WITH_CHECKPOINTS
        );
        assert_eq!(max_length, MAX_LENGTH_OF_BYTECODE_IDS);
        assert_eq!(descriptor_for(WIDE16).unwrap().name, "wide16");
        assert_eq!(descriptor_for(WIDE32).unwrap().name, "wide32");
        // Past the table end: no descriptor.
        assert_eq!(descriptor_for(NUMBER_OF_BYTECODE_IDS as u8), None);
        assert_eq!(descriptor_for(u8::MAX), None);
    }

    /// In-domain narrow-fitting representative per stream operand kind (the
    /// C++ value domains are cited on the matching `OperandValue` variants).
    fn narrow_representative(kind: OperandKind) -> OperandValue {
        match kind {
            OperandKind::VirtualRegister => OperandValue::VirtualRegister(-1), // local(0)
            OperandKind::UnsignedImmediate => OperandValue::UnsignedImmediate(7),
            OperandKind::SignedImmediate => OperandValue::SignedImmediate(-7),
            OperandKind::Bool => OperandValue::Bool(true),
            OperandKind::OperandTypes => OperandValue::OperandTypes(0x0201),
            OperandKind::BoundLabel => OperandValue::BoundLabel(-7),
            OperandKind::ECMAMode => OperandValue::ECMAMode(1),
            OperandKind::IndexingType => OperandValue::IndexingType(5),
            OperandKind::SymbolTableOrScopeDepth => OperandValue::SymbolTableOrScopeDepth(3),
            OperandKind::ResolveType => OperandValue::ResolveType(13),
            OperandKind::GetPutInfo => {
                OperandValue::GetPutInfo((1 << 30) | (1 << 20) | (2 << 10) | 13)
            }
            OperandKind::PutByIdFlags => OperandValue::PutByIdFlags(0b11),
            OperandKind::ProfileTypeBytecodeFlag => OperandValue::ProfileTypeBytecodeFlag(4),
            OperandKind::PrivateFieldPutKind => OperandValue::PrivateFieldPutKind(2),
            OperandKind::ErrorTypeWithExtension => OperandValue::ErrorTypeWithExtension(3),
            OperandKind::DebugHookType => OperandValue::DebugHookType(8),
            OperandKind::ResultType => OperandValue::ResultType(0x7e),
            OperandKind::JSType => OperandValue::JSType(20),
        }
    }

    /// A value of `kind` that fails every width below `target` and fits
    /// `target`, so ONE such operand forces the whole instruction to
    /// `target` (the shared-width rule) — or `None` for the kinds whose
    /// `Fits` check passes narrower widths for every value (u8-backed kinds;
    /// GetPutInfo, whose narrow and wide16 checks are the same compressed
    /// check, `Fits.h:206-212`; OperandTypes at wide32, since its wide16
    /// same-size `bit_cast` accepts any bits, `Fits.h:52-64`).
    fn width_forcing_value(kind: OperandKind, target: OpcodeSize) -> Option<OperandValue> {
        let wide16 = matches!(target, OpcodeSize::Wide16);
        Some(match kind {
            OperandKind::VirtualRegister => {
                OperandValue::VirtualRegister(if wide16 { -129 } else { -32_769 })
            }
            OperandKind::UnsignedImmediate => {
                OperandValue::UnsignedImmediate(if wide16 { 5000 } else { 100_000 })
            }
            OperandKind::SignedImmediate => {
                OperandValue::SignedImmediate(if wide16 { 5000 } else { 100_000 })
            }
            OperandKind::BoundLabel => {
                OperandValue::BoundLabel(if wide16 { 5000 } else { 100_000 })
            }
            OperandKind::SymbolTableOrScopeDepth => {
                OperandValue::SymbolTableOrScopeDepth(if wide16 { 5000 } else { 100_000 })
            }
            // A ResultType outside the 4-bit narrow nibble pack
            // (`Fits.h:311-323`) rides the wide16 raw `bits()`.
            OperandKind::OperandTypes if wide16 => OperandValue::OperandTypes(0x0030),
            _ => return None,
        })
    }

    /// FULL-TABLE EMIT->DECODE ROUND-TRIP: every generated opcode, at every
    /// operand width the `Fits` cascade can select for it, encodes through
    /// the writer and decodes back identically — id, name, width, the
    /// `Instruction.h:138-145` size, and every operand value (including the
    /// trailing m_metadataID slot of the 49 metadata rows, emitted as a plain
    /// `unsigned`, `generator/Metadata.rb:126-131`).
    #[test]
    fn every_generated_opcode_round_trips_emit_decode_at_each_width() {
        for descriptor in OPCODE_TABLE.iter() {
            // op_wide16/op_wide32 are width PREFIXES, not standalone
            // instructions (`Instruction.h:81-89`): the writer only ever
            // emits them as the prefix byte of a wide instruction.
            if descriptor.is_wide_prefix {
                continue;
            }
            for target in ALL_WIDTHS {
                let mut operands: Vec<OperandValue> = (0..descriptor.opcode_length())
                    .map(|slot| narrow_representative(descriptor.operand_kind(slot)))
                    .collect();
                if !matches!(target, OpcodeSize::Narrow) {
                    let widenable = (0..descriptor.opcode_length()).find(|&slot| {
                        width_forcing_value(descriptor.operand_kind(slot), target).is_some()
                    });
                    let Some(slot) = widenable else {
                        // Only the zero-operand ops (enter/nop/loop_hint/...)
                        // have nothing to widen; the Fits cascade always
                        // emits them narrow, like C++.
                        assert_eq!(descriptor.opcode_length(), 0, "op_{}", descriptor.name);
                        continue;
                    };
                    operands[slot] =
                        width_forcing_value(descriptor.operand_kind(slot), target).unwrap();
                }
                let mut writer = InstructionStreamWriter::new();
                assert_eq!(writer.emit(descriptor.id, &operands), 0);
                let stream = writer.finalize();
                let decoded = decode_raw_instruction(stream.bytes(), 0)
                    .unwrap_or_else(|error| panic!("op_{} {target:?}: {error:?}", descriptor.name));
                assert_eq!(decoded.opcode_id, descriptor.id, "op_{}", descriptor.name);
                assert_eq!(decoded.name, descriptor.name);
                assert_eq!(decoded.width, target, "op_{}", descriptor.name);
                assert_eq!(
                    decoded.size,
                    OPCODE_ID_BYTES
                        + descriptor.opcode_length() * target.operand_bytes()
                        + target.prefix_bytes(),
                    "op_{} {target:?}",
                    descriptor.name
                );
                assert_eq!(decoded.size, stream.size_in_bytes());
                let expected: Vec<i64> = operands.iter().map(|value| value.as_i64()).collect();
                assert_eq!(
                    decoded.operands, expected,
                    "op_{} {target:?}",
                    descriptor.name
                );
            }
        }
    }

    /// `Fits` width selection: one out-of-narrow-range operand widens the WHOLE
    /// instruction; locals that fit narrow are still emitted wide.
    #[test]
    fn width_selection_is_per_instruction_shared() {
        // All operands fit narrow.
        assert_eq!(
            select_width(&[
                OperandValue::VirtualRegister(-1),
                OperandValue::VirtualRegister(-2),
            ]),
            OpcodeSize::Narrow
        );
        // profileIndex (a plain `unsigned` arg) 5000 overflows int8/uint8 ->
        // Wide16 for every field.
        assert_eq!(
            select_width(&[
                OperandValue::VirtualRegister(-1),
                OperandValue::VirtualRegister(-2),
                OperandValue::VirtualRegister(-3),
                OperandValue::UnsignedImmediate(5000),
                OperandValue::OperandTypes(0),
            ]),
            OpcodeSize::Wide16
        );
        // profileIndex 100000 overflows uint16 -> Wide32.
        assert_eq!(
            select_width(&[
                OperandValue::VirtualRegister(-1),
                OperandValue::VirtualRegister(-2),
                OperandValue::VirtualRegister(-3),
                OperandValue::UnsignedImmediate(100_000),
                OperandValue::OperandTypes(0),
            ]),
            OpcodeSize::Wide32
        );
    }

    /// THE STRONG TEST: write `enter; mov; add(wide16); add(wide32); jtrue->ret;
    /// ret`, then walk the packed bytes and prove:
    ///   (a) decoded opcode + operands equal what was written,
    ///   (b) `next()` advances by exactly the encoded `size()`,
    ///   (c) each instruction's BYTE OFFSET equals the running byte position and
    ///       is NOT its ordinal,
    ///   (d) the jump target resolves to the correct BYTE OFFSET.
    #[test]
    fn round_trip_packed_stream_byte_offsets_and_widths() {
        let mut writer = InstructionStreamWriter::new();

        // enter  (narrow, size 1)
        let enter_at = writer.emit(ENTER, &[]);
        // mov local(0), local(1)  (narrow, size 3)
        let mov_at = writer.emit(
            MOV,
            &[
                OperandValue::VirtualRegister(-1),
                OperandValue::VirtualRegister(-2),
            ],
        );
        // add ... profile=5000 -> wide16 (size 1+5*2+1 = 12)
        let add16_at = writer.emit(
            ADD,
            &[
                OperandValue::VirtualRegister(-1),
                OperandValue::VirtualRegister(-2),
                OperandValue::VirtualRegister(-3),
                OperandValue::UnsignedImmediate(5000),
                OperandValue::OperandTypes(0x0102),
            ],
        );
        // add ... profile=100000 -> wide32 (size 1+5*4+1 = 22)
        let add32_at = writer.emit(
            ADD,
            &[
                OperandValue::VirtualRegister(-1),
                OperandValue::VirtualRegister(-2),
                OperandValue::VirtualRegister(-3),
                OperandValue::UnsignedImmediate(100_000),
                OperandValue::OperandTypes(0x0304),
            ],
        );
        // jtrue local(0), target=<ret>  (forward jump; patched after ret).
        let jtrue_at = writer.emit(
            JTRUE,
            &[
                OperandValue::VirtualRegister(-1),
                OperandValue::BoundLabel(0), // placeholder, fits narrow
            ],
        );
        // ret local(0)  (narrow, size 2)
        let ret_at = writer.emit(RET, &[OperandValue::VirtualRegister(-1)]);

        // Resolve the forward jump: store the relative delta target-start.
        let delta = (ret_at as i64) - (jtrue_at as i64);
        writer.set_operand(jtrue_at, 1, OperandValue::BoundLabel(delta as i32));

        // Expected byte offsets (NOT ordinals).
        assert_eq!(enter_at, 0);
        assert_eq!(mov_at, 1); // after enter (1)
        assert_eq!(add16_at, 4); // after mov (3)
        assert_eq!(add32_at, 16); // after add16 (12)
        assert_eq!(jtrue_at, 38); // after add32 (22)
        assert_eq!(ret_at, 41); // after jtrue (3)

        let stream = writer.finalize();
        assert_eq!(stream.size_in_bytes(), 43); // ret (2) ends at 43

        // (b)+(c): walk via the cursor; byte offset == running position.
        let refs: Vec<Ref<'_>> = stream.cursor().collect();
        assert_eq!(refs.len(), 6);
        let mut running = 0usize;
        for (ordinal, r) in refs.iter().enumerate() {
            assert_eq!(r.offset(), running, "byte offset must equal running pos");
            // (c) BYTE OFFSET is not the ordinal once widths differ.
            if ordinal >= 2 {
                assert_ne!(
                    r.offset(),
                    ordinal,
                    "byte offset must diverge from ordinal index"
                );
            }
            // bytecode_index() carries the byte offset, not the ordinal.
            assert_eq!(r.bytecode_index().offset() as usize, r.offset());
            // (b) next() advances by exactly size().
            assert_eq!(r.next().offset(), r.offset() + r.size());
            running += r.size();
        }
        assert_eq!(running, stream.size_in_bytes());

        // (a) decode each instruction and check opcode + operands + width.
        let enter = stream.at_offset(enter_at);
        assert!(enter.is(ENTER));
        assert_eq!(enter.width(), OpcodeSize::Narrow);
        assert_eq!(enter.operand_count(), 0);
        assert_eq!(enter.size(), 1);

        let mov = stream.at_offset(mov_at);
        assert!(mov.is(MOV));
        assert_eq!(mov.width(), OpcodeSize::Narrow);
        assert_eq!(mov.operand(0), -1);
        assert_eq!(mov.operand(1), -2);
        assert_eq!(mov.size(), 3);

        let add16 = stream.at_offset(add16_at);
        assert!(add16.is(ADD));
        assert_eq!(add16.width(), OpcodeSize::Wide16);
        assert!(add16.is_wide16());
        assert_eq!(add16.operand(0), -1);
        assert_eq!(add16.operand(1), -2);
        assert_eq!(add16.operand(2), -3);
        assert_eq!(add16.operand(3), 5000); // unsigned profile, zero-extended
        assert_eq!(add16.operand(4), 0x0102);
        assert_eq!(add16.size(), 12);

        let add32 = stream.at_offset(add32_at);
        assert!(add32.is(ADD));
        assert_eq!(add32.width(), OpcodeSize::Wide32);
        assert!(add32.is_wide32());
        assert_eq!(add32.operand(3), 100_000);
        assert_eq!(add32.operand(4), 0x0304);
        assert_eq!(add32.size(), 22);

        let jtrue = stream.at_offset(jtrue_at);
        assert!(jtrue.is(JTRUE));
        assert_eq!(jtrue.width(), OpcodeSize::Narrow);
        // condition operand
        assert_eq!(jtrue.operand(0), -1);
        // (d) jump target resolves to ret's BYTE OFFSET; the operand is the stored delta.
        assert_eq!(jtrue.operand(1), delta);
        assert_eq!(jtrue.jump_target(1), ret_at);

        let ret = stream.at_offset(ret_at);
        assert!(ret.is(RET));
        assert_eq!(ret.operand(0), -1);
        assert_eq!(ret.size(), 2);

        // at(BytecodeIndex) resolves by BYTE OFFSET, mirroring at(index.offset()).
        let by_index = stream.at(jtrue.bytecode_index());
        assert_eq!(by_index.offset(), jtrue_at);
        assert!(by_index.is(JTRUE));
    }

    /// `seek`/`rewind`-style reposition: a placeholder operand can be patched in
    /// place without disturbing neighbouring instructions.
    #[test]
    fn set_operand_patches_in_place() {
        let mut writer = InstructionStreamWriter::new();
        let jmp_at = writer.emit(JMP, &[OperandValue::BoundLabel(0)]);
        let ret_at = writer.emit(RET, &[OperandValue::VirtualRegister(-1)]);
        let delta = (ret_at as i64) - (jmp_at as i64);
        writer.set_operand(jmp_at, 0, OperandValue::BoundLabel(delta as i32));
        let stream = writer.finalize();
        let jmp = stream.at_offset(jmp_at);
        assert_eq!(jmp.jump_target(0), ret_at);
        // The neighbouring ret is untouched.
        assert!(stream.at_offset(ret_at).is(RET));
        assert_eq!(stream.at_offset(ret_at).operand(0), -1);
    }

    /// JSC-derived byte FIXTURE (not writer output): decode hand-encoded bytes
    /// laid out exactly per the C++ layout so the test proves JSC's encoding,
    /// not the Rust writer's.
    ///
    /// Layout per `Instruction.h:181-198` narrow form `[opcode][operands...]`,
    /// one byte per narrow operand, opcode always one byte (`Opcode.h:86-87`);
    /// `Fits<VirtualRegister, Narrow>::convert` (`Fits.h:118-156` /
    /// `BytecodeConventions.h:35`): local(0) = -1 -> 0xff (two's complement),
    /// constant(0) -> s_firstConstantIndex(16) + 0 = 0x10.
    ///
    ///   mov local0, constant0   = [MOV, 0xff, 0x10]   (size 3)
    ///   ret local0              = [RET, 0xff]         (size 2)
    #[test]
    fn hand_encoded_jsc_layout_fixture_decodes_and_gates_instruction_starts() {
        let bytes = [MOV, 0xff, 0x10, RET, 0xff];

        let mov = decode_raw_instruction(&bytes, 0).expect("mov decodes at start 0");
        assert_eq!(mov.opcode_id, MOV);
        assert_eq!(mov.width, OpcodeSize::Narrow);
        assert_eq!(mov.size, 3);
        assert_eq!(mov.operands[0], -1); // local(0)
        assert_eq!(mov.operands[1], i64::from(FIRST_CONSTANT_REGISTER_INDEX)); // constant(0)

        let ret = decode_raw_instruction(&bytes, 3).expect("ret decodes at start 3");
        assert_eq!(ret.opcode_id, RET);
        assert_eq!(ret.size, 2);
        assert_eq!(ret.operands[0], -1);

        // Instruction starts are exactly {0, 3}; every other offset — the
        // operand bytes at 1, 2 (0xff, 0x10) and 4 (0xff) — must be rejected,
        // never decoded mid-instruction.
        for offset in 0..=bytes.len() + 1 {
            assert_eq!(
                is_instruction_start(&bytes, offset).expect("well-formed stream walks"),
                offset == 0 || offset == 3,
                "offset {offset}"
            );
        }

        // Wide16 fixture: `[op_wide16][MOV][dst.le16][src.le16]`
        // (`Instruction.h:181-198`, prefix per `OpcodeSize.h:63-76`).
        // local(128) = -129 -> 0xff7f LE [0x7f, 0xff];
        // constant(0) -> Fits<VirtualRegister, Wide16> band start 64 = 0x0040 LE
        // [0x40, 0x00] (`BytecodeConventions.h:36`).
        let wide16 = [WIDE16, MOV, 0x7f, 0xff, 0x40, 0x00];
        let mov16 = decode_raw_instruction(&wide16, 0).expect("wide16 mov decodes");
        assert_eq!(mov16.opcode_id, MOV);
        assert_eq!(mov16.width, OpcodeSize::Wide16);
        assert_eq!(mov16.size, 6);
        assert_eq!(mov16.operands[0], -129);
        assert_eq!(mov16.operands[1], i64::from(FIRST_CONSTANT_REGISTER_INDEX));
        // The prefix byte and the interior opcode byte are NOT starts.
        assert!(is_instruction_start(&wide16, 0).unwrap());
        for offset in 1..wide16.len() {
            assert!(!is_instruction_start(&wide16, offset).unwrap());
        }
    }

    /// JSC-derived byte FIXTURE: `[enter][add narrow][ret]` hand-encoded per
    /// the C++ layout (`Instruction.h:181-198` narrow form
    /// `[opcode][operands...]`, opcode always one byte per `Opcode.h:86-87`).
    /// `op_add`'s operands are dst, lhs, rhs, profileIndex, operandTypes
    /// (`BytecodeList.rb:1276-1292`), one byte each in narrow form. The
    /// narrow operandTypes byte is the `Fits<OperandTypes, Narrow>` 4-bit
    /// pack `(first << 4) | second` (`Fits.h:325-338`): byte 0x12 = first
    /// TypeInt32(0x01), second TypeMaybeNumber(0x02), i.e. `bits()` 0x0201.
    ///   offset 0: enter                          = [ENTER]           (size 1)
    ///   offset 1: add local0, local1, constant0,
    ///             profileIndex=7, operandTypes bits=0x0201
    ///             = [ADD, 0xff, 0xfe, 0x10, 0x07, 0x12]              (size 6)
    ///   offset 7: ret local0                     = [RET, 0xff]       (size 2)
    #[test]
    fn hand_encoded_enter_add_ret_stream_decodes_with_full_operand_shape() {
        let bytes = [ENTER, ADD, 0xff, 0xfe, 0x10, 0x07, 0x12, RET, 0xff];

        let enter = decode_raw_instruction(&bytes, 0).expect("enter decodes at 0");
        assert_eq!(enter.opcode_id, ENTER);
        assert_eq!(enter.name, "enter");
        assert_eq!(enter.width, OpcodeSize::Narrow);
        assert_eq!(enter.size, 1);
        assert!(enter.operands.is_empty());

        let add = decode_raw_instruction(&bytes, 1).expect("add decodes at 1");
        assert_eq!(add.opcode_id, ADD);
        assert_eq!(add.width, OpcodeSize::Narrow);
        assert_eq!(add.size, 6);
        assert_eq!(add.operands[0], -1); // dst = local(0), two's complement 0xff
        assert_eq!(add.operands[1], -2); // lhs = local(1)
        assert_eq!(add.operands[2], i64::from(FIRST_CONSTANT_REGISTER_INDEX)); // rhs = constant(0)
        assert_eq!(add.operands[3], 7); // profileIndex, unsigned zero-extend

        // operandTypes: narrow byte 0x12 unpacks to bits() 0x0201
        // (`Fits.h:340-352`; first = low byte). The old expectation 0x12 was
        // an accidental raw-truncation divergence, corrected to the C++
        // nibble unpack.
        assert_eq!(add.operands[4], 0x0201);

        let ret = decode_raw_instruction(&bytes, 7).expect("ret decodes at 7");
        assert_eq!(ret.opcode_id, RET);
        assert_eq!(ret.size, 2);
        assert_eq!(ret.operands[0], -1);

        // Instruction starts are exactly {0, 1, 7}; every mid-instruction
        // offset is rejected, never decoded.
        for offset in 0..=bytes.len() + 1 {
            assert_eq!(
                is_instruction_start(&bytes, offset).expect("well-formed stream walks"),
                matches!(offset, 0 | 1 | 7),
                "offset {offset}"
            );
        }
    }

    /// The ProfiledBinaryOpWithOperandTypes group shares ONE operand shape
    /// (`BytecodeList.rb:1276-1292`): `sub`/`mul` decode exactly like `add`
    /// with only the opcode id/name differing, in every width. Wide fixtures
    /// are hand-encoded per `Instruction.h:181-198` (`[prefix][opcode]` then
    /// little-endian operand fields) with the per-width constant bands of
    /// `Fits<VirtualRegister>` (`Fits.h:118-156`; band starts 16/64/raw per
    /// `BytecodeConventions.h:35-37`).
    #[test]
    fn sub_and_mul_decode_with_adds_group_shape_across_widths() {
        // (b) Narrow: identical operand bytes, only the opcode byte differs.
        let operand_bytes = [0xff, 0xfe, 0x10, 0x07, 0x12];
        let encode = |id: u8| -> Vec<u8> { std::iter::once(id).chain(operand_bytes).collect() };
        let add = decode_raw_instruction(&encode(ADD), 0).expect("add");
        let sub = decode_raw_instruction(&encode(SUB), 0).expect("sub");
        let mul = decode_raw_instruction(&encode(MUL), 0).expect("mul");
        assert_eq!(sub.opcode_id, SUB);
        assert_eq!(mul.opcode_id, MUL);
        assert_eq!(sub.operands, add.operands);
        assert_eq!(mul.operands, add.operands);
        assert_eq!(sub.size, add.size);
        assert_eq!(mul.size, add.size);
        assert_eq!(sub.width, OpcodeSize::Narrow);

        // Wide16 sub: [op_wide16][sub][dst.le16][lhs.le16][rhs.le16]
        // [profileIndex.le16][operandTypes.le16]; constant band start 64
        // (FirstConstantRegisterIndex16). profileIndex 5000 = 0x1388.
        let sub16 = [
            WIDE16, SUB, 0xff, 0xff, 0xfe, 0xff, 0x40, 0x00, 0x88, 0x13, 0x02, 0x01,
        ];
        let sub16 = decode_raw_instruction(&sub16, 0).expect("wide16 sub decodes");
        assert_eq!(sub16.opcode_id, SUB);
        assert_eq!(sub16.width, OpcodeSize::Wide16);
        assert_eq!(sub16.size, 12); // 1 prefix + 1 opcode + 5*2
        assert_eq!(sub16.operands[0], -1);
        assert_eq!(sub16.operands[1], -2);
        assert_eq!(sub16.operands[2], i64::from(FIRST_CONSTANT_REGISTER_INDEX));
        assert_eq!(sub16.operands[3], 5000);
        assert_eq!(sub16.operands[4], 0x0102);

        // Wide32 mul: [op_wide32][mul][5 x le32]; wide32 stores the raw
        // VirtualRegister namespace (constants at FirstConstantRegisterIndex =
        // 0x40000000). profileIndex 100000 = 0x000186a0.
        let mul32 = [
            WIDE32, MUL, 0xff, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x40,
            0xa0, 0x86, 0x01, 0x00, 0x02, 0x01, 0x00, 0x00,
        ];
        let mul32 = decode_raw_instruction(&mul32, 0).expect("wide32 mul decodes");
        assert_eq!(mul32.opcode_id, MUL);
        assert_eq!(mul32.width, OpcodeSize::Wide32);
        assert_eq!(mul32.size, 22); // 1 prefix + 1 opcode + 5*4
        assert_eq!(mul32.operands[0], -1);
        assert_eq!(mul32.operands[1], -2);
        assert_eq!(mul32.operands[2], i64::from(FIRST_CONSTANT_REGISTER_INDEX));
        assert_eq!(mul32.operands[3], 100_000);
        assert_eq!(mul32.operands[4], 0x0102);

        // (d) Multi-size walk: enter(1) + add narrow(6) + sub wide16(12) +
        // mul wide32(22) + ret(2) — instruction starts are exactly the
        // running byte offsets {0, 1, 7, 19, 41}.
        let mut stream = vec![ENTER, ADD, 0xff, 0xfe, 0x10, 0x07, 0x12];
        stream.extend_from_slice(&[
            WIDE16, SUB, 0xff, 0xff, 0xfe, 0xff, 0x40, 0x00, 0x88, 0x13, 0x02, 0x01,
        ]);
        stream.extend_from_slice(&[
            WIDE32, MUL, 0xff, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x40,
            0xa0, 0x86, 0x01, 0x00, 0x02, 0x01, 0x00, 0x00,
        ]);
        stream.extend_from_slice(&[RET, 0xff]);
        assert_eq!(stream.len(), 43);
        for offset in 0..=stream.len() + 1 {
            assert_eq!(
                is_instruction_start(&stream, offset).expect("well-formed stream walks"),
                matches!(offset, 0 | 1 | 7 | 19 | 41),
                "offset {offset}"
            );
        }
        // The writer agrees with the hand encoding for the widened rows.
        let mut writer = InstructionStreamWriter::new();
        assert_eq!(writer.emit(ENTER, &[]), 0);
        assert_eq!(
            writer.emit(
                ADD,
                &[
                    OperandValue::VirtualRegister(-1),
                    OperandValue::VirtualRegister(-2),
                    OperandValue::VirtualRegister(FIRST_CONSTANT_REGISTER_INDEX),
                    OperandValue::UnsignedImmediate(7),
                    // bits() 0x0201 narrow-packs to the byte 0x12
                    // (`Fits.h:325-338`), matching the hand fixture.
                    OperandValue::OperandTypes(0x0201),
                ],
            ),
            1
        );
        assert_eq!(
            writer.emit(
                SUB,
                &[
                    OperandValue::VirtualRegister(-1),
                    OperandValue::VirtualRegister(-2),
                    OperandValue::VirtualRegister(FIRST_CONSTANT_REGISTER_INDEX),
                    OperandValue::UnsignedImmediate(5000),
                    OperandValue::OperandTypes(0x0102),
                ],
            ),
            7
        );
        assert_eq!(
            writer.emit(
                MUL,
                &[
                    OperandValue::VirtualRegister(-1),
                    OperandValue::VirtualRegister(-2),
                    OperandValue::VirtualRegister(FIRST_CONSTANT_REGISTER_INDEX),
                    OperandValue::UnsignedImmediate(100_000),
                    OperandValue::OperandTypes(0x0102),
                ],
            ),
            19
        );
        assert_eq!(writer.emit(RET, &[OperandValue::VirtualRegister(-1)]), 41);
        assert_eq!(writer.finalize().bytes(), stream.as_slice());
    }

    /// The id->CoreOpcode dispatch bridge lives in the ONE canonical opcode
    /// table: exactly mov/ret are executable from raw packed bytes, and their
    /// `CoreOpcode` identities match the wedge contract.
    #[test]
    fn opcode_table_core_bridge_is_mov_and_ret_only() {
        for descriptor in OPCODE_TABLE {
            let expected = match descriptor.id {
                MOV => Some(CoreOpcode::Move),
                RET => Some(CoreOpcode::Return),
                _ => None,
            };
            assert_eq!(descriptor.core, expected, "opcode {}", descriptor.name);
            assert_eq!(
                CoreOpcode::from_packed_opcode_id(descriptor.id),
                expected,
                "opcode {}",
                descriptor.name
            );
        }
    }

    #[test]
    fn virtual_register_constant_remap_round_trips_per_width() {
        let constant0 = FIRST_CONSTANT_REGISTER_INDEX;
        let local0 = -1;

        let mut narrow_writer = InstructionStreamWriter::new();
        narrow_writer.emit(
            MOV,
            &[
                OperandValue::VirtualRegister(local0),
                OperandValue::VirtualRegister(constant0),
            ],
        );
        let narrow = narrow_writer.finalize();
        assert_eq!(
            narrow.bytes(),
            &[MOV, 0xff, FIRST_CONSTANT_REGISTER_INDEX8 as u8]
        );
        let narrow_mov = narrow.at_offset(0);
        assert_eq!(narrow_mov.width(), OpcodeSize::Narrow);
        assert_eq!(narrow_mov.operand(0), i64::from(local0));
        assert_eq!(narrow_mov.operand(1), i64::from(constant0));

        let wide16_local = -129;
        let mut wide16_writer = InstructionStreamWriter::new();
        wide16_writer.emit(
            MOV,
            &[
                OperandValue::VirtualRegister(wide16_local),
                OperandValue::VirtualRegister(constant0),
            ],
        );
        let wide16 = wide16_writer.finalize();
        assert_eq!(wide16.bytes()[0], WIDE16);
        let wide16_mov = wide16.at_offset(0);
        assert_eq!(wide16_mov.width(), OpcodeSize::Wide16);
        assert_eq!(wide16_mov.operand(0), i64::from(wide16_local));
        assert_eq!(wide16_mov.operand(1), i64::from(constant0));
        assert_eq!(
            read_unsigned_le(wide16.bytes(), 4, OpcodeSize::Wide16.operand_bytes()),
            FIRST_CONSTANT_REGISTER_INDEX16 as u64
        );

        let wide32_local = -32_769;
        let mut wide32_writer = InstructionStreamWriter::new();
        wide32_writer.emit(
            MOV,
            &[
                OperandValue::VirtualRegister(wide32_local),
                OperandValue::VirtualRegister(constant0),
            ],
        );
        let wide32 = wide32_writer.finalize();
        assert_eq!(wide32.bytes()[0], WIDE32);
        let wide32_mov = wide32.at_offset(0);
        assert_eq!(wide32_mov.width(), OpcodeSize::Wide32);
        assert_eq!(wide32_mov.operand(0), i64::from(wide32_local));
        assert_eq!(wide32_mov.operand(1), i64::from(constant0));
        assert_eq!(
            read_unsigned_le(wide32.bytes(), 6, OpcodeSize::Wide32.operand_bytes()),
            FIRST_CONSTANT_REGISTER_INDEX as u32 as u64
        );
    }

    /// Encode->decode one operand at a fixed width — the
    /// `Fits<T>::convert(T)` / `Fits<T>::convert(TargetType)` pair.
    fn round_trip(value: OperandValue, width: OpcodeSize) -> i64 {
        fits_decode(value.kind(), value.fits_convert(width), width)
    }

    const ALL_WIDTHS: [OpcodeSize; 3] =
        [OpcodeSize::Narrow, OpcodeSize::Wide16, OpcodeSize::Wide32];

    /// `unsigned` / `int` immediates hit the exact `Fits<integral>` bounds
    /// (`Fits.h:66-85`): unsigned saturates the width's unsigned max and
    /// zero-extends back; int uses the signed two's-complement range and
    /// sign-extends back.
    #[test]
    fn fits_round_trips_unsigned_and_signed_immediates_at_boundaries() {
        // Max narrow unsigned is 255; 256 widens; 65535 is the wide16 max.
        assert_eq!(
            select_width(&[OperandValue::UnsignedImmediate(255)]),
            OpcodeSize::Narrow
        );
        assert_eq!(
            round_trip(OperandValue::UnsignedImmediate(255), OpcodeSize::Narrow),
            255
        );
        assert_eq!(
            select_width(&[OperandValue::UnsignedImmediate(256)]),
            OpcodeSize::Wide16
        );
        assert_eq!(
            select_width(&[OperandValue::UnsignedImmediate(65_535)]),
            OpcodeSize::Wide16
        );
        assert_eq!(
            round_trip(OperandValue::UnsignedImmediate(65_535), OpcodeSize::Wide16),
            65_535
        );
        assert_eq!(
            select_width(&[OperandValue::UnsignedImmediate(65_536)]),
            OpcodeSize::Wide32
        );
        assert_eq!(
            round_trip(
                OperandValue::UnsignedImmediate(u32::MAX),
                OpcodeSize::Wide32
            ),
            i64::from(u32::MAX)
        );

        // `int`: int8 bounds [-128, 127], int16 bounds [-32768, 32767].
        assert_eq!(
            select_width(&[OperandValue::SignedImmediate(127)]),
            OpcodeSize::Narrow
        );
        assert_eq!(
            select_width(&[OperandValue::SignedImmediate(-128)]),
            OpcodeSize::Narrow
        );
        assert_eq!(
            round_trip(OperandValue::SignedImmediate(-128), OpcodeSize::Narrow),
            -128
        );
        assert_eq!(
            select_width(&[OperandValue::SignedImmediate(128)]),
            OpcodeSize::Wide16
        );
        assert_eq!(
            select_width(&[OperandValue::SignedImmediate(-129)]),
            OpcodeSize::Wide16
        );
        assert_eq!(
            round_trip(OperandValue::SignedImmediate(-32_768), OpcodeSize::Wide16),
            -32_768
        );
        assert_eq!(
            select_width(&[OperandValue::SignedImmediate(-32_769)]),
            OpcodeSize::Wide32
        );
        assert_eq!(
            round_trip(OperandValue::SignedImmediate(i32::MIN), OpcodeSize::Wide32),
            i64::from(i32::MIN)
        );
        // A negative int is stored two's complement and re-extended at EVERY
        // width, like the BoundLabel backward-jump delta.
        for width in ALL_WIDTHS {
            assert_eq!(round_trip(OperandValue::SignedImmediate(-7), width), -7);
            assert_eq!(round_trip(OperandValue::BoundLabel(-7), width), -7);
        }
    }

    /// Every uint8-backed stream type always passes the narrow `Fits` check
    /// (same-size `bit_cast` at Narrow, `Fits.h:52-64`; uint8->uint16/32
    /// upcast when wide, `Fits.h:66-85`) and round-trips zero-extended:
    /// bool (`Fits.h:87-103`), ECMAMode (`Fits.h:381-399`, value 0 = strict),
    /// IndexingType (`IndexingType.h:63`), PrivateFieldPutKind
    /// (`Fits.h:401-419`), ErrorTypeWithExtension (`ErrorType.h:59`),
    /// ResultType (`Fits.h:287-298`), JSType (`JSType.h:164`), and the 2-bit
    /// PutByIdFlags pack (`Fits.h:234-267`, check always true).
    #[test]
    fn bool_and_u8_backed_kinds_always_fit_narrow_and_round_trip() {
        let values = [
            OperandValue::Bool(true),
            OperandValue::Bool(false),
            OperandValue::ECMAMode(0),
            OperandValue::ECMAMode(1),
            OperandValue::IndexingType(255), // max narrow unsigned boundary
            OperandValue::PrivateFieldPutKind(2), // Define
            OperandValue::ErrorTypeWithExtension(3),
            OperandValue::ResultType(0x7e), // unknownType bits
            OperandValue::JSType(255),
            OperandValue::PutByIdFlags(0b11), // strict | direct
        ];
        for value in values {
            assert_eq!(select_width(&[value]), OpcodeSize::Narrow, "{value:?}");
            for width in ALL_WIDTHS {
                assert_eq!(
                    round_trip(value, width),
                    value.as_i64(),
                    "{value:?} at {width:?}"
                );
            }
        }
        // bool stores 0/1 exactly (`Fits.h:92-97` casts through uint8_t).
        assert_eq!(OperandValue::Bool(true).fits_convert(OpcodeSize::Narrow), 1);
        assert_eq!(
            OperandValue::Bool(false).fits_convert(OpcodeSize::Narrow),
            0
        );
    }

    /// The u32-backed unsigned kinds — SymbolTableOrScopeDepth raw()
    /// (`Fits.h:158-176`), ResolveType (`GetPutInfo.h:59` `: unsigned`), and
    /// the unscoped unsigned-underlying enums ProfileTypeBytecodeFlag /
    /// DebugHookType (`Fits.h:269-285`) — range-check and zero-extend like
    /// plain `unsigned`.
    #[test]
    fn u32_backed_unsigned_kinds_zero_extend_and_widen() {
        assert_eq!(
            select_width(&[OperandValue::ResolveType(13)]), // Dynamic
            OpcodeSize::Narrow
        );
        assert_eq!(
            select_width(&[OperandValue::DebugHookType(8)]), // DidAwait
            OpcodeSize::Narrow
        );
        assert_eq!(
            select_width(&[OperandValue::ProfileTypeBytecodeFlag(4)]),
            OpcodeSize::Narrow
        );
        assert_eq!(
            select_width(&[OperandValue::SymbolTableOrScopeDepth(300)]),
            OpcodeSize::Wide16
        );
        assert_eq!(
            select_width(&[OperandValue::SymbolTableOrScopeDepth(70_000)]),
            OpcodeSize::Wide32
        );
        // Unsigned decode NEVER sign-extends: 0xffff at wide16 is 65535.
        assert_eq!(
            round_trip(
                OperandValue::SymbolTableOrScopeDepth(0xffff),
                OpcodeSize::Wide16
            ),
            0xffff
        );
        for value in [
            OperandValue::ResolveType(13),
            OperandValue::DebugHookType(8),
            OperandValue::ProfileTypeBytecodeFlag(4),
            OperandValue::SymbolTableOrScopeDepth(70_000),
        ] {
            assert_eq!(
                round_trip(value, OpcodeSize::Wide32),
                value.as_i64(),
                "{value:?}"
            );
        }
    }

    /// `Fits<GetPutInfo>` (`Fits.h:178-232`): narrow/wide16 store the
    /// COMPRESSED byte `isStrict<<7 | resolveType<<3 | initMode<<1 |
    /// resolveMode`; wide32 is the same-size `bit_cast` of the raw
    /// `m_operand` (`GetPutInfo.h:222-257`). Decode rebuilds the m_operand
    /// the `GetPutInfo(...)` constructor produces (`GetPutInfo.h:238-240`).
    #[test]
    fn get_put_info_fits_matches_cpp_compressed_and_raw_encodings() {
        // (resolveMode=DoNotThrowIfNotFound(1), resolveType=Dynamic(13),
        // initializationMode=NotInitialization(2), strict).
        let operand: u32 = (1 << 30) | (1 << 20) | (2 << 10) | 13;
        let compressed: u64 = (1 << 7) | (13 << 3) | (2 << 1) | 1;
        let value = OperandValue::GetPutInfo(operand);
        assert_eq!(select_width(&[value]), OpcodeSize::Narrow);
        assert_eq!(value.fits_convert(OpcodeSize::Narrow), compressed);
        assert_eq!(value.fits_convert(OpcodeSize::Wide16), compressed);
        assert_eq!(value.fits_convert(OpcodeSize::Wide32), u64::from(operand));
        for width in ALL_WIDTHS {
            assert_eq!(round_trip(value, width), i64::from(operand), "{width:?}");
        }
        // A resolveType overflowing the 4-bit compressed field fails the
        // narrow/wide16 check (`Fits.h:206-212`) and rides wide32 raw.
        let overflow = OperandValue::GetPutInfo(16);
        assert!(!overflow.fits_check(OpcodeSize::Narrow));
        assert!(!overflow.fits_check(OpcodeSize::Wide16));
        assert_eq!(select_width(&[overflow]), OpcodeSize::Wide32);
    }

    /// `Fits<OperandTypes>` (`Fits.h:300-353`): narrow packs
    /// `(first << 4) | second` with unknownType(0x7e) <-> 0 remapped;
    /// wide16 is the same-size `bit_cast` of the raw `bits()`
    /// (`Fits.h:52-64`, little-endian {m_first, m_second} so first is the
    /// LOW byte, `ResultType.h:244-274`); wide32 truncates back to u16 on
    /// decode (`Fits.h:351`).
    #[test]
    fn operand_types_fits_matches_cpp_nibble_packing() {
        // first = TypeInt32 (0x01), second = TypeMaybeNumber (0x02).
        let int32_maybe_number = OperandValue::OperandTypes(0x0201);
        assert_eq!(select_width(&[int32_maybe_number]), OpcodeSize::Narrow);
        assert_eq!(int32_maybe_number.fits_convert(OpcodeSize::Narrow), 0x12);
        assert_eq!(
            fits_decode(OperandKind::OperandTypes, 0x12, OpcodeSize::Narrow),
            0x0201
        );

        // The default unknown/unknown pair encodes narrow as 0x00 and comes
        // back through the 0 -> unknownType remap.
        let unknown_pair = OperandValue::OperandTypes(0x7e7e);
        assert_eq!(select_width(&[unknown_pair]), OpcodeSize::Narrow);
        assert_eq!(unknown_pair.fits_convert(OpcodeSize::Narrow), 0x00);
        assert_eq!(
            fits_decode(OperandKind::OperandTypes, 0x00, OpcodeSize::Narrow),
            0x7e7e
        );

        // A type neither unknown nor 4-bit (first = TypeMaybeNull |
        // TypeMaybeBool = 0x30) fails the narrow check and stores raw bits.
        let wide = OperandValue::OperandTypes(0x0030);
        assert!(!wide.fits_check(OpcodeSize::Narrow));
        assert_eq!(select_width(&[wide]), OpcodeSize::Wide16);
        assert_eq!(wide.fits_convert(OpcodeSize::Wide16), 0x0030);
        assert_eq!(round_trip(wide, OpcodeSize::Wide16), 0x0030);
        assert_eq!(round_trip(wide, OpcodeSize::Wide32), 0x0030);
    }

    /// The m_metadataID slot: a metadata opcode's length is
    /// `args.length + 1` (`generator/Opcode.rb:372-374`), the extra slot
    /// being ONE per-width `unsigned` stream field
    /// (`generator/Metadata.rb:126-131`). Pinned against the generated
    /// `WebKitBuild/.../Bytecodes.h`: `macro(op_get_by_id, 5)` (id 20, 4
    /// args + metadata), `macro(op_call, 6)` (id 25, 5 args + metadata),
    /// `macro(op_jneq_ptr, 4)` (id 48, 3 args + metadata).
    #[test]
    fn metadata_slot_lengthens_instruction_per_generated_rule() {
        // op :get_by_id, args: { dst: VirtualRegister, base: VirtualRegister,
        // property: unsigned, valueProfile: unsigned }, metadata: { ... }
        // (`BytecodeList.rb:387-396`) — the REAL generated row (id 20).
        let get_by_id = descriptor_for(20).unwrap();
        assert_eq!(get_by_id.name, "get_by_id");
        assert!(get_by_id.has_metadata);
        assert_eq!(get_by_id.operands.len(), 4);
        assert_eq!(get_by_id.opcode_length(), 5);
        // The trailing slot is the metadataID, typed `unsigned`.
        assert_eq!(get_by_id.operand_kind(4), OperandKind::UnsignedImmediate);
        assert_eq!(get_by_id.operand_kind(0), OperandKind::VirtualRegister);
        // `size()` (`Instruction.h:138-145`) counts the extra slot per width.
        for (width, expected) in [
            (OpcodeSize::Narrow, 1 + 5 + 0),
            (OpcodeSize::Wide16, 1 + 10 + 1),
            (OpcodeSize::Wide32, 1 + 20 + 1),
        ] {
            assert_eq!(
                OPCODE_ID_BYTES
                    + get_by_id.opcode_length() * width.operand_bytes()
                    + width.prefix_bytes(),
                expected
            );
        }
        // Without metadata the same arg count stays at args.length:
        // op :get_by_id_with_this (id 53) has 5 args, no metadata block
        // (`BytecodeList.rb`), so `macro(op_get_by_id_with_this, 5)`.
        let with_this = descriptor_for(53).unwrap();
        assert_eq!(with_this.name, "get_by_id_with_this");
        assert!(!with_this.has_metadata);
        assert_eq!(with_this.opcode_length(), with_this.operands.len());
        // The checkpoint-bearing prefix rows carry their Bytecodes.h
        // `bytecodeCheckpointCountTable` counts: [2, 2, 3, 2, 2, 2, 3].
        let checkpoint_counts: Vec<u8> = OPCODE_TABLE
            .iter()
            .take(NUMBER_OF_BYTECODE_WITH_CHECKPOINTS)
            .map(|row| row.num_checkpoints)
            .collect();
        assert_eq!(checkpoint_counts, vec![2, 2, 3, 2, 2, 2, 3]);
    }
}
