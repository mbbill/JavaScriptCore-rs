//! Native-thread-stack JS call-frame substrate (JSStack migration B1).
//!
//! ## What this is
//!
//! The faithful C++-JSC TYPES and FP-relative offset table for the JavaScript
//! call stack, plus a reservation-bookkeeping skeleton and the single
//! provenance gate that recovers a `*Register` from a raw slot address. This is
//! the ADDITIVE, parallel-safe FOUNDATION for moving JS execution onto the
//! native thread stack. It is `dead_code` and is NOT wired into the live
//! dispatch path: the live engine still uses the abstract
//! `RegisterFile`/`runtime::interpreter::CallFrame` model. The owner-gated
//! cutovers (B4/B6) are out of scope here, as are the B2 mmap reservation +
//! `doVMEntry` seeding + stack-limit guard and the B3 dual-write.
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

use crate::value::{EncodedJsValue, JsValue};

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
    /// Owns the B1 test backing so its once-exposed provenance stays valid for
    /// the lifetime of the gate. `None` for the live B2 mmap reservation. Dead in
    /// production.
    _backing: Option<Box<[Register]>>,
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
            _backing: Some(backing),
        }
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

    /// `ensureCapacityFor(newTopOfStack)` (`CLoopStack.h:57`).
    ///
    /// B1 SKELETON: with a fully-committed test backing this only checks the new
    /// top stays at/above `m_end` (`lowAddress()`). The real `grow()`/commit path
    /// (`CLoopStack.cpp grow`) that extends `m_commitTop` toward
    /// `reservationTop()` is B2.
    pub(crate) fn ensure_capacity_for(&self, new_top_of_stack: usize) -> bool {
        new_top_of_stack >= self.low_address()
    }

    /// `isSafeToRecurse()` (`CLoopStack.h:79`).
    ///
    /// B1 SKELETON: the live test compares `currentStackPointer` against a
    /// soft-reserved limit above `m_end`. Here the limit is
    /// `lowAddress() + softReservedZone`; the full JSC limit derivation (which
    /// also accounts for the reserved zone and the C++ stack origin) is B2.
    pub(crate) fn is_safe_to_recurse(&self) -> bool {
        let reserved_bytes =
            (self.soft_reserved_zone_in_registers.max(0) as usize) * REGISTER_SIZE_IN_BYTES;
        self.current_stack_pointer >= self.low_address() + reserved_bytes
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
