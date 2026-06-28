//! ARM64 register identifiers and ABI role metadata.
//!
//! Faithful port of `Source/JavaScriptCore/assembler/ARM64Registers.h` (the
//! `FOR_EACH_GP_REGISTER` / `FOR_EACH_FP_REGISTER` / `FOR_EACH_REGISTER_ALIAS`
//! tables) and the register-id enums declared in
//! `Source/JavaScriptCore/assembler/ARM64Assembler.h:171-198`
//! (`ARM64Registers::RegisterID` / `FPRegisterID`).
//!
//! These are pure stack-owned value enums: no allocation, `Copy`, mirroring the
//! C++ `enum : int8_t`. JSC types the enum as `int8_t` (so it can carry the
//! `InvalidGPRReg = -1` sentinel); every *live* register discriminant is in
//! `0..=63`, and only the low 5 bits ever reach an instruction field, so this
//! port uses `#[repr(u8)]`. The sentinel `-1` values are intentionally omitted
//! (they are never encoded).
#![allow(dead_code)]

/// General-purpose register identifier.
///
/// C++ map: `ARM64Registers::RegisterID` (ARM64Assembler.h:171-184) generated
/// from `FOR_EACH_GP_REGISTER` (ARM64Registers.h:44-125) plus the aliases in
/// `FOR_EACH_REGISTER_ALIAS` (ARM64Registers.h:128-133). `x0..x28` take
/// discriminants `0..28`; the three "special" slots are `fp=29`, `lr=30`,
/// `sp=31`; `zr` is the dedicated alias `0x3f` (ARM64Registers.h:133) which the
/// encoder masks to the register field `31` via `xOrZr`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RegisterID {
    X0 = 0,
    X1 = 1,
    X2 = 2,
    X3 = 3,
    X4 = 4,
    X5 = 5,
    X6 = 6,
    X7 = 7,
    X8 = 8,
    X9 = 9,
    X10 = 10,
    X11 = 11,
    X12 = 12,
    X13 = 13,
    X14 = 14,
    X15 = 15,
    X16 = 16,
    X17 = 17,
    X18 = 18,
    X19 = 19,
    X20 = 20,
    X21 = 21,
    X22 = 22,
    X23 = 23,
    X24 = 24,
    X25 = 25,
    X26 = 26,
    X27 = 27,
    X28 = 28,
    /// `fp` / frame pointer; alias `x29`. ARM64Registers.h:82,131.
    Fp = 29,
    /// `lr` / link register; alias `x30`. ARM64Registers.h:83,132.
    Lr = 30,
    /// `sp` / stack pointer. ARM64Registers.h:84. Register field `31` decodes as
    /// `SP` in `xOrSp` contexts and `ZR` in `xOrZr` contexts.
    Sp = 31,
    /// Zero register `zr`; ARM64Registers.h:133 gives it the value `0x3f`, which
    /// `xOrZr` (`reg & 31`) collapses to the `31` register field.
    Zr = 0x3f,
}

impl RegisterID {
    /// `x29` is the JSC alias of `fp` (ARM64Registers.h:131).
    pub const X29: RegisterID = RegisterID::Fp;
    /// `x30` is the JSC alias of `lr` (ARM64Registers.h:132).
    pub const X30: RegisterID = RegisterID::Lr;
    /// Intra-procedure-call scratch register alias `ip0 == x16`
    /// (ARM64Registers.h:129).
    pub const IP0: RegisterID = RegisterID::X16;
    /// Intra-procedure-call scratch register alias `ip1 == x17`
    /// (ARM64Registers.h:130).
    pub const IP1: RegisterID = RegisterID::X17;

    /// Raw enum discriminant (the JSC `RegisterID` integer value).
    #[inline]
    pub const fn value(self) -> u32 {
        self as u32
    }

    /// `ARM64Registers::isSp` (ARM64Registers.h:200).
    #[inline]
    pub const fn is_sp(self) -> bool {
        matches!(self, RegisterID::Sp)
    }

    /// `ARM64Registers::isZr` (ARM64Registers.h:201).
    #[inline]
    pub const fn is_zr(self) -> bool {
        matches!(self, RegisterID::Zr)
    }

    /// AAPCS64 callee-saved GPR per the `cs` column of `FOR_EACH_GP_REGISTER`
    /// (ARM64Registers.h:71-84): `x19..x28` and `fp`. `lr` is deliberately NOT
    /// in this set (ARM64Registers.h:39-42 keeps it out of the callee-save set
    /// because the return-address save/restore is handled separately).
    #[inline]
    pub const fn is_callee_saved(self) -> bool {
        matches!(
            self,
            RegisterID::X19
                | RegisterID::X20
                | RegisterID::X21
                | RegisterID::X22
                | RegisterID::X23
                | RegisterID::X24
                | RegisterID::X25
                | RegisterID::X26
                | RegisterID::X27
                | RegisterID::X28
                | RegisterID::Fp
        )
    }
}

/// AAPCS64 parameter / result registers `x0..x7`
/// (ARM64Registers.h:46-54 "Parameter/result registers").
pub const ARGUMENT_GPRS: [RegisterID; 8] = [
    RegisterID::X0,
    RegisterID::X1,
    RegisterID::X2,
    RegisterID::X3,
    RegisterID::X4,
    RegisterID::X5,
    RegisterID::X6,
    RegisterID::X7,
];

