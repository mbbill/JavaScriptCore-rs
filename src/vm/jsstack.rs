//! Native-thread-stack JS call-frame substrate (JSStack migration B1).
//!
//! ## What this is
//!
//! The faithful C++-JSC TYPES and FP-relative offset table for the JavaScript
//! call stack, the single provenance gate that recovers a `*Register` from a
//! raw slot address, and (B2) the LIVE contiguous mmap reservation it backs:
//! `JsStack::new` reserves one immovable RW region with a low-end guard page,
//! seeds `sp`/`fp` at the high end, and exposes the reservation's provenance
//! once. On top of that B2 adds the MANDATORY stack-limit guard
//! (`CLoopStack::isSafeToRecurse` / `ensureCapacityFor` + soft-reserved zone)
//! and the `doVMEntry` frame-seeding primitive (`try_seed_entry_frame`) that
//! materializes one `CallFrame` (header + `this` + args + undefined fill) into
//! the arena. This is the ADDITIVE, parallel-safe FOUNDATION for moving JS
//! execution onto the native thread stack. It is `dead_code` and is NOT wired
//! into the live dispatch path: the live engine still uses the abstract
//! `RegisterFile`/`runtime::interpreter::CallFrame` model. The B3 dual-write
//! (which feeds this seeding primitive from the live model) and the owner-gated
//! read-flip cutovers (B4/B6) are out of scope here.
//!
//! ## C++ ground truth
//!
//! - `JSC::Register` — `interpreter/Register.h:45-106`: a `union` over
//!   `EncodedJSValue`/`CallFrame*`/`CodeBlock*`/`double`/`int64_t` viewing the
//!   SAME 8 bytes. WTF marks it trivially memcpy-movable
//!   (`VectorTraits<Register> : VectorTraitsBase<true,...>`, `Register.h:244`).
//! - `JSC::CallFrame : private Register` — `interpreter/CallFrame.h:189`: a
//!   `Register*` whose slot 0 is `callerFrame`; `registers() == this`
//!   (`CallFrame.h:218`). Slots are addressed at fixed FP-relative offsets
//!   (`CallFrameSlot`, `CallFrame.h:176-191`).
//! - `JSC::CalleeBits` — `interpreter/CalleeBits.h:37-188`: a tagged `void*`
//!   distinguishing a `JSCell` callee from a boxed `NativeCallee`.
//! - `JSC::CLoopStack` — `interpreter/CLoopStack.h:48-121`: the descending JS
//!   value-stack reservation; the closest ground truth for the reservation
//!   bookkeeping. The live engine runs on the native thread stack instead — the
//!   private register stack was replaced in 2014
//!   (`mcts_mem/.../call-frame-layout.alt/jsc-private-register-stack.md`,
//!   move `a3ac51de`). The `Vec`-backed C_LOOP model is superseded and is NOT
//!   re-proposed here; this models the address/offset substrate faithfully.
//!
//! ## Unsafe confinement
//!
//! Every `unsafe` operation in the JS-stack substrate is confined to this
//! module, behind `#![allow(unsafe_code)]`, with a per-block `SAFETY:` citation
//! (mirroring `gc/heap/precise_allocation.rs` and
//! `jit/unsafe_platform_boundary.rs`). The crate denies `unsafe_op_in_unsafe_fn`
//! (`Cargo.toml [lints.rust]`), so unsafe ops live in explicit blocks even
//! inside `unsafe fn`s. Provenance is exposed ONCE for the reservation backing
//! (`precise_allocation.rs:57`) and recovered with `with_exposed_provenance`
//! (`precise_allocation.rs:77`).

#![allow(unsafe_code)]
// B1 is the additive foundation: the types, offsets, and gate land before any
// caller. Wiring (B2 seeding, B3 dual-write, B4/B6 cutovers) is out of scope, so
// these items are intentionally unused in the live build.
#![allow(dead_code)]

use core::ptr::{self, NonNull};

use crate::bytecode::CodeBlock;
use crate::value::{EncodedJsValue, JsValue};
use crate::vm::FrameAddress;

// === Constants ===

/// `sizeof(Register)` on JSVALUE64 (`Register.h:98-105`, a single 8-byte union).
pub(crate) const REGISTER_SIZE_IN_BYTES: usize = 8;

/// `CallerFrameAndPC::sizeInRegisters` (`CallFrame.h:109-114`): two pointer-sized
/// words (callerFrame, returnPC) occupy two `Register` slots on JSVALUE64.
pub(crate) const CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS: i32 = 2;

/// `CallFrame::headerSizeInRegisters` (`CallFrame.h:191`):
/// `argumentCountIncludingThis + 1` == 5. The faithful JS call-frame header is
/// exactly these five slots; `this`, arguments, and locals are NOT header slots
/// (`VirtualRegister::isHeader`, `VirtualRegister.h:73`).
pub(crate) const HEADER_SIZE_IN_REGISTERS: i32 = 5;

/// Default JS-stack reservation size == `Options::maxPerThreadStackUsage()`
/// (`runtime/OptionsList.h:92`, `5 * MB`). `CLoopStack::CLoopStack` rounds this
/// to the page size before reserving (`CLoopStack.cpp:58-59`).
pub(crate) const DEFAULT_JS_STACK_RESERVATION_BYTES: usize = 5 * 1024 * 1024;

/// Default soft-reserved zone == `Options::softReservedZoneSize()`
/// (`runtime/OptionsList.h:93`, `128 * KB`). `VM::updateSoftReservedZoneSize`
/// feeds it to `CLoopStack::setSoftReservedZoneSize`
/// (`runtime/VM.cpp:1140-1145`). The zone is the soft margin a frame push must
/// stay above; the low-end guard page is the hard backstop below it.
pub(crate) const DEFAULT_SOFT_RESERVED_ZONE_BYTES: usize = 128 * 1024;

/// FP-relative `Register` slot indices, byte-exact to `enum class CallFrameSlot`
/// (`CallFrame.h:176-181`). These reconcile EXACTLY with the private
/// `JSC_CALL_FRAME_*_SLOT` constants in `vm/arm64_native_entry.rs:74-80`; the
/// B4/B6 cutover unifies the two (the entry path can later consume these). They
/// are kept here as the authoritative table for the JS-stack substrate.
pub(crate) struct CallFrameSlot;

impl CallFrameSlot {
    /// `CallerFrameAndPC::callerFrame` (`CallFrame.h:110`); `callerFrameOffset()`
    /// is slot 0 (`CallFrame.h:232`). Byte +0.
    pub(crate) const CALLER_FRAME: i32 = 0;
    /// `CallerFrameAndPC::returnPC` (`CallFrame.h:111`); `returnPCOffset()` is
    /// slot 1 (`CallFrame.h:238`). Byte +8.
    pub(crate) const RETURN_PC: i32 = 1;
    /// `CallFrameSlot::codeBlock = CallerFrameAndPC::sizeInRegisters`
    /// (`CallFrame.h:177`). Byte +16.
    pub(crate) const CODE_BLOCK: i32 = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS;
    /// `CallFrameSlot::callee = codeBlock + 1` (`CallFrame.h:178`). Byte +24.
    pub(crate) const CALLEE: i32 = Self::CODE_BLOCK + 1;
    /// `CallFrameSlot::argumentCountIncludingThis = callee + 1`
    /// (`CallFrame.h:179`). Byte +32. Payload half = count
    /// (`CallFrame.h:287`); tag half = `CallSiteIndex` (`CallFrame.h:165-167`).
    pub(crate) const ARGUMENT_COUNT_INCLUDING_THIS: i32 = Self::CALLEE + 1;
    /// `CallFrameSlot::thisArgument = argumentCountIncludingThis + 1`
    /// (`CallFrame.h:180`). Byte +40. `thisArgumentOffset()` (`CallFrame.h:308`).
    pub(crate) const THIS_ARGUMENT: i32 = Self::ARGUMENT_COUNT_INCLUDING_THIS + 1;
    /// `CallFrameSlot::firstArgument = thisArgument + 1` (`CallFrame.h:181`):
    /// arg0. Byte +48.
    pub(crate) const FIRST_ARGUMENT: i32 = Self::THIS_ARGUMENT + 1;
}

/// `VirtualRegister::localToOperand(local) = -1 - local` (`VirtualRegister.h:111`):
/// locals grow DOWN. local0 is operand -1 (byte -8), local1 operand -2, ...
pub(crate) const fn local_to_operand(local: i32) -> i32 {
    -1 - local
}

/// `VirtualRegister::operandToLocal(operand) = -1 - operand`
/// (`VirtualRegister.h:112`).
pub(crate) const fn operand_to_local(operand: i32) -> i32 {
    -1 - operand
}

/// `CallFrame::argumentOffset(argument)` (`CallFrame.h:288`):
/// `firstArgument + argument`. argN is at byte +48 + 8*N.
pub(crate) const fn argument_offset(argument: i32) -> i32 {
    CallFrameSlot::FIRST_ARGUMENT + argument
}

/// `CallFrame::argumentOffsetIncludingThis(argument)` (`CallFrame.h:289`):
/// `thisArgument + argument`.
pub(crate) const fn argument_offset_including_this(argument: i32) -> i32 {
    CallFrameSlot::THIS_ARGUMENT + argument
}

/// `VirtualRegister::offsetInBytes` (`VirtualRegister.h:79`):
/// `operand * sizeof(Register)`.
pub(crate) const fn operand_offset_in_bytes(operand: i32) -> isize {
    (operand as isize) * (REGISTER_SIZE_IN_BYTES as isize)
}

/// Byte address of the value `Register` at FP-relative `operand` within the
/// frame whose slot-0 (`fp`) address is `fp`. This is the single
/// `VirtualRegister -> arena-address` mapping the B4 read-flip uses, faithful to
/// `AssemblyHelpers::addressFor(vreg) = Address(x29, vreg.offset()*8)`
/// (`AssemblyHelpers.h:1290-1298`, cfr == x29) and `VirtualRegister::offsetInBytes
/// = operand * sizeof(Register)` (`VirtualRegister.h:79`): the operand IS the
/// VirtualRegister's raw value, so locals (operand < 0) land below `fp` and the
/// header/`this`/arguments (operand >= 0) at/above `fp`. Returns `None` only on
/// arithmetic overflow (never in practice: `fp` is a live mmap address and the
/// operand is a small in-frame offset).
pub(crate) fn frame_slot_addr(fp: usize, operand: i32) -> Option<usize> {
    let byte_offset = (operand as isize).checked_mul(REGISTER_SIZE_IN_BYTES as isize)?;
    let addr = (fp as isize).checked_add(byte_offset)?;
    if addr < 0 {
        return None;
    }
    Some(addr as usize)
}

// === Register (Register.h:45-106) ===

/// `JSC::Register` (`Register.h:45-106`): one JS stack slot. A `union` over
/// `EncodedJSValue`/`CallFrame*`/`CodeBlock*`/`double`/`int64_t` — every view
/// reads the SAME 8 raw bytes. WTF marks it trivially memcpy-movable
/// (`Register.h:244`), so the Rust port is a plain `Copy` POD with NO `Drop`.
///
/// It holds RAW NaN-boxed bits (`value::repr::EncodedJsValue`, the JSVALUE64
/// keystone), NEVER the live `RuntimeValue` enum: a stack slot is untyped 8-byte
/// storage, and typed/provenance-carrying recovery is the gate's job, not the
/// slot's.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct Register(EncodedJsValue);

// `Register.h:244`: trivially memcpy-movable POD; the value-rep keystone already
// proved `EncodedJsValue` is 8 bytes, so a `repr(transparent)` wrapper is 8 too.
const _: () = assert!(core::mem::size_of::<Register>() == REGISTER_SIZE_IN_BYTES);

impl Register {
    /// Wrap raw encoded bits (the `u.value` / `u.encodedValue` view).
    pub(crate) const fn from_encoded(value: EncodedJsValue) -> Self {
        Register(value)
    }

    /// Wrap raw 64-bit storage directly.
    pub(crate) const fn from_bits(bits: u64) -> Self {
        Register(EncodedJsValue(bits))
    }

    /// The raw 64 bits of this slot.
    pub(crate) const fn bits(self) -> u64 {
        self.0 .0
    }

    /// `Register::encodedJSValue()` (`Register.h:53`).
    pub(crate) const fn encoded_js_value(self) -> EncodedJsValue {
        self.0
    }

