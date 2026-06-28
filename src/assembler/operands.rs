//! MacroAssembler operand value types.
//!
//! Faithful port of the "Section 1: MacroAssembler operand types" structs in
//! `Source/JavaScriptCore/assembler/AbstractMacroAssembler.h:135-424`: the
//! address forms (`Address`, `BaseIndex`, `PreIndexAddress`, `PostIndexAddress`,
//! `AbsoluteAddress`), the `Scale` enum, and the immediate wrapper types
//! (`TrustedImm32`/`Imm32`, `TrustedImm64`/`Imm64`, `TrustedImmPtr`/`ImmPtr`).
//!
//! All of these are `Copy` stack-owned value types with no allocation, exactly
//! like the C++ structs. They carry no behavior beyond construction; the
//! MacroAssembler layer (a later port) consumes them to choose addressing-mode
//! encodings.
#![allow(dead_code)]

use super::registers::RegisterID;

/// Index scaling for `BaseIndex`.
///
/// C++ map: `AbstractMacroAssembler::Scale` (AbstractMacroAssembler.h:140-147).
/// The variants are a plain enum (`TimesOne..TimesEight`); JSC deliberately
/// dropped the literal x86 SIB multiplicands in 2009 (see
/// `mcts_mem/.../assembler.md` 2009-02-04) so the operand model stays portable.
/// `ScalePtr`/`ScaleRegWord` are pointer-width aliases; on the 64-bit targets
/// this port cares about they resolve to `TimesEight`, exposed as `const`s
/// below rather than enum variants (Rust cannot duplicate a discriminant).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Scale {
    TimesOne = 0,
    TimesTwo = 1,
    TimesFour = 2,
    TimesEight = 3,
}

impl Scale {
    /// `Scale::ScalePtr` (AbstractMacroAssembler.h:145) on a 64-bit address
    /// space (`isAddress64Bit()` true) selects `TimesEight`.
    pub const SCALE_PTR: Scale = Scale::TimesEight;
    /// `Scale::ScaleRegWord` (AbstractMacroAssembler.h:146) on a 64-bit register
    /// width (`isRegister64Bit()` true) selects `TimesEight`.
    pub const SCALE_REG_WORD: Scale = Scale::TimesEight;

    /// log2 of the scale, i.e. the shift amount a `BaseIndex` applies to its
    /// index register (`TimesEight` -> shift 3).
    #[inline]
    pub const fn log2(self) -> u32 {
        self as u32
    }
}

/// ARM64 index extend for `BaseIndex` (32/64-bit, signed/unsigned/none).
///
/// C++ map: `AbstractMacroAssembler::Extend` (AbstractMacroAssembler.h:149-153).
/// Only meaningful on ARM64; other targets assert `Extend::None`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Extend {
    ZExt32 = 0,
    SExt32 = 1,
    #[default]
    None = 2,
}

/// Simple base+offset memory operand.
///
/// C++ map: `AbstractMacroAssembler::Address` (AbstractMacroAssembler.h:169-192).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Address {
    pub base: RegisterID,
    pub offset: i32,
}

impl Address {
    /// `Address(RegisterID base, int32_t offset = 0)`
    /// (AbstractMacroAssembler.h:170).
    #[inline]
    pub const fn new(base: RegisterID, offset: i32) -> Self {
        Self { base, offset }
    }

    /// `Address::withOffset` (AbstractMacroAssembler.h:176-179).
    #[inline]
    pub const fn with_offset(self, additional_offset: i32) -> Self {
        Self {
            base: self.base,
            offset: self.offset.wrapping_add(additional_offset),
        }
    }

    /// `Address::indexedBy` (AbstractMacroAssembler.h:186) — promote to a
    /// `BaseIndex` keeping this address' base and offset.
    #[inline]
    pub const fn indexed_by(self, index: RegisterID, scale: Scale) -> BaseIndex {
        BaseIndex::new(self.base, index, scale, self.offset)
    }
}

/// Base+offset operand with a pointer-width (`intptr_t`) offset.
///
/// C++ map: `AbstractMacroAssembler::ExtendedAddress`
/// (AbstractMacroAssembler.h:194-205).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExtendedAddress {
    pub base: RegisterID,
    pub offset: isize,
}

impl ExtendedAddress {
    /// `ExtendedAddress(RegisterID base, intptr_t offset = 0)`
    /// (AbstractMacroAssembler.h:195).
    #[inline]
    pub const fn new(base: RegisterID, offset: isize) -> Self {
        Self { base, offset }
    }
}

/// Complex base+index*scale+offset memory operand.
///
/// C++ map: `AbstractMacroAssembler::BaseIndex`
/// (AbstractMacroAssembler.h:210-240).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BaseIndex {
    pub base: RegisterID,
    pub index: RegisterID,
    pub scale: Scale,
    pub offset: i32,
    pub extend: Extend,
}