/// Floating-point / SIMD register identifier.
///
/// C++ map: `ARM64Registers::FPRegisterID` (ARM64Assembler.h:192-198) generated
/// from `FOR_EACH_FP_REGISTER` (ARM64Registers.h:140-175). `q0..q31` take
/// discriminants `0..31`. The `D` (64-bit) and `S` (32-bit) views address the
/// same register file index; only the instruction datasize field differs, so a
/// single `Qn` identifier covers `Qn`/`Dn`/`Sn`. The `InvalidFPRReg = -1`
/// sentinel is omitted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum FPRegisterID {
    Q0 = 0,
    Q1 = 1,
    Q2 = 2,
    Q3 = 3,
    Q4 = 4,
    Q5 = 5,
    Q6 = 6,
    Q7 = 7,
    Q8 = 8,
    Q9 = 9,
    Q10 = 10,
    Q11 = 11,
    Q12 = 12,
    Q13 = 13,
    Q14 = 14,
    Q15 = 15,
    Q16 = 16,
    Q17 = 17,
    Q18 = 18,
    Q19 = 19,
    Q20 = 20,
    Q21 = 21,
    Q22 = 22,
    Q23 = 23,
    Q24 = 24,
    Q25 = 25,
    Q26 = 26,
    Q27 = 27,
    Q28 = 28,
    Q29 = 29,
    Q30 = 30,
    Q31 = 31,
}

impl FPRegisterID {
    /// Raw enum discriminant (the JSC `FPRegisterID` integer value).
    #[inline]
    pub const fn value(self) -> u32 {
        self as u32
    }

    /// Callee-saved FP register per the `cs` column of `FOR_EACH_FP_REGISTER`
    /// (ARM64Registers.h:151-158): `q8..q15` (low 64 bits only, per the source
    /// comment "Callee-saved (up to 64-bits only!)").
    #[inline]
    pub const fn is_callee_saved(self) -> bool {
        matches!(
            self,
            FPRegisterID::Q8
                | FPRegisterID::Q9
                | FPRegisterID::Q10
                | FPRegisterID::Q11
                | FPRegisterID::Q12
                | FPRegisterID::Q13
                | FPRegisterID::Q14
                | FPRegisterID::Q15
        )
    }
}

/// AAPCS64 floating-point parameter / result registers `q0..q7`
/// (ARM64Registers.h:141-149 "Parameter/result registers").
pub const FP_ARGUMENT_REGS: [FPRegisterID; 8] = [
    FPRegisterID::Q0,
    FPRegisterID::Q1,
    FPRegisterID::Q2,
    FPRegisterID::Q3,
    FPRegisterID::Q4,
    FPRegisterID::Q5,
    FPRegisterID::Q6,
    FPRegisterID::Q7,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpr_discriminants_match_jsc_register_ids() {
        // FOR_EACH_GP_REGISTER ordering (ARM64Registers.h:46-84): x0..x28, then
        // fp=29, lr=30, sp=31.
        assert_eq!(RegisterID::X0.value(), 0);
        assert_eq!(RegisterID::X28.value(), 28);
        assert_eq!(RegisterID::Fp.value(), 29);
        assert_eq!(RegisterID::Lr.value(), 30);
        assert_eq!(RegisterID::Sp.value(), 31);
        // zr alias is 0x3f (ARM64Registers.h:133); the encoder masks it to 31.
        assert_eq!(RegisterID::Zr.value(), 0x3f);
        assert_eq!(RegisterID::Zr.value() & 31, 31);
    }

    #[test]
    fn gpr_aliases_match_jsc() {
        assert_eq!(RegisterID::X29, RegisterID::Fp);
        assert_eq!(RegisterID::X30, RegisterID::Lr);
        assert_eq!(RegisterID::IP0, RegisterID::X16);
        assert_eq!(RegisterID::IP1, RegisterID::X17);
    }

    #[test]
    fn callee_saved_matches_aapcs64() {
        // x19..x28 + fp are callee-saved; lr and the argument regs are not.
        assert!(RegisterID::X19.is_callee_saved());
        assert!(RegisterID::X28.is_callee_saved());
        assert!(RegisterID::Fp.is_callee_saved());
        assert!(!RegisterID::Lr.is_callee_saved());
        assert!(!RegisterID::X0.is_callee_saved());
        assert!(!RegisterID::X18.is_callee_saved());
        for reg in ARGUMENT_GPRS {
            assert!(!reg.is_callee_saved());
        }
    }

    #[test]
    fn fpr_discriminants_and_callee_saved() {
        assert_eq!(FPRegisterID::Q0.value(), 0);
        assert_eq!(FPRegisterID::Q31.value(), 31);
        assert!(FPRegisterID::Q8.is_callee_saved());
        assert!(FPRegisterID::Q15.is_callee_saved());
        assert!(!FPRegisterID::Q7.is_callee_saved());
        assert!(!FPRegisterID::Q16.is_callee_saved());
    }
}