    /// `Register::jsValue()` (`Register.h:51`): the slot as a `JSValue`.
    pub(crate) const fn js_value(self) -> JsValue {
        JsValue::from_encoded(self.0)
    }

    /// `Register::payload()` (`Register.h:210-213`): `u.encodedValue.asBits.payload`
    /// — the LOW 32 bits on little-endian (`EncodedValueDescriptor.h:52-55`).
    pub(crate) const fn payload(self) -> i32 {
        self.bits() as u32 as i32
    }

    /// `Register::tag()` (`Register.h:215-218`): `u.encodedValue.asBits.tag` — the
    /// HIGH 32 bits on little-endian (`EncodedValueDescriptor.h:52-55`).
    pub(crate) const fn tag(self) -> i32 {
        (self.bits() >> 32) as u32 as i32
    }

    /// `Register::i()` (`Register.h:61`) == `unboxedInt32()` (`Register.h:112-115`):
    /// the int32 payload.
    pub(crate) const fn i(self) -> i32 {
        self.payload()
    }

    /// `Register::unboxedInt64()` (`Register.h:137-140`): `u.integer`.
    pub(crate) const fn unboxed_int64(self) -> i64 {
        self.bits() as i64
    }

    /// `Register::unboxedDouble()` (`Register.h:164-167`): `u.number`.
    pub(crate) const fn unboxed_double(self) -> f64 {
        f64::from_bits(self.bits())
    }

    /// `Register::pointer()` (`Register.h:192-199`): on JSVALUE64
    /// `u.encodedValue.ptr`. The POD carries no provenance, so this returns the
    /// ADDRESS bits; a provenance-carrying `*Register`/cell is recovered through
    /// the [`JsStack`] gate (`with_exposed_provenance`).
    pub(crate) const fn pointer_bits(self) -> usize {
        self.bits() as usize
    }

    /// `Register::callFrame()` (`Register.h:62`, union `u.callFrame`): the bits as
    /// a caller-frame address. Typed recovery is the gate's job (B2+).
    pub(crate) const fn call_frame_bits(self) -> usize {
        self.bits() as usize
    }

    /// `Register::codeBlock()` (`Register.h:63`, union `u.codeBlock`): the bits as
    /// a `CodeBlock` address.
    pub(crate) const fn code_block_bits(self) -> usize {
        self.bits() as usize
    }

    /// The slot interpreted as a callee: `CallFrame::callee()` reads
    /// `CalleeBits(slot.unboxedInt64())` (`CallFrame.h:202`).
    pub(crate) const fn callee(self) -> CalleeBits {
        CalleeBits::from_bits(self.bits() as usize)
    }
}

// === CalleeBits (CalleeBits.h:37-188) ===

// JSVALUE64 immediate tag bits (`runtime/JSCJSValue.h`), repeated locally with
// citations rather than reaching into `value::repr`'s private constants. These
// match `value/repr.rs:33,38` exactly.
//   NumberTag = 0xfffe_0000_0000_0000 (JSCJSValue.h:457)
//   OtherTag  = 0x2                   (JSCJSValue.h:464)
const NUMBER_TAG: u64 = 0xfffe_0000_0000_0000;
const OTHER_TAG: u64 = 0x2;
/// `JSValue::NativeCalleeTag = OtherTag | 0x1` == 0x3 (`JSCJSValue.h:490`).
const NATIVE_CALLEE_TAG: u64 = OTHER_TAG | 0x1;
/// `JSValue::NativeCalleeMask = NumberTag | 0x7` (`JSCJSValue.h:491`). The full
/// test is `x & NativeCalleeMask == NativeCalleeTag` (`JSCJSValue.h:494`).
const NATIVE_CALLEE_MASK: u64 = NUMBER_TAG | 0x7;

/// `JSC::CalleeBits` (`CalleeBits.h:37-188`): on JSVALUE64 a single tagged `void*`
/// (`m_ptr`, `CalleeBits.h:184`) that is either a `JSCell` callee or a boxed
/// `NativeCallee` (Wasm / JS builtin). 32-bit's separate `m_tag` field is not
/// modeled (JSVALUE64 only).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct CalleeBits(usize);

impl CalleeBits {
    /// Wrap raw `m_ptr` bits.
    pub(crate) const fn from_bits(ptr: usize) -> Self {
        CalleeBits(ptr)
    }

    /// `CalleeBits::nullCallee()` on JSVALUE64 (`CalleeBits.h:104-107`): a default
    /// (null) `m_ptr`.
    pub(crate) const fn null() -> Self {
        CalleeBits(0)
    }

    /// `CalleeBits::rawPtr()` (`CalleeBits.h:178`).
    pub(crate) const fn raw_ptr(self) -> usize {
        self.0
    }

    /// `CalleeBits::isNativeCallee()` on JSVALUE64 (`CalleeBits.h:152-159`):
    /// `(m_ptr & NativeCalleeMask) == NativeCalleeTag`.
    pub(crate) const fn is_native_callee(self) -> bool {
        ((self.0 as u64) & NATIVE_CALLEE_MASK) == NATIVE_CALLEE_TAG
    }

    /// `CalleeBits::isCell()` (`CalleeBits.h:160`): `!isNativeCallee()`.
    pub(crate) const fn is_cell(self) -> bool {
        !self.is_native_callee()
    }

    /// `CalleeBits::asCell()` (`CalleeBits.h:162-166`): `static_cast<JSCell*>(m_ptr)`
    /// — the pointer bits unchanged. (Returned as raw bits; cell typing is B2+.)
    pub(crate) const fn as_cell(self) -> usize {
        self.0
    }

    /// `CalleeBits::asNativeCallee()` on JSVALUE64 (`CalleeBits.h:168-176`):
    /// `(m_ptr & ~NativeCalleeTag) + lowestAccessibleAddress()`.
    ///
    /// `lowest_accessible_address` is JSC's `g_wtfConfig.lowestAccessibleAddress`
    /// (`WTF/wtf/AccessibleAddress.h:32-35`). B1 takes it as a parameter so the
    /// box/unbox round-trip is unit-testable without the live config; the
    /// B-series wires the process global.
    pub(crate) const fn as_native_callee(self, lowest_accessible_address: usize) -> usize {
        (self.0 & !(NATIVE_CALLEE_TAG as usize)) + lowest_accessible_address
    }

    /// `CalleeBits::boxNativeCallee()` on JSVALUE64 (`CalleeBits.h:140-146`):
    /// `(bits - lowestAccessibleAddress()) | NativeCalleeTag`. Inverse of
    /// [`Self::as_native_callee`].
    pub(crate) const fn box_native_callee(
        callee_ptr: usize,
        lowest_accessible_address: usize,
    ) -> Self {
        CalleeBits((callee_ptr - lowest_accessible_address) | (NATIVE_CALLEE_TAG as usize))
    }
}

// === CallFrame (CallFrame.h:189) ===

/// `JSC::CallFrame : private Register` (`CallFrame.h:189`). A `CallFrame` IS a
/// `Register*` whose slot 0 is `callerFrame`; `registers() == this`
/// (`CallFrame.h:218`). The Rust port is a `NonNull<Register>` pointing AT slot
/// 0 and models NO owned fields — every accessor indexes the EXACT JSC
/// FP-relative slot. Distinct from the live abstract
/// `runtime::interpreter::CallFrame`; this is the native-stack faithful type the
/// B4/B6 cutover targets.
///
/// The pointer must carry valid provenance over the live frame window
/// `[frame, frame + used)` — e.g. obtained from [`JsStack::call_frame_at`], which
/// recovers it from the once-exposed reservation base.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct CallFrame(NonNull<Register>);

impl CallFrame {
    /// `CallFrame::create(Register*)` (`CallFrame.h:217`): wrap a `Register*` that
    /// addresses slot 0 (the `callerFrame` slot).
    pub(crate) const fn from_registers(slot0: NonNull<Register>) -> Self {
        CallFrame(slot0)
    }

    /// `CallFrame::registers()` (`CallFrame.h:218`): `registers() == this`.
    pub(crate) const fn registers(self) -> NonNull<Register> {
        self.0
    }

    /// Raw pointer to the `Register` at FP-relative `offset` (`this[offset]`).
    /// Locals are negative (grow down); the header and arguments are
    /// non-negative. Uses `wrapping_offset` (no UB on the arithmetic itself); the
    /// in-range guarantee is checked at the deref in [`Self::slot`]/[`Self::set_slot`].
    fn slot_ptr(self, offset: i32) -> *mut Register {
        self.0.as_ptr().wrapping_offset(offset as isize)
    }

    /// Read the `Register` at FP-relative `offset`.
    ///
    /// # Safety
    /// `offset` must address a slot inside this frame's live window, and the
    /// frame pointer must carry provenance for that slot.
    pub(crate) unsafe fn slot(self, offset: i32) -> Register {
        // SAFETY: the caller guarantees `offset` is within the frame; `slot_ptr`
        // preserves the `NonNull`'s provenance and the 8-byte alignment of slot
        // 0, and the slot holds an initialized `Register` (a `Copy` POD, so the
        // read is a byte copy with no `Drop`).
        unsafe { ptr::read(self.slot_ptr(offset)) }
    }

    /// Write the `Register` at FP-relative `offset`.
    ///
    /// # Safety
    /// As [`Self::slot`].
    pub(crate) unsafe fn set_slot(self, offset: i32, value: Register) {
        // SAFETY: as `slot`; `offset` is an in-range, 8-aligned slot of this
        // frame. Overwriting a `Copy` POD runs no destructor.
        unsafe { ptr::write(self.slot_ptr(offset), value) }
    }

    /// `callerFrameOrEntryFrame()` bits (`CallFrame.h:224`, slot 0).
    ///
    /// # Safety
    /// The frame's slot 0 must be live (see [`Self::slot`]).
    pub(crate) unsafe fn caller_frame_bits(self) -> usize {
        // SAFETY: slot 0 (`CallerFrameAndPC::callerFrame`) of a live frame.
        unsafe { self.slot(CallFrameSlot::CALLER_FRAME).call_frame_bits() }
    }

    /// `rawReturnPC()` bits (`CallFrame.h:234`, slot 1).
    ///
    /// # Safety
    /// The frame's slot 1 must be live.
    pub(crate) unsafe fn return_pc_bits(self) -> usize {
        // SAFETY: slot 1 (`CallerFrameAndPC::returnPC`) of a live frame.
        unsafe { self.slot(CallFrameSlot::RETURN_PC).pointer_bits() }
    }

    /// `codeBlock` bits (`CallFrame.h:204-205`, slot 2).
    ///
    /// # Safety
    /// The frame's slot 2 must be live.
    pub(crate) unsafe fn code_block_bits(self) -> usize {
        // SAFETY: slot 2 (`CallFrameSlot::codeBlock`) of a live frame.
        unsafe { self.slot(CallFrameSlot::CODE_BLOCK).code_block_bits() }
    }

    /// `CallFrame::codeBlock()` (`CallFrame.h:204-205`, slot 2) recovered as a real
    /// `*const CodeBlock` — the cfr-relative read of `[fp+16]` the JIT/op_call
    /// path uses to reach the callee's `CodeBlock`. After K1 the seed writes the
    /// registry's stable `CodeBlock*` here (see
    /// `CodeBlockRegistry::code_block_pointer`), so this is machine-meaningful; a
    /// null slot (no code block, e.g. host frames) yields a null pointer.
    ///
    /// # Safety
    /// - The frame's slot 2 must be live (see [`Self::slot`]).
    /// - The returned pointer is only dereferenceable when slot 2 was seeded from
    ///   a live `CodeBlockRegistry` record's `Rc<CodeBlock>` (the K1 write path);
    ///   the registry keeps that box alive and unmoved for the instance's life, so
    ///   the address stays valid as long as the owner is registered. Callers MUST
    ///   null-check the result before dereferencing.
    pub(crate) unsafe fn code_block_ptr(self) -> *const CodeBlock {
        // SAFETY: slot 2 (`CallFrameSlot::codeBlock`) of a live frame; reinterpret
        // the stored address bits as the `CodeBlock*` the K1 seed wrote.
        unsafe { self.code_block_bits() as *const CodeBlock }
    }

