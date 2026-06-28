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
//!   2. VirtualRegister operand encoding: the real `Fits<VirtualRegister>`
//!      (`Fits.h:118-156`) remaps constant registers into a per-width
//!      `FirstConstantRegisterIndex{8,16}` band. This core stores a plain signed
//!      offset (faithful for locals/arguments, the tested path) and flags the
//!      constant remap as a shared decision.
//!   3. The metadata table (`UnlinkedMetadataTable`/`MetadataTable`): opcodes
//!      with metadata add ONE operand slot to `opcodeLengths` (the metadataID).
//!      The representative subset has no metadata; wiring it is shared work.
#![allow(dead_code)]

use crate::bytecode::code_block::BytecodeIndex;

/// Opcode-ID width in bytes. For JS bytecode the opcode is ALWAYS one byte,
/// even inside wide16/wide32 instructions: `Opcode.h:86-87`
/// (`static_assert(NUMBER_OF_BYTECODE_IDS < 255)` and
/// `maxJSOpcodeIDWidth = OpcodeSize::Narrow`). This matches the rejected-alt
/// `bytecode-wide-instruction-opcode-same-width` (move `24b088b7`): the opcode
/// is narrow in every form so wide instructions only widen operand fields.
const OPCODE_ID_BYTES: usize = 1;

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

/// Representative subset of JSC's generated JS opcode IDs.
///
/// JSC generates the real numeric IDs and their order from `BytecodeList.rb`
/// (`Opcode.h:66` `FOR_EACH_OPCODE_ID(OPCODE_ID_ENUM)`). This subset assigns its
/// own representative IDs to prove the packed-stream mechanism end to end; the
/// full ~240-opcode table is a generator follow-up. `wide16`/`wide32` are real
/// JSC opcodes (`BytecodeList.rb:1174,1178`) whose narrow 1-byte ID, read at the
/// instruction's first byte, selects the operand width (`Instruction.h:81-96`).
pub mod opcode_id {
    /// Width-prefix opcode. `narrow()->opcodeID() == wide16` => wide16 form.
    pub const WIDE16: u8 = 0;
    /// Width-prefix opcode. `narrow()->opcodeID() == wide32` => wide32 form.
    pub const WIDE32: u8 = 1;
    pub const ENTER: u8 = 2;
    pub const MOV: u8 = 3;
    pub const ADD: u8 = 4;
    pub const EQ: u8 = 5;
    pub const JMP: u8 = 6;
    pub const JTRUE: u8 = 7;
    pub const RET: u8 = 8;
}

/// Operand classes in the representative subset.
///
/// Each maps to a `BytecodeList.rb` arg type. The class fixes the operand's
/// signedness for `Fits` width selection and for sign/zero extension on decode
/// (`Fits.h:66-85`, `Fits.h:118-156`, `Fits.h:355-379`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OperandKind {
    /// `VirtualRegister` arg: a signed frame/constant slot index.
    VirtualRegister,
    /// `unsigned` profile-table index (e.g. `op_add`'s `profileIndex`).
    ProfileIndex,
    /// `OperandTypes` arg: packed result-type hints, stored unsigned
    /// (`Fits.h:300-353`).
    OperandTypes,
    /// `BoundLabel` arg: a SIGNED byte delta from the jump instruction's start
    /// to its target (`Label.h:73-79,146-151`).
    BoundLabel,
}

impl OperandKind {
    /// `BoundLabel` and `VirtualRegister` are signed; profile/type indices are
    /// unsigned. Governs both the `Fits` range check and decode extension.
    pub const fn is_signed(self) -> bool {
        matches!(self, Self::VirtualRegister | Self::BoundLabel)
    }
}

/// Faithful descriptor for one opcode in the representative core subset.
///
/// Mirrors a `BytecodeList.rb` `op :name, args: { ... }` declaration: the
/// operand schema plus the derived `opcodeLength` that drives
/// `BaseInstruction::size()` (`Instruction.h:138-145`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpcodeDescriptor {
    pub id: u8,
    pub name: &'static str,
    pub operands: &'static [OperandKind],
    /// True for the width-prefix opcodes (`op_wide16`/`op_wide32`).
    pub is_wide_prefix: bool,
}