impl BaseIndex {
    /// `BaseIndex(base, index, scale, offset = 0, extend = None)`
    /// (AbstractMacroAssembler.h:211).
    #[inline]
    pub const fn new(base: RegisterID, index: RegisterID, scale: Scale, offset: i32) -> Self {
        Self {
            base,
            index,
            scale,
            offset,
            extend: Extend::None,
        }
    }

    /// `BaseIndex` with an explicit ARM64 index `Extend`
    /// (AbstractMacroAssembler.h:211).
    #[inline]
    pub const fn with_extend(
        base: RegisterID,
        index: RegisterID,
        scale: Scale,
        offset: i32,
        extend: Extend,
    ) -> Self {
        Self {
            base,
            index,
            scale,
            offset,
            extend,
        }
    }

    /// `BaseIndex::withOffset` (AbstractMacroAssembler.h:223-226).
    #[inline]
    pub const fn with_offset(self, additional_offset: i32) -> Self {
        Self {
            offset: self.offset.wrapping_add(additional_offset),
            ..self
        }
    }
}

/// Base address with a pre-increment/decrement index.
///
/// C++ map: `AbstractMacroAssembler::PreIndexAddress`
/// (AbstractMacroAssembler.h:245-254). The `index` field is the signed byte
/// displacement applied to `base` *before* the access (and written back).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PreIndexAddress {
    pub base: RegisterID,
    pub index: i32,
}

impl PreIndexAddress {
    /// `PreIndexAddress(RegisterID base, int index)`
    /// (AbstractMacroAssembler.h:246).
    #[inline]
    pub const fn new(base: RegisterID, index: i32) -> Self {
        Self { base, index }
    }
}

/// Base address with a post-increment/decrement index.
///
/// C++ map: `AbstractMacroAssembler::PostIndexAddress`
/// (AbstractMacroAssembler.h:259-268). The `index` field is the signed byte
/// displacement applied to `base` *after* the access (and written back).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PostIndexAddress {
    pub base: RegisterID,
    pub index: i32,
}

impl PostIndexAddress {
    /// `PostIndexAddress(RegisterID base, int index)`
    /// (AbstractMacroAssembler.h:260).
    #[inline]
    pub const fn new(base: RegisterID, index: i32) -> Self {
        Self { base, index }
    }
}

/// Memory operand given by an absolute pointer.
///
/// C++ map: `AbstractMacroAssembler::AbsoluteAddress`
/// (AbstractMacroAssembler.h:274-281). JSC stores a raw `const void*`; this port
/// keeps the pointer value as `usize` so the operand stays a plain `Copy` value
/// with no provenance obligations (it is metadata for the encoder, never
/// dereferenced here).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AbsoluteAddress {
    pub ptr: usize,
}

impl AbsoluteAddress {
    /// `AbsoluteAddress(const void* ptr)` (AbstractMacroAssembler.h:275).
    #[inline]
    pub const fn new(ptr: usize) -> Self {
        Self { ptr }
    }
}

/// A 32-bit immediate that is trusted not to be a JS value bit pattern.
///
/// C++ map: `AbstractMacroAssembler::TrustedImm32`
/// (AbstractMacroAssembler.h:354-370).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TrustedImm32 {
    pub value: i32,
}

impl TrustedImm32 {
    /// `explicit TrustedImm32(int32_t value)` (AbstractMacroAssembler.h:357).
    #[inline]
    pub const fn new(value: i32) -> Self {
        Self { value }
    }

    /// `explicit TrustedImm32(TrustedImmPtr ptr)`
    /// (AbstractMacroAssembler.h:363, non-x86_64): narrow a pointer-sized
    /// immediate to 32 bits.
    #[inline]
    pub const fn from_ptr(ptr: TrustedImmPtr) -> Self {
        Self {
            value: ptr.value as i32,
        }
    }
}

/// A 32-bit immediate that may be a JS value bit pattern (poison/blinding
/// candidate). C++ map: `AbstractMacroAssembler::Imm32`
/// (AbstractMacroAssembler.h:373-386).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Imm32 {
    inner: TrustedImm32,
}

impl Imm32 {
    /// `explicit Imm32(int32_t value)` (AbstractMacroAssembler.h:374).
    #[inline]
    pub const fn new(value: i32) -> Self {
        Self {
            inner: TrustedImm32::new(value),
        }
    }

    /// `Imm32::asTrustedImm32` (AbstractMacroAssembler.h:384).
    #[inline]
    pub const fn as_trusted_imm32(self) -> TrustedImm32 {
        self.inner
    }
}

/// A 64-bit immediate that is trusted not to be a JS value bit pattern.
///
/// C++ map: `AbstractMacroAssembler::TrustedImm64`
/// (AbstractMacroAssembler.h:394-410).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TrustedImm64 {
    pub value: i64,
}

impl TrustedImm64 {
    /// `explicit TrustedImm64(int64_t value)` (AbstractMacroAssembler.h:397).
    #[inline]
    pub const fn new(value: i64) -> Self {
        Self { value }
    }