    /// `CallFrame::callee()` (`CallFrame.h:202`): `CalleeBits(slot.unboxedInt64())`,
    /// slot 3.
    ///
    /// # Safety
    /// The frame's slot 3 must be live.
    pub(crate) unsafe fn callee(self) -> CalleeBits {
        // SAFETY: slot 3 (`CallFrameSlot::callee`) of a live frame.
        unsafe { self.slot(CallFrameSlot::CALLEE).callee() }
    }

    /// `CallFrame::argumentCountIncludingThis()` (`CallFrame.h:287`): the PAYLOAD
    /// half of slot 4.
    ///
    /// # Safety
    /// The frame's slot 4 must be live.
    pub(crate) unsafe fn argument_count_including_this(self) -> i32 {
        // SAFETY: slot 4 (`CallFrameSlot::argumentCountIncludingThis`) of a live
        // frame; the count is the payload half (`CallFrame.h:287`).
        unsafe {
            self.slot(CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS)
                .payload()
        }
    }

    /// `CallFrame::argumentCount()` (`CallFrame.h:286`):
    /// `argumentCountIncludingThis - 1`.
    ///
    /// # Safety
    /// The frame's slot 4 must be live.
    pub(crate) unsafe fn argument_count(self) -> i32 {
        // SAFETY: as `argument_count_including_this`.
        unsafe { self.argument_count_including_this() - 1 }
    }

    /// The `CallSiteIndex` carried in the TAG half of slot 4
    /// (`CallFrame.h:165-167`, `callSiteIndex()` `CallFrame.h:245`).
    ///
    /// # Safety
    /// The frame's slot 4 must be live.
    pub(crate) unsafe fn call_site_index_bits(self) -> i32 {
        // SAFETY: slot 4 tag half holds the call-site index.
        unsafe {
            self.slot(CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS)
                .tag()
        }
    }

    /// `CallFrame::thisValue()` slot (`CallFrame.h:308-309`, slot 5).
    ///
    /// # Safety
    /// The frame's slot 5 must be live.
    pub(crate) unsafe fn this_value(self) -> Register {
        // SAFETY: slot 5 (`CallFrameSlot::thisArgument`) of a live frame.
        unsafe { self.slot(CallFrameSlot::THIS_ARGUMENT) }
    }

    /// `CallFrame::argument(n)` slot (`CallFrame.h:288`, `firstArgument + n`).
    ///
    /// # Safety
    /// Argument `n` must be inside this frame's argument area.
    pub(crate) unsafe fn argument(self, n: i32) -> Register {
        // SAFETY: argument slot `firstArgument + n` of a live frame.
        unsafe { self.slot(argument_offset(n)) }
    }

    /// The local at index `n` (`localToOperand(n)`, `VirtualRegister.h:111`):
    /// operand `-1 - n`, i.e. byte `-8 * (n + 1)`. Locals grow DOWN.
    ///
    /// # Safety
    /// Local `n` must be inside this frame's local area.
    pub(crate) unsafe fn local(self, n: i32) -> Register {
        // SAFETY: local slot `localToOperand(n)` of a live frame.
        unsafe { self.slot(local_to_operand(n)) }
    }
}

// === JSStack reservation skeleton (CLoopStack.h:48-121) ===

/// `JSStack` — the descending JS value-stack reservation, faithful to
/// `JSC::CLoopStack` (`CLoopStack.h:48-121`).
///
/// Growth is DOWNWARD: `highAddress()` (= `base + size`) is the top where the
/// first frame sits; frames descend toward `m_end`/`reservationTop()`. The
/// invariant (`CLoopStack.h:111-112`) is:
///
/// ```text
/// reservationTop() <= commitTop <= end <= currentStackPointer <= highAddress()
/// ```
///
/// B1 SCOPE: the field bookkeeping, the descending-growth queries, and the
/// provenance GATE, unit-testable over an owned backing. The live mmap
/// reservation, `doVMEntry` seeding, the grow()/commit path, and the real
/// stack-limit computation are B2 (out of scope); the skeleton methods below
/// say so at each site.
pub(crate) struct JsStack {
    /// `m_reservation.base()` (`CLoopStack.h:94`) == `reservationTop()`
    /// (`CLoopStack.h:99-103`): lowest reservation address. EXPOSED ONCE in the
    /// constructor (`precise_allocation.rs:57`); all slot pointers are recovered
    /// from it with `with_exposed_provenance` (`precise_allocation.rs:77`).
    reservation_base: usize,
    /// `m_reservation.size()` in bytes. `highAddress() = base + size`
    /// (`CLoopStack.h:92-95`).
    reservation_size: usize,
    /// `m_commitTop` (`CLoopStack.h:114`): lowest committed address.
    commit_top: usize,
    /// `m_end` (`CLoopStack.h:113`) == `lowAddress()` (`CLoopStack.h:85-88`):
    /// lowest JS-allocatable address.
    end: usize,
    /// `m_currentStackPointer` (`CLoopStack.h:117`).
    current_stack_pointer: usize,
    /// `m_softReservedZoneSizeInRegisters` (`CLoopStack.h:118`).
    soft_reserved_zone_in_registers: isize,
    /// Owns the reservation backing so its once-exposed provenance stays valid
    /// for the lifetime of the gate: the B1 owned `[Register]` test backing, or
    /// (B2) the live mmap reservation whose `Drop` munmaps. Dead in production.
    backing: ReservationBacking,
}

/// Owns the memory backing a [`JsStack`]'s once-exposed reservation provenance.
///
/// `Test` is the B1 owned-heap backing for the offset/gate unit tests. `Mmap`
/// is the live B2 reservation: an immovable RW region (with a low-end guard
/// page) whose `Drop` (via [`reservation::MmapReservation`]) munmaps the whole
/// mapping. Both keep the exposed base valid for as long as the gate may run.
enum ReservationBacking {
    Test(Box<[Register]>),
    #[cfg(unix)]
    Mmap(reservation::MmapReservation),
}

/// Failure modes of the live mmap reservation (B2). These are control flow, not
/// crashes — faithful to JSC, where reservation failure surfaces as a failed
/// allocation rather than aborting (`OSAllocator`/`PageReservation`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JsStackReservationError {
    /// A zero-byte reservation was requested.
    EmptyRequest,
    /// Page-rounding or the guard-page addition overflowed `usize`.
    SizeOverflow,
    /// `getpagesize()` returned a non-positive / non-power-of-two value.
    InvalidPageSize { page_size: i64 },
    /// `mmap(...)` failed (e.g. ENOMEM). The OS errno is captured for triage.
    MmapFailed { errno: Option<i32> },
    /// `mprotect(PROT_NONE)` of the low-end guard page failed.
    MprotectFailed { errno: Option<i32> },
}

/// The single Unix FFI boundary for the live JS-stack mmap reservation.
///
/// C++ ground truth: `CLoopStack::CLoopStack` reserves the stack with a
/// `PageReservation` (`CLoopStack.cpp:62`). On Apple Silicon the faithful
/// realization of a JS *register* stack is a plain anonymous RW mapping — NOT
/// `MAP_JIT`, because this holds JS values (data), not executable code — plus an
/// `mprotect(PROT_NONE)` guard page at the low (growth) end so an overflow that
/// escapes the soft limit faults instead of corrupting neighbouring memory.
/// Mirrors the FFI pattern of `platform/unix_executable_memory.rs:17-51`.
#[cfg(unix)]
mod reservation {
    use super::JsStackReservationError;
    use core::ffi::{c_int, c_void};

    const PROT_NONE: c_int = 0x0;
    const PROT_READ: c_int = 0x1;
    const PROT_WRITE: c_int = 0x2;
    const MAP_PRIVATE: c_int = 0x02;

    // `MAP_ANON` value, per OS family (mirrors
    // `platform/unix_executable_memory.rs:22-35`).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const MAP_ANON: c_int = 0x20;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const MAP_ANON: c_int = 0x1000;

    unsafe extern "C" {
        fn getpagesize() -> c_int;
        fn mmap(
            addr: *mut c_void,
            length: usize,
            prot: c_int,
            flags: c_int,
            fd: c_int,
            offset: i64,
        ) -> *mut c_void;
        fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
        fn munmap(addr: *mut c_void, len: usize) -> c_int;
    }

    fn map_failed() -> *mut c_void {
        usize::MAX as *mut c_void
    }

    fn last_errno() -> Option<i32> {
        std::io::Error::last_os_error().raw_os_error()
    }

    fn page_size() -> Result<usize, JsStackReservationError> {
        // SAFETY: `getpagesize` takes no arguments and only reads a process
        // constant; it cannot violate memory safety.
        let raw = unsafe { getpagesize() };
        if raw <= 0 || !(raw as u32).is_power_of_two() {
            return Err(JsStackReservationError::InvalidPageSize {
                page_size: i64::from(raw),
            });
        }
        Ok(raw as usize)
    }

    /// RAII owner of one live JS-stack mapping `[mmap_base, mmap_base + len)`,
    /// whose low `guard_bytes` are `PROT_NONE` and whose remainder is the RW
    /// allocatable JS register stack. `Drop` munmaps the whole mapping.
    pub(crate) struct MmapReservation {
        mmap_base: *mut c_void,
        mmap_len: usize,
    }

    /// One reserved region: the owner plus the once-exposed allocatable base.
    pub(crate) struct ReservedRegion {
        pub(crate) reservation: MmapReservation,
        /// Lowest *allocatable* address (== `mmap_base + guard_bytes`), with its
        /// provenance already exposed ONCE (`precise_allocation.rs:57`).
        pub(crate) allocatable_base: usize,
        /// Page-rounded allocatable byte length (excludes the guard page).
        pub(crate) allocatable_size: usize,
    }

    impl MmapReservation {
        /// Reserve `allocatable_bytes` (page-rounded) of RW JS stack plus one
        /// low-end `PROT_NONE` guard page, exposing the allocatable base's
        /// provenance once.
        pub(crate) fn reserve(
            allocatable_bytes: usize,
        ) -> Result<ReservedRegion, JsStackReservationError> {
            if allocatable_bytes == 0 {
                return Err(JsStackReservationError::EmptyRequest);
            }
            let page = page_size()?;
            let alloc = allocatable_bytes
                .checked_add(page - 1)
                .map(|v| v & !(page - 1))
                .ok_or(JsStackReservationError::SizeOverflow)?;
            // One guard page at the low (growth-toward) end.
            let guard = page;
            let total = alloc
                .checked_add(guard)
                .ok_or(JsStackReservationError::SizeOverflow)?;

            // SAFETY: null preferred address, non-zero page-rounded length, RW
            // protection, anonymous private mapping, the required `fd=-1` /
            // `offset=0`, and no input pointer to alias. The result is checked
            // against MAP_FAILED/null before any use. NOT `MAP_JIT`: this is the
            // JS value stack (data), not executable code.
            let raw = unsafe {
                mmap(
                    core::ptr::null_mut(),
                    total,
                    PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANON,
                    -1,
                    0,
                )
            };
            if raw == map_failed() || raw.is_null() {
                return Err(JsStackReservationError::MmapFailed {
                    errno: last_errno(),
                });
            }
            // Take ownership immediately so any early return munmaps the mapping.
            let reservation = MmapReservation {
                mmap_base: raw,
                mmap_len: total,
            };

            // SAFETY: `raw` is the live mapping just returned; `guard` (one page)
            // is page-aligned at the base and `<= total`. `PROT_NONE` makes the
            // low guard page fault on any access — the hard backstop below the
            // soft stack limit.
            let rc = unsafe { mprotect(raw, guard, PROT_NONE) };
            if rc != 0 {
                return Err(JsStackReservationError::MprotectFailed {
                    errno: last_errno(),
                });
            }

            // Expose the allocatable base's provenance ONCE
            // (`precise_allocation.rs:57`); the gate recovers slot pointers from
            // it with `with_exposed_provenance_mut`. The whole mapping shares one
            // provenance, so exposing the allocatable base covers every slot in
            // `[allocatable_base, allocatable_base + alloc)`.
            let allocatable_ptr = raw.cast::<u8>().wrapping_add(guard);
            let allocatable_base = allocatable_ptr.expose_provenance();
            Ok(ReservedRegion {
                reservation,
                allocatable_base,
                allocatable_size: alloc,
            })
        }
    }