impl OpcodeDescriptor {
    /// `opcodeLengths[id]` = operand-slot count.
    ///
    /// `generator/Opcode.rb:372` `length = args.length + (metadata ? 1 : 0)` and
    /// `generator/Section.rb:111` emits `macro(name, length)`. The representative
    /// subset has no metadata, so this is exactly `operands.len()`.
    pub const fn opcode_length(&self) -> usize {
        self.operands.len()
    }
}

/// Representative opcode table. Models JSC's ONE untyped op per operation
/// (`op_add`, NOT `AddInt32`): type specialization is the JIT's job via the
/// profile operand (`metadata-table.md` `d1cb45f8`).
static OPCODE_TABLE: &[OpcodeDescriptor] = &[
    OpcodeDescriptor {
        id: opcode_id::WIDE16,
        name: "wide16",
        operands: &[],
        is_wide_prefix: true,
    },
    OpcodeDescriptor {
        id: opcode_id::WIDE32,
        name: "wide32",
        operands: &[],
        is_wide_prefix: true,
    },
    // op :enter  (BytecodeList.rb:1180) — no operands.
    OpcodeDescriptor {
        id: opcode_id::ENTER,
        name: "enter",
        operands: &[],
        is_wide_prefix: false,
    },
    // op :mov, args: { dst, src }  (BytecodeList.rb:1248-1252).
    OpcodeDescriptor {
        id: opcode_id::MOV,
        name: "mov",
        operands: &[OperandKind::VirtualRegister, OperandKind::VirtualRegister],
        is_wide_prefix: false,
    },
    // op_group :ProfiledBinaryOpWithOperandTypes [:add, ...],
    //   args: { dst, lhs, rhs, profileIndex, operandTypes }
    //   (BytecodeList.rb:1276-1291). UNTYPED op carrying a profile index.
    OpcodeDescriptor {
        id: opcode_id::ADD,
        name: "add",
        operands: &[
            OperandKind::VirtualRegister,
            OperandKind::VirtualRegister,
            OperandKind::VirtualRegister,
            OperandKind::ProfileIndex,
            OperandKind::OperandTypes,
        ],
        is_wide_prefix: false,
    },
    // op_group :BinaryOp [:eq, ...], args: { dst, lhs, rhs }
    //   (BytecodeList.rb:1254-1268). Unprofiled binary op.
    OpcodeDescriptor {
        id: opcode_id::EQ,
        name: "eq",
        operands: &[
            OperandKind::VirtualRegister,
            OperandKind::VirtualRegister,
            OperandKind::VirtualRegister,
        ],
        is_wide_prefix: false,
    },
    // op :jmp, args: { targetLabel }  (BytecodeList.rb:933-936).
    OpcodeDescriptor {
        id: opcode_id::JMP,
        name: "jmp",
        operands: &[OperandKind::BoundLabel],
        is_wide_prefix: false,
    },
    // op :jtrue, args: { condition, targetLabel }  (BytecodeList.rb:938-942).
    OpcodeDescriptor {
        id: opcode_id::JTRUE,
        name: "jtrue",
        operands: &[OperandKind::VirtualRegister, OperandKind::BoundLabel],
        is_wide_prefix: false,
    },
    // op :ret, args: { value }  (BytecodeList.rb:1040-1043).
    OpcodeDescriptor {
        id: opcode_id::RET,
        name: "ret",
        operands: &[OperandKind::VirtualRegister],
        is_wide_prefix: false,
    },
];

/// Look up a descriptor by opcode ID. The real engine indexes generated tables
/// directly by `OpcodeID`; the representative subset scans its small table.
pub fn descriptor_for(id: u8) -> Option<&'static OpcodeDescriptor> {
    OPCODE_TABLE.iter().find(|descriptor| descriptor.id == id)
}

/// A concrete operand value handed to the writer. Carries the integer plus its
/// `OperandKind` so the writer can range-check and convert via `Fits`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperandValue {
    VirtualRegister(i32),
    ProfileIndex(u32),
    OperandTypes(u16),
    /// Signed byte delta from the jump instruction start to its target.
    BoundLabel(i32),
}

impl OperandValue {
    const fn kind(self) -> OperandKind {
        match self {
            Self::VirtualRegister(_) => OperandKind::VirtualRegister,
            Self::ProfileIndex(_) => OperandKind::ProfileIndex,
            Self::OperandTypes(_) => OperandKind::OperandTypes,
            Self::BoundLabel(_) => OperandKind::BoundLabel,
        }
    }