    /// `explicit TrustedImm64(TrustedImmPtr ptr)`
    /// (AbstractMacroAssembler.h:403, ARM64): widen a pointer-sized immediate to
    /// 64 bits.
    #[inline]
    pub const fn from_ptr(ptr: TrustedImmPtr) -> Self {
        Self {
            value: ptr.value as i64,
        }
    }
}

/// A 64-bit immediate that may be a JS value bit pattern. C++ map:
/// `AbstractMacroAssembler::Imm64` (AbstractMacroAssembler.h:412-424).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Imm64 {
    inner: TrustedImm64,
}

impl Imm64 {
    /// `explicit Imm64(int64_t value)` (AbstractMacroAssembler.h:413).
    #[inline]
    pub const fn new(value: i64) -> Self {
        Self {
            inner: TrustedImm64::new(value),
        }
    }

    /// `Imm64::asTrustedImm64` (AbstractMacroAssembler.h:423).
    #[inline]
    pub const fn as_trusted_imm64(self) -> TrustedImm64 {
        self.inner
    }
}

/// A pointer-sized trusted immediate.
///
/// C++ map: `AbstractMacroAssembler::TrustedImmPtr`
/// (AbstractMacroAssembler.h:294-336). JSC wraps a `const void*` to distinguish
/// an immediate-valued pointer from an `AbsoluteAddress` memory operand; this
/// port stores the integer value (`asIntptr`) as `isize`, matching the targets'
/// pointer width without holding live provenance.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TrustedImmPtr {
    pub value: isize,
}

impl TrustedImmPtr {
    /// `explicit TrustedImmPtr(const void* value)`
    /// (AbstractMacroAssembler.h:297), via the integer `asIntptr` view.
    #[inline]
    pub const fn new(value: isize) -> Self {
        Self { value }
    }

    /// `TrustedImmPtr::asIntptr` (AbstractMacroAssembler.h:325).
    #[inline]
    pub const fn as_intptr(self) -> isize {
        self.value
    }
}

/// A pointer-sized immediate that may be a JS value bit pattern. C++ map:
/// `AbstractMacroAssembler::ImmPtr` (AbstractMacroAssembler.h:338-346).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImmPtr {
    inner: TrustedImmPtr,
}

impl ImmPtr {
    /// `explicit ImmPtr(const void* value)` (AbstractMacroAssembler.h:340).
    #[inline]
    pub const fn new(value: isize) -> Self {
        Self {
            inner: TrustedImmPtr::new(value),
        }
    }

    /// `ImmPtr::asTrustedImmPtr` (AbstractMacroAssembler.h:345).
    #[inline]
    pub const fn as_trusted_imm_ptr(self) -> TrustedImmPtr {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_log2_matches_jsc_enum_order() {
        // AbstractMacroAssembler.h:140-144 — TimesOne..TimesEight are 0..3, the
        // log2 of the byte multiplier.
        assert_eq!(Scale::TimesOne.log2(), 0);
        assert_eq!(Scale::TimesTwo.log2(), 1);
        assert_eq!(Scale::TimesFour.log2(), 2);
        assert_eq!(Scale::TimesEight.log2(), 3);
        assert_eq!(Scale::SCALE_PTR, Scale::TimesEight);
        assert_eq!(Scale::SCALE_REG_WORD, Scale::TimesEight);
    }

    #[test]
    fn address_with_offset_and_indexed_by() {
        let addr = Address::new(RegisterID::Fp, 8);
        assert_eq!(addr.with_offset(8), Address::new(RegisterID::Fp, 16));

        let bi = addr.indexed_by(RegisterID::X2, Scale::TimesEight);
        assert_eq!(
            bi,
            BaseIndex::new(RegisterID::Fp, RegisterID::X2, Scale::TimesEight, 8)
        );
    }

    #[test]
    fn base_index_defaults_extend_none() {
        let bi = BaseIndex::new(RegisterID::X0, RegisterID::X1, Scale::TimesFour, -4);
        assert_eq!(bi.extend, Extend::None);
        assert_eq!(bi.with_offset(8).offset, 4);
    }

    #[test]
    fn immediate_wrappers_carry_value() {
        assert_eq!(TrustedImm32::new(-7).value, -7);
        assert_eq!(Imm32::new(42).as_trusted_imm32().value, 42);
        assert_eq!(TrustedImm64::new(1 << 40).value, 1 << 40);
        assert_eq!(Imm64::new(-3).as_trusted_imm64().value, -3);
        assert_eq!(TrustedImmPtr::new(0x1000).as_intptr(), 0x1000);
        assert_eq!(ImmPtr::new(0x2000).as_trusted_imm_ptr().value, 0x2000);

        // TrustedImm32 narrows / TrustedImm64 widens a pointer immediate,
        // mirroring the cross-constructors (AbstractMacroAssembler.h:363,403).
        let ptr = TrustedImmPtr::new(0x1234_5678);
        assert_eq!(TrustedImm32::from_ptr(ptr).value, 0x1234_5678);
        assert_eq!(TrustedImm64::from_ptr(ptr).value, 0x1234_5678);
    }
}