    impl Drop for MmapReservation {
        fn drop(&mut self) {
            // SAFETY: this owner's single `munmap` of the live mapping it
            // created. Errors are unreportable in `Drop` and harmless (process
            // teardown reclaims).
            unsafe {
                munmap(self.mmap_base, self.mmap_len);
            }
        }
    }
}

impl JsStack {
    /// Build a `JsStack` over an owned `[Register]` backing for B1 unit tests:
    /// fully committed and fully allocatable (`end == reservationTop()`), which
    /// suffices to exercise the offset table and the provenance gate without a
    /// live mmap arena. Exposes the backing's provenance ONCE
    /// (`precise_allocation.rs:57`).
    pub(crate) fn with_test_backing(register_count: usize) -> Self {
        let mut backing: Box<[Register]> =
            vec![Register::default(); register_count].into_boxed_slice();
        // Expose the whole allocation's provenance ONCE; the address is used by
        // the gate to recover slot pointers (precise_allocation.rs:57). Moving
        // the `Box` into the struct below does not move the heap allocation, so
        // the exposed base stays valid.
        let base = backing.as_mut_ptr().expose_provenance();
        let size = register_count * REGISTER_SIZE_IN_BYTES;
        let high = base + size;
        JsStack {
            reservation_base: base,
            reservation_size: size,
            commit_top: base,
            end: base,
            // Empty stack: SP at the top (`highAddress()`); frames descend from
            // here.
            current_stack_pointer: high,
            soft_reserved_zone_in_registers: 0,
            backing: ReservationBacking::Test(backing),
        }
    }

    /// Build a LIVE `JsStack` over a fresh mmap reservation of `size` allocatable
    /// bytes (page-rounded) plus a low-end guard page (B2). Faithful to
    /// `CLoopStack::CLoopStack` (`CLoopStack.cpp:55-72`): reserve, seed
    /// `sp`/`fp` at the high end (the stack grows DOWN), and default the
    /// soft-reserved zone to `Options::softReservedZoneSize()`.
    ///
    /// DIVERGENCE (commented at the field): JSC commits lazily and moves
    /// `m_end`/`m_commitTop` downward on demand via `grow()`
    /// (`CLoopStack.cpp:82-109`). B2 commits the whole reservation up front and
    /// relies on the fixed low-end guard page plus the soft-reserved-zone
    /// software limit ([`Self::stack_limit`]) instead of the lazy grow path; the
    /// grow/commit path is a memory-footprint optimization, not a correctness
    /// requirement, so `m_end == m_commitTop == reservationTop()`.
    #[cfg(unix)]
    pub(crate) fn new(size: usize) -> Result<Self, JsStackReservationError> {
        let region = reservation::MmapReservation::reserve(size)?;
        let base = region.allocatable_base;
        let alloc_size = region.allocatable_size;
        let high = base + alloc_size;
        Ok(JsStack {
            reservation_base: base,
            reservation_size: alloc_size,
            // Full commit up front (see DIVERGENCE above).
            commit_top: base,
            end: base,
            // Empty stack: SP at `highAddress()`; frames descend from here.
            current_stack_pointer: high,
            soft_reserved_zone_in_registers: (DEFAULT_SOFT_RESERVED_ZONE_BYTES
                / REGISTER_SIZE_IN_BYTES) as isize,
            backing: ReservationBacking::Mmap(region.reservation),
        })
    }

    /// [`Self::new`] with the default reservation size
    /// (`Options::maxPerThreadStackUsage()`, [`DEFAULT_JS_STACK_RESERVATION_BYTES`]).
    #[cfg(unix)]
    pub(crate) fn new_default() -> Result<Self, JsStackReservationError> {
        Self::new(DEFAULT_JS_STACK_RESERVATION_BYTES)
    }

    /// `reservationTop()` (`CLoopStack.h:99-103`).
    pub(crate) const fn reservation_top(&self) -> usize {
        self.reservation_base
    }

    /// `lowAddress()` == `m_end` (`CLoopStack.h:85-88`).
    pub(crate) const fn low_address(&self) -> usize {
        self.end
    }

    /// `highAddress()` = `base + size` (`CLoopStack.h:92-95`).
    pub(crate) const fn high_address(&self) -> usize {
        self.reservation_base + self.reservation_size
    }

    /// `size()` = `highAddress() - lowAddress()` (`CLoopStack.h:76`).
    pub(crate) const fn size(&self) -> usize {
        self.high_address() - self.low_address()
    }

    /// `m_commitTop` (`CLoopStack.h:114`).
    pub(crate) const fn commit_top(&self) -> usize {
        self.commit_top
    }

    /// `currentStackPointer()` (`CLoopStack.h:65-73`).
    pub(crate) const fn current_stack_pointer(&self) -> usize {
        self.current_stack_pointer
    }

    /// `setCurrentStackPointer(sp)` (`CLoopStack.h:74`).
    pub(crate) fn set_current_stack_pointer(&mut self, sp: usize) {
        self.current_stack_pointer = sp;
    }

    /// `setSoftReservedZoneSize(...)` (`CLoopStack.h:78`), here in `Register` units.
    pub(crate) fn set_soft_reserved_zone_in_registers(&mut self, registers: isize) {
        self.soft_reserved_zone_in_registers = registers;
    }

    /// `containsAddress(Register*)` (`CLoopStack.h:59`):
    /// `lowAddress() <= addr < highAddress()`.
    pub(crate) const fn contains_address(&self, addr: usize) -> bool {
        self.low_address() <= addr && addr < self.high_address()
    }

    /// The soft stack limit: a frame whose lowest address (its top-of-frame, the
    /// new SP) drops BELOW this signals stack overflow. Faithful to the
    /// `reservationTop() + m_softReservedZoneSizeInRegisters` limit used by
    /// `CLoopStack::isSafeToRecurse` (`CLoopStack.cpp:156-157`) and to the
    /// `VMCLoopStackLimit` that `doVMEntry` compares the new SP against before
    /// copying args (`LowLevelInterpreter64.asm` doVMEntry `.stackHeightOK`).
    /// With B2's full commit `m_end == reservationTop()`, so this equals
    /// `lowAddress() + softReservedZone`.
    pub(crate) fn stack_limit(&self) -> usize {
        let reserved_bytes =
            (self.soft_reserved_zone_in_registers.max(0) as usize) * REGISTER_SIZE_IN_BYTES;
        self.reservation_top() + reserved_bytes
    }

    /// `ensureCapacityFor(newTopOfStack)` (`CLoopStack.h:51-56`): a new top
    /// at/above the committed floor (`m_end`/`lowAddress()`) needs no growth.
    /// With B2's full commit there is never anything to grow, so this is the
    /// pure capacity predicate (the lazy `grow()`/commit path is the documented
    /// divergence on [`Self::new`]).
    pub(crate) fn ensure_capacity_for(&self, new_top_of_stack: usize) -> bool {
        new_top_of_stack >= self.low_address()
    }

    /// `isSafeToRecurse()` (`CLoopStack.cpp:154-158`): the current SP must stay
    /// above the soft-reserved limit ([`Self::stack_limit`]).
    pub(crate) fn is_safe_to_recurse(&self) -> bool {
        self.current_stack_pointer >= self.stack_limit()
    }

    // --- Provenance gate (precise_allocation.rs:57,77) ---

    /// Recover a `*mut Register` for the slot at `addr`, or `None` if `addr` is
    /// outside `[lowAddress(), highAddress())` or is not 8-byte aligned. This is
    /// the SINGLE address->pointer recovery point: the provenance comes from the
    /// base exposed ONCE in [`Self::with_test_backing`]
    /// (`with_exposed_provenance`, `precise_allocation.rs:77`). Recovering the
    /// pointer is safe; the deref happens in [`Self::read_slot`]/[`Self::write_slot`].
    fn register_ptr(&self, addr: usize) -> Option<*mut Register> {
        if !self.contains_address(addr) {
            return None;
        }
        if addr & (REGISTER_SIZE_IN_BYTES - 1) != 0 {
            return None;
        }
        Some(ptr::with_exposed_provenance_mut::<Register>(addr))
    }

    /// Read the `Register` slot at `addr` through the provenance gate.
    pub(crate) fn read_slot(&self, addr: usize) -> Option<Register> {
        let p = self.register_ptr(addr)?;
        // SAFETY: `register_ptr` verified `addr` is 8-aligned and inside the
        // once-exposed reservation `[base, base + size)`; the slot holds an
        // initialized `Register` (the backing is zero-initialized to
        // `Register::default()`). Reading copies 8 POD bytes (no `Drop`).
        Some(unsafe { ptr::read(p) })
    }

    /// Write `value` to the `Register` slot at `addr` through the provenance gate.
    /// Returns `false` if `addr` is out of range or misaligned.
    pub(crate) fn write_slot(&self, addr: usize, value: Register) -> bool {
        match self.register_ptr(addr) {
            Some(p) => {
                // SAFETY: as `read_slot` — `addr` is an 8-aligned slot inside the
                // exposed reservation, and `Register` is a `Copy` POD, so no
                // destructor runs on the overwritten value.
                unsafe { ptr::write(p, value) };
                true
            }
            None => false,
        }
    }

    /// Recover a [`CallFrame`] whose slot-0 address is `frame_addr`: the faithful
    /// analog of `CallFrame::create` (`CallFrame.h:217`) over a gate-recovered
    /// `Register*`. `None` if `frame_addr` is out of range or misaligned.
    pub(crate) fn call_frame_at(&self, frame_addr: usize) -> Option<CallFrame> {
        let p = self.register_ptr(frame_addr)?;
        NonNull::new(p).map(CallFrame::from_registers)
    }

    // --- VM-entry frame seeding (doVMEntry, B2) ---