    const fn as_i64(self) -> i64 {
        match self {
            Self::VirtualRegister(value) => value as i64,
            Self::ProfileIndex(value) => value as i64,
            Self::OperandTypes(value) => value as i64,
            Self::BoundLabel(value) => value as i64,
        }
    }

    /// `Fits<T, size>::check` (`Fits.h:71-74`): does this operand fit the width?
    ///
    /// NOTE (serial coupling 2): the real `Fits<VirtualRegister>`
    /// (`Fits.h:118-156`) additionally remaps constant registers into a
    /// per-width constant band. This core checks the plain signed offset, which
    /// is faithful for locals/arguments; the constant remap is flagged, not
    /// ported.
    fn fits_check(self, width: OpcodeSize) -> bool {
        if self.kind().is_signed() {
            let value = self.as_i64();
            value >= width.signed_min() && value <= width.signed_max()
        } else {
            (self.as_i64() as u64) <= width.unsigned_max()
        }
    }

    /// `Fits<T, size>::convert` (`Fits.h:76-80`): the width-truncated bit pattern
    /// stored little-endian into the stream. Asserts fit, mirroring the C++
    /// `ASSERT(check(t))`.
    fn fits_convert(self, width: OpcodeSize) -> u64 {
        debug_assert!(
            self.fits_check(width),
            "operand does not fit selected width"
        );
        let mask = width.unsigned_max();
        (self.as_i64() as u64) & mask
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

/// Decode an instruction header at `offset`: the operand width, descriptor, and
/// the byte offset where operands begin.
///
/// Faithful to `BaseInstruction::opcodeID()`/`width()` (`Instruction.h:67-96`)
/// and the `narrow()`/`wide16()`/`wide32()` pointer math (`Instruction.h:181-198`):
/// read the first byte; if it is the `wide32`/`wide16` prefix the real opcode is
/// the NEXT byte and operands start after it, otherwise the first byte is the
/// opcode and operands start immediately after.
fn decode_header(bytes: &[u8], offset: usize) -> (OpcodeSize, &'static OpcodeDescriptor, usize) {
    let first = bytes[offset];
    let (width, opcode_byte_index) = if first == opcode_id::WIDE32 {
        (OpcodeSize::Wide32, offset + 1)
    } else if first == opcode_id::WIDE16 {
        (OpcodeSize::Wide16, offset + 1)
    } else {
        (OpcodeSize::Narrow, offset)
    };
    let id = bytes[opcode_byte_index];
    let descriptor = descriptor_for(id).expect("unknown opcode id in stream");
    // operands_start = opcode byte index + opcodeIDBytes(1). Equivalently
    // offset + prefixSize + 1 (Argument.rb setter/load_from_stream location).
    let operands_start = opcode_byte_index + OPCODE_ID_BYTES;
    (width, descriptor, operands_start)
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
            "operand count must match opcodeLengths[{}]",
            descriptor.name
        );
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
        if descriptor.operands[i].is_signed() {
            sign_extend(raw, width.operand_bytes())
        } else {
            raw as i64
        }
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

    /// `opcodeLengths[id]` = operand-slot count (`generator/Opcode.rb:372`).
    #[test]
    fn opcode_lengths_match_bytecode_list() {
        assert_eq!(descriptor_for(ENTER).unwrap().opcode_length(), 0);
        assert_eq!(descriptor_for(MOV).unwrap().opcode_length(), 2);
        assert_eq!(descriptor_for(ADD).unwrap().opcode_length(), 5); // dst,lhs,rhs,profile,types
        assert_eq!(descriptor_for(EQ).unwrap().opcode_length(), 3);
        assert_eq!(descriptor_for(JMP).unwrap().opcode_length(), 1);
        assert_eq!(descriptor_for(JTRUE).unwrap().opcode_length(), 2);
        assert_eq!(descriptor_for(RET).unwrap().opcode_length(), 1);
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
        // profileIndex 5000 overflows int8/uint8 -> Wide16 for every field.
        assert_eq!(
            select_width(&[
                OperandValue::VirtualRegister(-1),
                OperandValue::VirtualRegister(-2),
                OperandValue::VirtualRegister(-3),
                OperandValue::ProfileIndex(5000),
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
                OperandValue::ProfileIndex(100_000),
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
                OperandValue::ProfileIndex(5000),
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
                OperandValue::ProfileIndex(100_000),
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
}