    /// Seed ONE JS `CallFrame` for a program/function call into the live arena,
    /// faithful to `doVMEntry` (`LowLevelInterpreter64.asm` `doVMEntry`) PLUS the
    /// callee prologue's local reservation (`sub sp, fp, #localsBytes`). This is
    /// the seeding primitive the B3/B4 dual-write feeds; B4 flips the live engine
    /// reads onto the window it establishes.
    ///
    /// Algorithm (matching `doVMEntry` + prologue):
    /// 1. The header + `this` + (padded) args occupy `headerSizeInRegisters +
    ///    paddedArgCount` slots AT/ABOVE `fp` (`addp CallFrameHeaderSlots, t4` /
    ///    `lshiftp 3`); `fp` (the new `CallFrame*`) is `current_sp - aboveFpBytes`
    ///    (`subp sp, t4, t3`). The new SP is then lowered a further
    ///    `callee_local_count` slots for the locals/temporaries that grow DOWN
    ///    from `fp` (the prologue reservation, brought forward for B4).
    /// 2. STACK-LIMIT GUARD (the mandatory bound): reject — as a `Result`,
    ///    writing NOTHING — if the new SP (lowest occupied address, incl. locals)
    ///    would drop below [`Self::stack_limit`] (`bpaeq t3, VMCLoopStackLimit`).
    ///    This runs BEFORE any slot write, so an over-deep push never touches the
    ///    guard page.
    /// 3. Copy the header words from the `ProtoCallFrame`-shaped inputs into the
    ///    callee frame: `codeBlock`/`callee`/`argumentCountIncludingThis`/`this`
    ///    at slots 2..5 (`copyHeaderLoop`), and the caller-frame/return-PC pair
    ///    at slots 0..1 (which `doVMEntry`'s callee prologue + call instruction
    ///    write; the seeding primitive writes them directly).
    /// 4. Fill `undefined` into the padded-but-unprovided arg slots
    ///    (`fillExtraArgsLoop`), copy the real args (`copyArgsLoop`), then
    ///    undefined-fill the reserved locals (operands -1 .. -callee_local_count).
    /// 5. Publish the new SP (`move t3, sp`), now below the reserved locals.
    ///
    /// On success the new `CallFrame` (slot-0 / `fp`) is returned and
    /// `current_stack_pointer` is lowered to `fp - callee_local_count*8`. On any
    /// error nothing is written and the SP is unchanged.
    pub(crate) fn try_seed_entry_frame(
        &mut self,
        seed: &VmEntryFrameSeed<'_>,
    ) -> Result<CallFrame, JsStackPushError> {
        // doVMEntry: `loadi PayloadOffset + argCountAndCodeOriginValue; subi 1`.
        let count_including_this = seed.argument_count_including_this.payload();
        if count_including_this < 1 {
            return Err(JsStackPushError::InvalidArgumentCount {
                count_including_this,
            });
        }
        let real_args = (count_including_this - 1) as usize;
        if seed.arguments.len() != real_args {
            return Err(JsStackPushError::ArgumentCountMismatch {
                count_including_this,
                provided: seed.arguments.len(),
            });
        }
        // paddedArgCount must cover the real (incl-this) count (it is the
        // alignment-rounded incl-this count; `fillExtraArgsLoop` fills the gap).
        if (seed.padded_argument_count as i32) < count_including_this {
            return Err(JsStackPushError::PaddedArgumentCountTooSmall {
                count_including_this,
                padded: seed.padded_argument_count,
            });
        }

        // Step 1: frame size and new CallFrame address. The header + `this` +
        // (padded) arguments sit AT/ABOVE `fp` (`CallFrame.h:176-181`); `fp` (the
        // new `CallFrame*`, slot 0) is the current SP minus that above-`fp`
        // footprint.
        let above_fp_registers = (HEADER_SIZE_IN_REGISTERS as usize)
            .checked_add(seed.padded_argument_count as usize)
            .ok_or(JsStackPushError::AddressOverflow)?;
        let above_fp_bytes = above_fp_registers
            .checked_mul(REGISTER_SIZE_IN_BYTES)
            .ok_or(JsStackPushError::AddressOverflow)?;
        let new_call_frame = self
            .current_stack_pointer
            .checked_sub(above_fp_bytes)
            .ok_or(JsStackPushError::AddressOverflow)?;

        // B4 FULL-WINDOW reservation: the callee's LOCALS/temporaries grow DOWN
        // from `fp` (operand -1 -> `fp-8`, ..., `VirtualRegister::localToOperand`
        // `VirtualRegister.h:111`). Reserve `callee_local_count` slots below `fp`
        // so the new SP is the LOWEST address the frame occupies and a nested
        // callee seeds strictly BELOW this frame's locals (no overlap). This
        // brings forward the prologue's `sub sp, fp, #localsBytes`: the live
        // `RegisterFile::allocate_frame` reserves locals + args in ONE step, so
        // the faithful arena mirror reserves the full window here rather than
        // splitting entry-seeding from a later prologue.
        let locals_bytes = (seed.callee_local_count as usize)
            .checked_mul(REGISTER_SIZE_IN_BYTES)
            .ok_or(JsStackPushError::AddressOverflow)?;
        let new_stack_pointer = new_call_frame
            .checked_sub(locals_bytes)
            .ok_or(JsStackPushError::AddressOverflow)?;

        // Step 2: MANDATORY stack-limit guard, BEFORE any write. The guard checks
        // the LOWEST occupied address (the new SP, including the reserved locals).
        // With `callee_local_count == 0` this is exactly `new_call_frame`, the
        // pre-B4 doVMEntry check.
        let limit = self.stack_limit();
        if new_stack_pointer < limit {
            return Err(JsStackPushError::StackOverflow {
                new_top_of_frame: new_stack_pointer,
                limit,
            });
        }

        // Recover the frame pointer through the provenance gate. This also
        // re-verifies `new_call_frame` is in `[lowAddress, highAddress)` and
        // 8-aligned; the limit check already guaranteed the former, and an
        // 8-aligned `current_sp` minus an 8-multiple stays 8-aligned.
        let frame = self
            .call_frame_at(new_call_frame)
            .ok_or(JsStackPushError::AddressOverflow)?;

        // Steps 3-4: write the frame. SAFETY for every `set_slot` below: the
        // frame occupies `[new_stack_pointer, current_sp)`. The above-`fp` slots
        // (header/`this`/args) span `[new_call_frame, current_sp)`; the highest is
        // `firstArgument + (paddedArgCount - 2)` == `current_sp - 8` `< highAddress`
        // (`current_sp <= highAddress`). The reserved-locals slots span
        // `[new_stack_pointer, new_call_frame)`, all `>= limit >= lowAddress` and
        // `< fp`. So every written slot lies inside the once-exposed reservation,
        // is 8-aligned, and holds (zeroed) POD `Register` storage; `set_slot`
        // overwrites a `Copy` POD (no `Drop`). The frame pointer carries the
        // whole reservation's provenance (exposed once), valid below `fp` too.
        unsafe {
            // Slots 0..1: caller-frame (EntryFrame sentinel) and return PC.
            frame.set_slot(CallFrameSlot::CALLER_FRAME, seed.caller_frame_or_entry);
            frame.set_slot(CallFrameSlot::RETURN_PC, seed.return_pc);
            // Slots 2..5: copyHeaderLoop (codeBlock, callee, argCount, this).
            frame.set_slot(CallFrameSlot::CODE_BLOCK, seed.code_block);
            frame.set_slot(CallFrameSlot::CALLEE, seed.callee);
            frame.set_slot(
                CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS,
                seed.argument_count_including_this,
            );
            frame.set_slot(CallFrameSlot::THIS_ARGUMENT, seed.this_value);

            // fillExtraArgsLoop: `undefined` into the padded-but-unprovided arg
            // slots `firstArgument + [real_args .. paddedArgCount - 1)`.
            let undefined = Register::from_encoded(JsValue::undefined().encoded());
            for i in real_args..(seed.padded_argument_count as usize - 1) {
                frame.set_slot(argument_offset(i as i32), undefined);
            }
            // copyArgsLoop: the real args at `firstArgument + [0 .. real_args)`.
            for (i, arg) in seed.arguments.iter().enumerate() {
                frame.set_slot(argument_offset(i as i32), *arg);
            }

            // B4 FULL-WINDOW reservation: undefined-fill the reserved locals
            // (operands -1 .. -callee_local_count, i.e. `fp-8` .. SP) so an
            // unwritten local reads back `undefined`, matching the live
            // `RegisterFile::allocate_frame`'s `RuntimeValue::undefined()` local
            // fill (the analog of `op_enter` clearing callee locals,
            // `LowLevelInterpreter64.asm` op_enter -> `op_enter` undefined loop).
            for local in 0..(seed.callee_local_count as i32) {
                frame.set_slot(local_to_operand(local), undefined);
            }
        }

        // Step 5: publish the new SP (`move t3, sp`), now below the reserved
        // locals (== `new_call_frame` when there are no locals).
        self.current_stack_pointer = new_stack_pointer;
        Ok(frame)
    }
}

/// One JS call-frame's worth of raw `Register` inputs to seed via
/// [`JsStack::try_seed_entry_frame`], faithful to the words `doVMEntry` copies
/// from a `ProtoCallFrame` (`interpreter/ProtoCallFrame.h:48-54`) plus the
/// caller-frame/return-PC pair that the callee prologue + call write.
///
/// All fields carry RAW NaN-boxed/`Register` bits (the arena is untyped 8-byte
/// storage); typed/ID recovery is the gate's and the live model's job, not the
/// seed's. `caller_frame_or_entry` is the slot-0 value `doVMEntry` leaves as the
/// `EntryFrame` sentinel (`callerFrameOrEntryFrame`, `CallFrame.h:224`).
pub(crate) struct VmEntryFrameSeed<'a> {
    /// Slot 0 — `callerFrame` / EntryFrame sentinel (`CallFrame.h:110,224`).
    pub(crate) caller_frame_or_entry: Register,
    /// Slot 1 — `returnPC` (`CallFrame.h:111`).
    pub(crate) return_pc: Register,
    /// Slot 2 — `codeBlock` (`ProtoCallFrame::codeBlockValue`,
    /// `ProtoCallFrame.h:48`).
    pub(crate) code_block: Register,
    /// Slot 3 — `callee` (`ProtoCallFrame::calleeValue`, `ProtoCallFrame.h:49`).
    pub(crate) callee: Register,
    /// Slot 4 — `argumentCountIncludingThis` (payload) + `CodeOrigin` (tag)
    /// (`ProtoCallFrame::argCountAndCodeOriginValue`, `ProtoCallFrame.h:50`).
    pub(crate) argument_count_including_this: Register,
    /// Slot 5 — `this` (`ProtoCallFrame::thisArg`, `ProtoCallFrame.h:51`).
    pub(crate) this_value: Register,
    /// The real arguments (excluding `this`): `ProtoCallFrame::args`
    /// (`ProtoCallFrame.h:54`). Length must equal `argumentCountIncludingThis - 1`.
    pub(crate) arguments: &'a [Register],
    /// `ProtoCallFrame::paddedArgCount` (`ProtoCallFrame.h:53`): the
    /// alignment-rounded argument-count-including-this driving the above-`fp`
    /// frame size.
    pub(crate) padded_argument_count: u32,
    /// The callee's local/temporary register count — `CodeBlock::m_numCalleeLocals`
    /// (the locals that grow DOWN from `fp`, `VirtualRegister.h:111`). B4 reserves
    /// and undefined-fills exactly this many slots below `fp` so EVERY
    /// VirtualRegister the interpreter addresses (locals AND args) has an arena
    /// slot and nested callees do not overlap the locals. Driven from the live
    /// `RegisterWindow::local_count` (`RegisterFile::allocate_frame`).
    pub(crate) callee_local_count: u32,
}

/// Why [`JsStack::try_seed_entry_frame`] refused to seed a frame. Every variant
/// means NOTHING was written and the SP is unchanged. `StackOverflow` is the
/// mandatory guard firing (`doVMEntry .stackCheckFailed` ->
/// `_llint_throw_stack_overflow_error_from_vm_entry`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JsStackPushError {
    /// The push would drop the new SP below the soft stack limit.
    StackOverflow {
        new_top_of_frame: usize,
        limit: usize,
    },
    /// `argumentCountIncludingThis` (slot-4 payload) was `< 1`.
    InvalidArgumentCount { count_including_this: i32 },
    /// The provided `arguments` slice length disagreed with
    /// `argumentCountIncludingThis - 1`.
    ArgumentCountMismatch {
        count_including_this: i32,
        provided: usize,
    },
    /// `paddedArgCount` was smaller than `argumentCountIncludingThis`.
    PaddedArgumentCountTooSmall {
        count_including_this: i32,
        padded: u32,
    },
    /// Frame-size or new-SP arithmetic overflowed / underflowed `usize`.
    AddressOverflow,
}

// === B3 dual-write shadow (the bridge between the live model and the arena) ===

/// One live frame's bookkeeping in the [`JsStackShadow`]: the arena address of
/// its `CallFrame*` (slot 0) and the arena SP to restore when it is released.
///
/// This is the Rust-port bridge between the live model's abstract
/// `CallFrameId(u32)` identity (`runtime::interpreter::CallFrameId`) and the
/// arena's native `CallFrame*` address. JSC has no such table — the `CallFrame*`
/// IS the identity (`CallFrame.h:189`); the table exists only because the live
/// Rust model still keys frames by an out-of-line `u32` (the divergence B6
/// retires). It mirrors `RegisterFile::windows` (a LIFO `Vec`) so the arena SP
/// moves in lockstep with the value-stack truncation in `release_frame`.
struct ShadowFrameRecord {
    /// `runtime::interpreter::CallFrameId(u32)` of the live frame this mirrors.
    frame_id: u32,
    /// Arena address of slot 0 (the `CallFrame*` / `fp`), i.e.
    /// `CallFrame::registers()` (`CallFrame.h:218`).
    frame_address: usize,
    /// `JsStack::current_stack_pointer` BEFORE this frame was seeded; restored on
    /// release so the arena SP rises in lockstep with the live frame pop
    /// (the analog of `RegisterFile::values.truncate(window.base)`).
    sp_before: usize,
}

/// Why [`JsStackShadow::release_frame`] could not release a frame. Both variants
/// mean the live model and the arena shadow have diverged from strict LIFO
/// nesting; the dual-write caller responds by DISABLING the shadow (never by
/// panicking or altering control flow), keeping B3 behavior-neutral.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShadowReleaseError {
    /// The released frame id is not the top of the shadow stack (non-LIFO pop).
    NotTopOfStack {
        expected_top: Option<u32>,
        requested: u32,
    },
    /// The shadow frame stack was empty when a release was requested.
    Empty { requested: u32 },
}

/// A read-back image of a seeded arena frame's header slots + `this`, in RAW
/// bits, for the B3 cross-check. Copied out (POD) so the comparison borrows
/// nothing from the arena. Mirrors the slots `doVMEntry` writes
/// (`CallFrame.h:176-181`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShadowFrameImage {
    /// Slot 0 — `callerFrameOrEntryFrame` (`CallFrame.h:224`).
    pub(crate) caller_frame_bits: usize,
    /// Slot 1 — `returnPC` (`CallFrame.h:234`).
    pub(crate) return_pc_bits: usize,
    /// Slot 2 — `codeBlock` (`CallFrame.h:204-205`).
    pub(crate) code_block_bits: usize,
    /// Slot 3 — `callee` raw `CalleeBits` (`CallFrame.h:202`).
    pub(crate) callee_bits: usize,
    /// Slot 4 payload — `argumentCountIncludingThis` (`CallFrame.h:287`).
    pub(crate) argument_count_including_this: i32,
    /// Slot 4 tag — `CallSiteIndex` (`CallFrame.h:165-167,245`).
    pub(crate) call_site_index_bits: i32,
    /// Slot 5 — `this` (`CallFrame.h:308-309`).
    pub(crate) this_value: Register,
}

/// The DUAL-WRITE SHADOW: a [`JsStack`] arena that mirrors every frame the live
/// `ExecutionContextStack`/`RegisterFile` model pushes. As of B4 it is the
/// register window the live engine READS from (`RegisterFile::read`), with the
/// `Vec` kept as a cross-checked oracle.
///
/// ## What the shadow writes (the FULL window, B4)
///
/// On each live `push_frame`, the dual-write seeds ONE arena `CallFrame` via the
/// `doVMEntry`-shaped primitive ([`JsStack::try_seed_entry_frame`]): the 5 header
/// slots + `this` + arguments at/above `fp` (`CallFrame.h:176-181`) AND the
/// callee's locals/temporaries reserved + undefined-filled BELOW `fp` (operands
/// -1 .. -callee_local_count, `VirtualRegister.h:111`). The new SP descends past
/// the locals (the prologue's `sub sp, fp, #localsBytes`, brought forward), so
/// across nested pushes each callee seeds STRICTLY BELOW the caller's locals with
/// no overlap, and EVERY VirtualRegister the interpreter reads/writes has an
/// arena slot. [`Self::frame_register_at`]/[`Self::write_frame_register`] are the
/// gate-checked accessors `RegisterFile` uses for the read-flip.
///
/// ## Reversible + cross-checked
///
/// The `Vec` is still dual-written (`RegisterFile::write` writes BOTH), so the
/// read-flip stays REVERSIBLE and every arena read is debug-cross-checked against
/// the `Vec` oracle. The dual-write must NEVER change control flow: if the arena
/// cannot hold a frame (overflow) or LIFO nesting desyncs, the OWNER
/// (`RegisterFile`) DISABLES the shadow and reads fall back to the `Vec`, so
/// behavior is identical. (B4b/B6 drop the `Vec` once proven green suite-wide.)
pub(crate) struct JsStackShadow {
    /// The descending arena the live frames are mirrored into (B2).
    stack: JsStack,
    /// LIFO record of every currently-live mirrored frame, mirroring
    /// `RegisterFile::windows`. `push` appends, `release_frame` pops the top.
    frames: Vec<ShadowFrameRecord>,
}

impl core::fmt::Debug for JsStackShadow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Terse: do not recurse into the mmap reservation / raw arena bytes.
        f.debug_struct("JsStackShadow")
            .field("depth", &self.frames.len())
            .field("current_stack_pointer", &self.stack.current_stack_pointer())
            .finish()
    }
}

impl JsStackShadow {
    /// Build a live shadow over a fresh default-size arena
    /// (`Options::maxPerThreadStackUsage()`, 5 MiB). Returns `None` if the arena
    /// cannot be reserved (e.g. `mmap` failure) or on a non-`unix` target, in
    /// which case the owner leaves the shadow disabled and the dual-write is a
    /// no-op (behavior-neutral).
    pub(crate) fn new() -> Option<Self> {
        #[cfg(unix)]
        {
            JsStack::new_default().ok().map(|stack| JsStackShadow {
                stack,
                frames: Vec::new(),
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Seed one mirrored frame from `seed` and record its `frame_id` ->
    /// arena-address mapping. Returns the seeded slot-0 [`FrameAddress`], or
    /// `None` if the arena refused the push (overflow / bad args) — on `None`
    /// the owner disables the shadow. The arena SP descends by exactly the
    /// `doVMEntry` frame size.
    pub(crate) fn seed_frame(
        &mut self,
        frame_id: u32,
        seed: &VmEntryFrameSeed<'_>,
    ) -> Option<FrameAddress> {
        let sp_before = self.stack.current_stack_pointer();
        match self.stack.try_seed_entry_frame(seed) {
            Ok(frame) => {
                let frame_address = frame.registers().as_ptr() as usize;
                self.frames.push(ShadowFrameRecord {
                    frame_id,
                    frame_address,
                    sp_before,
                });
                Some(FrameAddress(frame_address))
            }
            Err(_) => None,
        }
    }

    /// Release the top mirrored frame, restoring the arena SP to its pre-push
    /// value (lockstep with the live `release_frame`'s `values.truncate`). The
    /// `frame_id` MUST be the top of the LIFO stack, mirroring
    /// `RegisterFile::release_frame`'s `windows.pop()` identity check.
    pub(crate) fn release_frame(&mut self, frame_id: u32) -> Result<(), ShadowReleaseError> {
        match self.frames.last() {
            Some(top) if top.frame_id == frame_id => {
                let sp_before = top.sp_before;
                self.frames.pop();
                self.stack.set_current_stack_pointer(sp_before);
                Ok(())
            }
            Some(top) => Err(ShadowReleaseError::NotTopOfStack {
                expected_top: Some(top.frame_id),
                requested: frame_id,
            }),
            None => Err(ShadowReleaseError::Empty {
                requested: frame_id,
            }),
        }
    }

    /// `CallFrameId(u32)` -> arena `FrameAddress`: the forward side-table lookup.
    pub(crate) fn address_of(&self, frame_id: u32) -> Option<FrameAddress> {
        self.frames
            .iter()
            .find(|record| record.frame_id == frame_id)
            .map(|record| FrameAddress(record.frame_address))
    }

    /// Arena `FrameAddress` -> `CallFrameId(u32)`: the reverse side-table lookup.
    pub(crate) fn frame_id_at(&self, address: FrameAddress) -> Option<u32> {
        self.frames
            .iter()
            .find(|record| record.frame_address == address.0)
            .map(|record| record.frame_id)
    }

    /// Number of currently-live mirrored frames (mirrors
    /// `RegisterFile::windows.len()`).
    pub(crate) fn depth(&self) -> usize {
        self.frames.len()
    }

    /// `JsStack::current_stack_pointer` — for the lockstep-SP cross-check.
    pub(crate) fn current_stack_pointer(&self) -> usize {
        self.stack.current_stack_pointer()
    }

    /// Read back the header slots + `this` of a frame this shadow seeded, for the
    /// B3 cross-check. `None` if `address` is not a frame this shadow currently
    /// holds. Confines the arena `unsafe` reads here (the interpreter cross-check
    /// calls this safe API).
    pub(crate) fn frame_header_image(&self, address: FrameAddress) -> Option<ShadowFrameImage> {
        if !self.holds(address) {
            return None;
        }
        let frame = self.stack.call_frame_at(address.0)?;
        // SAFETY: `address` is a frame THIS shadow seeded via
        // `try_seed_entry_frame`, which wrote slots 0..=5 (header + `this`); those
        // slots lie inside the live arena window `[address, sp_before)` and a
        // nested callee occupies a strictly lower, disjoint region, so they are
        // still live and initialized. The reads copy POD `Register` bits.
        unsafe {
            Some(ShadowFrameImage {
                caller_frame_bits: frame.caller_frame_bits(),
                return_pc_bits: frame.return_pc_bits(),
                code_block_bits: frame.code_block_bits(),
                callee_bits: frame.callee().raw_ptr(),
                argument_count_including_this: frame.argument_count_including_this(),
                call_site_index_bits: frame.call_site_index_bits(),
                this_value: frame.this_value(),
            })
        }
    }

    /// Recover slot 2 of a frame this shadow seeded as a real `*const CodeBlock`
    /// (the cfr-relative `[fp+16]` read), for the op_call/JIT path that lands
    /// later. `None` if `address` is not a frame this shadow currently holds.
    /// Purely additive: no existing site reads slot 2 as a pointer. The returned
    /// pointer's dereferenceability follows [`CallFrame::code_block_ptr`] — it is
    /// the registry's stable `CodeBlock*` only for frames whose slot 2 was K1
    /// seeded from a registered owner.
    pub(crate) fn frame_code_block_ptr(&self, address: FrameAddress) -> Option<*const CodeBlock> {
        if !self.holds(address) {
            return None;
        }
        let frame = self.stack.call_frame_at(address.0)?;
        // SAFETY: as `frame_header_image`; slot 2 lies inside the live arena
        // window this shadow seeded and holds POD `Register` bits.
        Some(unsafe { frame.code_block_ptr() })
    }

    /// Read back argument `index` (0-based, EXCLUDING `this`) of a frame this
    /// shadow seeded, for the B3 cross-check. `None` if `address` is unknown.
    pub(crate) fn frame_argument(&self, address: FrameAddress, index: i32) -> Option<Register> {
        if !self.holds(address) {
            return None;
        }
        let frame = self.stack.call_frame_at(address.0)?;
        // SAFETY: as `frame_header_image`; argument `index` is `firstArgument +
        // index` (`CallFrame.h:288`), inside the seeded frame's argument area.
        Some(unsafe { frame.argument(index) })
    }

    /// `true` if `address` is the slot-0 (`fp`) of a frame this shadow currently
    /// holds (a live mirrored frame).
    fn holds(&self, address: FrameAddress) -> bool {
        self.frames
            .iter()
            .any(|record| record.frame_address == address.0)
    }

    /// B4 read-flip: read the value `Register` at FP-relative `operand` within the
    /// frame whose slot-0 is `address`, through the provenance gate. `operand` is
    /// the VirtualRegister's raw value (locals negative, header/`this`/args
    /// non-negative); the byte address is `fp + operand*8` ([`frame_slot_addr`]).
    /// `None` if `address` is not a held frame or the slot is out of the
    /// reservation / misaligned. The full window is reserved at seed time, so any
    /// in-frame operand resolves to a live slot.
    pub(crate) fn frame_register_at(
        &self,
        address: FrameAddress,
        operand: i32,
    ) -> Option<Register> {
        if !self.holds(address) {
            return None;
        }
        let addr = frame_slot_addr(address.0, operand)?;
        // The gate re-verifies `addr` is 8-aligned and inside `[low, high)`.
        self.stack.read_slot(addr)
    }

    /// B4 read-flip: write `value` to the value `Register` at FP-relative
    /// `operand` within the frame whose slot-0 is `address` (the dual-write half
    /// of [`Self::frame_register_at`]). Returns `false` if `address` is not a held
    /// frame or the slot is out of range; the caller (`RegisterFile::write`) has
    /// already written the `Vec` oracle, so a `false` here is behavior-neutral.
    pub(crate) fn write_frame_register(
        &self,
        address: FrameAddress,
        operand: i32,
        value: Register,
    ) -> bool {
        if !self.holds(address) {
            return false;
        }
        let Some(addr) = frame_slot_addr(address.0, operand) else {
            return false;
        };
        self.stack.write_slot(addr, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_frame_slot_offsets_are_byte_exact() {
        // Slot indices: enum class CallFrameSlot (CallFrame.h:176-181), with
        // CallerFrameAndPC::sizeInRegisters = 2 (CallFrame.h:112).
        assert_eq!(CallFrameSlot::CALLER_FRAME, 0);
        assert_eq!(CallFrameSlot::RETURN_PC, 1);
        assert_eq!(CallFrameSlot::CODE_BLOCK, 2);
        assert_eq!(CallFrameSlot::CALLEE, 3);
        assert_eq!(CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS, 4);
        assert_eq!(CallFrameSlot::THIS_ARGUMENT, 5);
        assert_eq!(CallFrameSlot::FIRST_ARGUMENT, 6);
        // headerSizeInRegisters = argumentCountIncludingThis + 1 (CallFrame.h:191).
        assert_eq!(HEADER_SIZE_IN_REGISTERS, 5);

        // Byte offsets = operand * sizeof(Register) (VirtualRegister.h:79).
        assert_eq!(operand_offset_in_bytes(CallFrameSlot::CALLER_FRAME), 0);
        assert_eq!(operand_offset_in_bytes(CallFrameSlot::RETURN_PC), 8);
        assert_eq!(operand_offset_in_bytes(CallFrameSlot::CODE_BLOCK), 16);
        assert_eq!(operand_offset_in_bytes(CallFrameSlot::CALLEE), 24);
        assert_eq!(
            operand_offset_in_bytes(CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS),
            32
        );
        assert_eq!(operand_offset_in_bytes(CallFrameSlot::THIS_ARGUMENT), 40);
        assert_eq!(operand_offset_in_bytes(CallFrameSlot::FIRST_ARGUMENT), 48);

        // argN @ +48 + 8*N (CallFrame.h:288).
        assert_eq!(operand_offset_in_bytes(argument_offset(0)), 48);
        assert_eq!(operand_offset_in_bytes(argument_offset(3)), 48 + 24);

        // Locals grow DOWN: local0 @ operand -1 @ byte -8 (VirtualRegister.h:111).
        assert_eq!(local_to_operand(0), -1);
        assert_eq!(operand_offset_in_bytes(local_to_operand(0)), -8);
        assert_eq!(operand_offset_in_bytes(local_to_operand(1)), -16);
        assert_eq!(operand_to_local(-1), 0);
    }

    #[test]
    fn register_is_pod_and_round_trips_raw_bits() {
        // Register.h:244: trivially memcpy-movable 8-byte POD.
        assert_eq!(core::mem::size_of::<Register>(), 8);

        let bits = 0x1234_5678_9abc_def0u64;
        let r = Register::from_bits(bits);
        assert_eq!(r.bits(), bits);
        // payload = low 32, tag = high 32 (EncodedValueDescriptor.h:52-55 LE;
        // Register.h:210-218).
        assert_eq!(r.payload() as u32, 0x9abc_def0);
        assert_eq!(r.tag() as u32, 0x1234_5678);
        // unboxedInt64 = u.integer (Register.h:137-140).
        assert_eq!(r.unboxed_int64(), bits as i64);

        // An int32 boxed via the live JSVALUE64 encoding reads back through i()
        // (Register.h:61 / unboxedInt32 :112-115).
        let int_reg = Register::from_encoded(JsValue::from_i32(-7).encoded());
        assert_eq!(int_reg.i(), -7);

        // A double round-trips through unboxed_double bits (Register.h:164-167).
        let d = Register::from_bits(1.5f64.to_bits());
        assert_eq!(d.unboxed_double(), 1.5);

        // The encoded view is preserved (Register.h:53).
        assert_eq!(r.encoded_js_value(), EncodedJsValue(bits));
    }

    #[test]
    fn callee_bits_distinguishes_cell_from_native_callee() {
        // JSCJSValue.h:490-491: NativeCalleeTag = OtherTag|1 = 0x3,
        // NativeCalleeMask = NumberTag|0x7. CalleeBits.h:152-176.

        // A plain cell pointer (8-aligned, no number/other bits) is a cell.
        let cell = CalleeBits::from_bits(0x4_0000);
        assert!(cell.is_cell());
        assert!(!cell.is_native_callee());
        assert_eq!(cell.as_cell(), 0x4_0000);

        // A boxed native callee carries the 0x3 tag (boxNativeCallee,
        // CalleeBits.h:140-146) and unboxes back (asNativeCallee, :168-176).
        let lowest = 0usize;
        let native_ptr = 0x10_0000usize; // 8-aligned
        let boxed = CalleeBits::box_native_callee(native_ptr, lowest);
        assert!(boxed.is_native_callee());
        assert!(!boxed.is_cell());
        assert_eq!(boxed.raw_ptr(), native_ptr | 0x3);
        assert_eq!(boxed.as_native_callee(lowest), native_ptr);

        // The round-trip also holds with a non-zero lowestAccessibleAddress.
        let lowest = 0x8000usize;
        let boxed = CalleeBits::box_native_callee(native_ptr, lowest);
        assert!(boxed.is_native_callee());
        assert_eq!(boxed.as_native_callee(lowest), native_ptr);

        // nullCallee is a cell (CalleeBits.h:104-107,160).
        assert!(CalleeBits::null().is_cell());
    }

    #[test]
    fn provenance_gate_slot_read_write_round_trips() {
        // precise_allocation.rs:57,77 provenance pattern over an owned backing.
        let stack = JsStack::with_test_backing(16);
        let base = stack.reservation_top();
        assert_eq!(stack.size(), 16 * 8);
        assert_eq!(stack.high_address(), base + 16 * 8);

        let slot3 = base + 3 * 8;
        assert!(stack.contains_address(slot3));

        // Write then read back through the gate.
        let written = Register::from_bits(0xdead_beef_0000_0042);
        assert!(stack.write_slot(slot3, written));
        assert_eq!(stack.read_slot(slot3), Some(written));

        // Out-of-range (== highAddress) and misaligned addresses are rejected.
        assert_eq!(stack.read_slot(stack.high_address()), None);
        assert_eq!(stack.read_slot(base + 1), None);
        assert!(!stack.write_slot(base + 1, written));
    }

    #[test]
    fn call_frame_header_accessors_over_gate_recovered_frame() {
        // CallFrame.h:189,202,287; a frame whose slot-0 sits mid-backing so a
        // local (negative offset) stays in range.
        let stack = JsStack::with_test_backing(16);
        let base = stack.reservation_top();
        let frame_addr = base + 4 * 8;

        // argumentCountIncludingThis (slot+4): payload = 3 (=> argCount 2), tag =
        // CallSiteIndex 9 (CallFrame.h:165-167,287).
        let arg_count_bits = (9u64 << 32) | 3u64;
        let arg_count_addr =
            frame_addr + (CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS as usize) * 8;
        assert!(stack.write_slot(arg_count_addr, Register::from_bits(arg_count_bits)));

        // callee (slot+3): a plain cell pointer (CallFrame.h:202).
        let callee_addr = frame_addr + (CallFrameSlot::CALLEE as usize) * 8;
        assert!(stack.write_slot(callee_addr, Register::from_bits(0x5_0000)));

        let frame = stack.call_frame_at(frame_addr).expect("frame in range");
        assert_eq!(frame.registers().as_ptr() as usize, frame_addr);

        // SAFETY: the header slots (+3,+4), this (+5), and one local (-1) of this
        // frame are all inside the 16-slot backing.
        unsafe {
            assert_eq!(frame.argument_count_including_this(), 3);
            assert_eq!(frame.argument_count(), 2);
            assert_eq!(frame.call_site_index_bits(), 9);
            assert!(frame.callee().is_cell());
            assert_eq!(frame.callee().as_cell(), 0x5_0000);

            // Locals grow DOWN: write local0 (operand -1) and read it back.
            frame.set_slot(local_to_operand(0), Register::from_bits(0x77));
            assert_eq!(frame.local(0).bits(), 0x77);
        }
    }
}

// Live B2 tests over a real mmap reservation. Gated to `unix` because
// `JsStack::new`/the guard page require `mmap`/`mprotect`.
#[cfg(all(test, unix))]
mod live_tests {
    use super::*;
    use crate::bytecode::register::CallFrameSlotLayout;
    use crate::gc::CellId;
    use crate::interpreter::{FrameState, InstalledCallFrame, RegisterWindow};
    use crate::runtime::{CallFrameId, CodeBlockId, EntryFrameId, ObjectId};

    /// A live reservation with the soft-reserved zone disabled so small frames
    /// fit; the low-end guard PAGE still backs the reservation floor.
    fn live_stack(allocatable_bytes: usize) -> JsStack {
        let mut stack = JsStack::new(allocatable_bytes).expect("mmap reservation");
        stack.set_soft_reserved_zone_in_registers(0);
        stack
    }

    #[test]
    fn live_reservation_round_trips_slot_write_read() {
        let stack = JsStack::new(64 * 1024).expect("mmap reservation");
        let base = stack.reservation_top();
        // Page-rounded to at least the request; high = base + size; m_end == base.
        assert!(stack.size() >= 64 * 1024);
        assert_eq!(stack.high_address(), base + stack.size());
        assert_eq!(stack.low_address(), base);
        assert_eq!(stack.commit_top(), base);

        // Write/read a slot across the live mmap via the provenance gate.
        let slot = base + 7 * REGISTER_SIZE_IN_BYTES;
        let value = Register::from_bits(0x0123_4567_89ab_cdef);
        assert!(stack.write_slot(slot, value));
        assert_eq!(stack.read_slot(slot), Some(value));

        // The slot just below highAddress is in range; highAddress itself is not.
        assert!(stack
            .read_slot(stack.high_address() - REGISTER_SIZE_IN_BYTES)
            .is_some());
        assert_eq!(stack.read_slot(stack.high_address()), None);
        // Anonymous mmap is zero-filled: untouched slots read as Register::default.
        assert_eq!(stack.read_slot(base), Some(Register::default()));
    }

    #[test]
    fn stack_limit_guard_rejects_over_deep_push_without_writing() {
        // One-page reservation keeps the DEFAULT 128 KiB soft zone, whose limit
        // sits far above highAddress, so ANY push overflows the soft limit.
        let mut stack = JsStack::new(4096).expect("mmap reservation");
        let limit = stack.stack_limit();
        assert!(limit > stack.high_address());

        let sp_before = stack.current_stack_pointer();
        let high = stack.high_address();
        // argumentCountIncludingThis = 1 (just `this`), padded = 1 -> frame = 6 regs.
        let seed = VmEntryFrameSeed {
            caller_frame_or_entry: Register::from_bits(0xEE),
            return_pc: Register::from_bits(0xBB),
            code_block: Register::from_bits(0xCB),
            callee: Register::from_bits(0xCA),
            argument_count_including_this: Register::from_bits(1),
            this_value: Register::from_encoded(JsValue::from_i32(1).encoded()),
            arguments: &[],
            padded_argument_count: 1,
            callee_local_count: 0,
        };
        let frame_bytes = (HEADER_SIZE_IN_REGISTERS as usize + 1) * REGISTER_SIZE_IN_BYTES;
        let would_be_base = high - frame_bytes;

        let err = stack.try_seed_entry_frame(&seed).unwrap_err();
        assert_eq!(
            err,
            JsStackPushError::StackOverflow {
                new_top_of_frame: would_be_base,
                limit,
            }
        );
        // The guard fired BEFORE any write: SP unchanged, target slot still zero.
        assert_eq!(stack.current_stack_pointer(), sp_before);
        assert_eq!(stack.read_slot(would_be_base), Some(Register::default()));
    }

    #[test]
    fn seeded_frame_is_byte_identical_to_installed_call_frame_model() {
        let mut stack = live_stack(64 * 1024);
        let sp_before = stack.current_stack_pointer();

        // --- One ground-truth representative call feeds BOTH the arena seeding
        // and the equivalent InstalledCallFrame model ---
        let entry_bits = 0x0000_7fff_0000_0010u64; // EntryFrame sentinel (slot 0)
        let return_pc_bits = 0x0000_7fff_0000_0020u64;
        let code_block_bits = 0x0000_0001_0000_0040u64; // CodeBlock* (cell)
        let callee_bits = 0x0000_0001_0000_0080u64; // JSCell* callee (8-aligned)
        let this_value = JsValue::from_i32(7);
        let args = [JsValue::from_i32(11), JsValue::from_i32(13)];
        let count_including_this: i32 = 1 + args.len() as i32; // this + 2 = 3
        let code_origin: u32 = 0; // VM entry: no call-site index in the slot-4 tag
        let arg_count_bits = ((code_origin as u64) << 32) | (count_including_this as u32 as u64);
        let padded = count_including_this as u32; // round(3) == 3: no undefined fill

        let arg_regs: Vec<Register> = args
            .iter()
            .map(|value| Register::from_encoded(value.encoded()))
            .collect();
        let seed = VmEntryFrameSeed {
            caller_frame_or_entry: Register::from_bits(entry_bits),
            return_pc: Register::from_bits(return_pc_bits),
            code_block: Register::from_bits(code_block_bits),
            callee: Register::from_bits(callee_bits),
            argument_count_including_this: Register::from_bits(arg_count_bits),
            this_value: Register::from_encoded(this_value.encoded()),
            arguments: &arg_regs,
            padded_argument_count: padded,
            // B4: this entry-frame test exercises only the above-`fp` window
            // (sp == fp after seeding), so no locals are reserved.
            callee_local_count: 0,
        };

        let frame = stack
            .try_seed_entry_frame(&seed)
            .expect("seed program frame");
        let frame_base = frame.registers().as_ptr() as usize;

        // SP descended by EXACTLY the doVMEntry frame size (header + paddedArgs).
        let frame_bytes =
            (HEADER_SIZE_IN_REGISTERS as usize + padded as usize) * REGISTER_SIZE_IN_BYTES;
        assert_eq!(frame_base, sp_before - frame_bytes);
        assert_eq!(stack.current_stack_pointer(), frame_base);

        // --- The equivalent live InstalledCallFrame for the SAME call ---
        let layout = CallFrameSlotLayout::JSC_RUST;
        let installed = InstalledCallFrame {
            id: CallFrameId(1),
            entry: Some(EntryFrameId(1)), // entered from the VM entry...
            caller: None,                 // ...no JS caller frame above it
            code_block: Some(CodeBlockId(CellId(0x40))),
            callee: Some(ObjectId(CellId(0x80))),
            callee_value: None,
            lexical_scope: None,
            bytecode_index: None,
            return_address: None,
            return_continuation: None,
            argument_count_including_this: count_including_this as u32,
            register_window: RegisterWindow {
                owner: CallFrameId(1),
                base: frame_base,
                local_count: 0,
                // thisArgument is the first value slot after the 5-slot header.
                argument_base: frame_base
                    + (layout.this_argument_offset.0 as usize) * REGISTER_SIZE_IN_BYTES,
                argument_count: count_including_this as usize,
                this_offset: layout.this_argument_offset,
            },
            state: FrameState::Executing,
        };

        // (a) Header raw bits round-trip across the live mmap; (b) the callee
        // header agrees with the model (a non-null cell callee); (c) the count
        // slot is byte-identical to the model's count and the tag is the origin.
        // SAFETY: every accessed slot is inside the just-seeded frame window.
        unsafe {
            assert_eq!(frame.caller_frame_bits(), entry_bits as usize);
            assert_eq!(frame.return_pc_bits(), return_pc_bits as usize);
            assert_eq!(frame.code_block_bits(), code_block_bits as usize);
            assert!(installed.callee.is_some());
            assert!(frame.callee().is_cell());
            assert_eq!(frame.callee().as_cell(), callee_bits as usize);
            assert_eq!(
                frame.argument_count_including_this() as u32,
                installed.argument_count_including_this
            );
            assert_eq!(frame.call_site_index_bits() as u32, code_origin);
        }

        // (d) The model's entry/caller framing matches the seeded slot-0 sentinel.
        assert!(installed.entry.is_some());
        assert!(installed.caller.is_none());

        // (e) `this` + args are byte-identical (decode to the SAME JsValues) AND
        // land at the addresses the InstalledCallFrame's register window names.
        let this_addr =
            frame_base + (layout.this_argument_offset.0 as usize) * REGISTER_SIZE_IN_BYTES;
        assert_eq!(this_addr, installed.register_window.argument_base);
        assert_eq!(stack.read_slot(this_addr).unwrap().js_value(), this_value);
        for (index, arg) in args.iter().enumerate() {
            // Window slot 0 is `this`; arg i is at window slot (1 + i).
            let arg_addr =
                installed.register_window.argument_base + (1 + index) * REGISTER_SIZE_IN_BYTES;
            assert_eq!(stack.read_slot(arg_addr).unwrap().js_value(), *arg);
        }
        assert_eq!(installed.register_window.argument_count, args.len() + 1);
    }

    #[test]
    fn seeded_frame_fills_undefined_for_padded_arguments() {
        let mut stack = live_stack(64 * 1024);
        let sp_before = stack.current_stack_pointer();

        let this_value = JsValue::from_i32(5);
        // 1 real arg -> argumentCountIncludingThis = 2. round(2): (2 + 5) aligned
        // to 2 -> 8, minus 5 -> paddedArgCount = 3, i.e. 1 undefined fill slot.
        let args = [JsValue::from_i32(9)];
        let count_including_this: i32 = 1 + args.len() as i32;
        let padded: u32 = 3;
        let arg_regs: Vec<Register> = args
            .iter()
            .map(|value| Register::from_encoded(value.encoded()))
            .collect();
        let seed = VmEntryFrameSeed {
            caller_frame_or_entry: Register::from_bits(0),
            return_pc: Register::from_bits(0),
            code_block: Register::from_bits(0),
            callee: Register::from_bits(0),
            argument_count_including_this: Register::from_bits(count_including_this as u64),
            this_value: Register::from_encoded(this_value.encoded()),
            arguments: &arg_regs,
            padded_argument_count: padded,
            callee_local_count: 0,
        };

        let frame = stack.try_seed_entry_frame(&seed).expect("seed");
        let base = frame.registers().as_ptr() as usize;
        let undefined = JsValue::undefined();

        // this @ slot 5, real arg0 @ slot 6, undefined fill @ slot 7.
        assert_eq!(
            stack
                .read_slot(base + 5 * REGISTER_SIZE_IN_BYTES)
                .unwrap()
                .js_value(),
            this_value
        );
        assert_eq!(
            stack
                .read_slot(base + 6 * REGISTER_SIZE_IN_BYTES)
                .unwrap()
                .js_value(),
            args[0]
        );
        assert_eq!(
            stack
                .read_slot(base + 7 * REGISTER_SIZE_IN_BYTES)
                .unwrap()
                .js_value(),
            undefined
        );
        // Frame size = header(5) + padded(3) = 8 regs; SP lowered to the new base.
        assert_eq!(stack.current_stack_pointer(), base);
        assert_eq!(
            base + (HEADER_SIZE_IN_REGISTERS as usize + padded as usize) * REGISTER_SIZE_IN_BYTES,
            sp_before
        );
    }

    #[test]
    fn seed_rejects_argument_count_mismatch_without_writing() {
        let mut stack = live_stack(64 * 1024);
        let sp_before = stack.current_stack_pointer();
        // Slot-4 payload says 3 (this + 2), but only 1 arg is provided.
        let arg = [Register::from_encoded(JsValue::from_i32(1).encoded())];
        let seed = VmEntryFrameSeed {
            caller_frame_or_entry: Register::from_bits(0),
            return_pc: Register::from_bits(0),
            code_block: Register::from_bits(0),
            callee: Register::from_bits(0),
            argument_count_including_this: Register::from_bits(3),
            this_value: Register::from_encoded(JsValue::undefined().encoded()),
            arguments: &arg,
            padded_argument_count: 3,
            callee_local_count: 0,
        };
        assert_eq!(
            stack.try_seed_entry_frame(&seed).unwrap_err(),
            JsStackPushError::ArgumentCountMismatch {
                count_including_this: 3,
                provided: 1,
            }
        );
        assert_eq!(stack.current_stack_pointer(), sp_before);
    }

    #[test]
    fn seed_reserves_and_undefined_fills_callee_locals_below_fp() {
        // B4 FULL-WINDOW reservation: locals grow DOWN from `fp`; the seed
        // reserves + undefined-fills `callee_local_count` slots and lowers SP a
        // further `local_count*8` past them. The above-`fp` window is untouched.
        let mut stack = live_stack(64 * 1024);
        let sp_before = stack.current_stack_pointer();
        let local_count: u32 = 3;
        let this_value = JsValue::from_i32(7);
        let args = [JsValue::from_i32(11)];
        let count_including_this = 1 + args.len() as u32; // this + 1 = 2
        let arg_regs: Vec<Register> = args
            .iter()
            .map(|value| Register::from_encoded(value.encoded()))
            .collect();
        let seed = VmEntryFrameSeed {
            caller_frame_or_entry: Register::from_bits(0),
            return_pc: Register::from_bits(0),
            code_block: Register::from_bits(0),
            callee: Register::from_bits(0),
            argument_count_including_this: Register::from_bits(count_including_this as u64),
            this_value: Register::from_encoded(this_value.encoded()),
            arguments: &arg_regs,
            padded_argument_count: count_including_this,
            callee_local_count: local_count,
        };
        let frame = stack.try_seed_entry_frame(&seed).expect("seed with locals");
        let fp = frame.registers().as_ptr() as usize;

        // `fp = sp_before - (header + paddedArgs)*8`; the new SP is `fp - locals*8`.
        let above = (HEADER_SIZE_IN_REGISTERS as usize + count_including_this as usize)
            * REGISTER_SIZE_IN_BYTES;
        assert_eq!(fp, sp_before - above);
        assert_eq!(
            stack.current_stack_pointer(),
            fp - local_count as usize * REGISTER_SIZE_IN_BYTES
        );

        // Every reserved local reads back `undefined` at `fp - 8*(i+1)`
        // (operand -1-i), via the FP-relative mapping `frame_slot_addr`.
        let undefined = JsValue::undefined();
        for i in 0..local_count as i32 {
            let addr = frame_slot_addr(fp, local_to_operand(i)).unwrap();
            assert_eq!(addr, (fp as isize - (i as isize + 1) * 8) as usize);
            assert_eq!(stack.read_slot(addr).unwrap().js_value(), undefined);
        }
        // `this` @ fp+40 and arg0 @ fp+48 are still the above-`fp` values.
        assert_eq!(
            stack
                .read_slot(frame_slot_addr(fp, CallFrameSlot::THIS_ARGUMENT).unwrap())
                .unwrap()
                .js_value(),
            this_value
        );
        assert_eq!(
            stack
                .read_slot(frame_slot_addr(fp, CallFrameSlot::FIRST_ARGUMENT).unwrap())
                .unwrap()
                .js_value(),
            args[0]
        );
    }

    #[test]
    fn shadow_full_window_read_write_round_trips_every_vreg() {
        // B4 read-flip: the arena window is read/written via
        // `frame_register_at` / `write_frame_register` for locals (operand < 0),
        // `this` (operand 5) and args (operand >= 6), through the provenance gate.
        let mut shadow = JsStackShadow::new().expect("shadow arena");
        let this_value = JsValue::from_i32(100);
        let args = [JsValue::from_i32(101), JsValue::from_i32(102)];
        let count_including_this = 1 + args.len() as u32; // 3
        let local_count: u32 = 4;
        let arg_regs: Vec<Register> = args
            .iter()
            .map(|value| Register::from_encoded(value.encoded()))
            .collect();
        let seed = VmEntryFrameSeed {
            caller_frame_or_entry: Register::from_bits(0),
            return_pc: Register::from_bits(0),
            code_block: Register::from_bits(0),
            callee: Register::from_bits(0),
            argument_count_including_this: Register::from_bits(count_including_this as u64),
            this_value: Register::from_encoded(this_value.encoded()),
            arguments: &arg_regs,
            padded_argument_count: count_including_this,
            callee_local_count: local_count,
        };
        let addr = shadow.seed_frame(7, &seed).expect("seed mirrored frame");

        // Initial reads: header `this`/args, and undefined locals.
        assert_eq!(
            shadow.frame_register_at(addr, 5).unwrap().js_value(),
            this_value
        );
        assert_eq!(
            shadow.frame_register_at(addr, 6).unwrap().js_value(),
            args[0]
        );
        assert_eq!(
            shadow.frame_register_at(addr, 7).unwrap().js_value(),
            args[1]
        );
        for i in 0..local_count as i32 {
            assert_eq!(
                shadow.frame_register_at(addr, -1 - i).unwrap().js_value(),
                JsValue::undefined()
            );
        }

        // Writes round-trip at every value register (locals and args alike).
        for (operand, payload) in [(-1i32, 200), (-4, 203), (5, 300), (6, 301), (7, 302)] {
            let value = JsValue::from_i32(payload);
            assert!(shadow.write_frame_register(
                addr,
                operand,
                Register::from_encoded(value.encoded())
            ));
            assert_eq!(
                shadow.frame_register_at(addr, operand).unwrap().js_value(),
                value
            );
        }

        // An address that is not a held frame is rejected (None / false).
        let bogus = FrameAddress(addr.0 + REGISTER_SIZE_IN_BYTES);
        assert!(shadow.frame_register_at(bogus, -1).is_none());
        assert!(!shadow.write_frame_register(bogus, -1, Register::from_bits(0)));
    }
}
