//! ARM64 JSC CallFrame address-mode proof.
//!
//! C++ JSC map: `AssemblyHelpers::addressFor(VirtualRegister)` addresses
//! `GPRInfo::callFrameRegister` with `VirtualRegister::offsetInBytes()`.
//! On ARM64 this means `fp/x29` is the `CallFrame*`; locals are negative
//! displacements below `fp`, while header slots, `this`, and arguments are
//! non-negative displacements above it.
//!
//! The live Rust ARM64 return seed still uses a raw register-window carrier and
//! positive local offsets. This module is proof-only until public ARM64
//! admission moves to the JSC machine-stack CallFrame entry path.

use crate::bytecode::register::CallFrameSlotLayout;
use crate::bytecode::{RegisterClass, VirtualRegister};

use super::register_contract::{self, Arm64Gpr};

const JSC_REGISTER_BYTES: i32 = 8;
const ARM64_UNSCALED_SIGNED_IMMEDIATE_MIN: i32 = -256;
const ARM64_UNSCALED_SIGNED_IMMEDIATE_MAX: i32 = 255;
const ARM64_UNSIGNED_SCALED_64_MAX_BYTE_OFFSET: i32 = 0x0fff * JSC_REGISTER_BYTES;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64JscCallFrameSlotKind {
    Local { local_index: u32 },
    Header { raw_slot: u32 },
    ArgumentIncludingThis { argument_index: u32 },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64JscCallFrameAddressMode {
    SignedUnscaledImmediate9,
    UnsignedScaledImmediate12,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64JscCallFrameAddressingError {
    InvalidRegister {
        register: VirtualRegister,
    },
    ConstantRegisterUnsupported {
        register: VirtualRegister,
        constant_index: u32,
    },
    ByteOffsetOverflow {
        register: VirtualRegister,
    },
    OffsetNotEncodableAsSingleInstruction {
        register: VirtualRegister,
        byte_offset_from_call_frame: i32,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64JscCallFrameAddress {
    pub(crate) register: VirtualRegister,
    pub(crate) slot: Arm64JscCallFrameSlotKind,
    pub(crate) base: Arm64Gpr,
    pub(crate) byte_offset_from_call_frame: i32,
    pub(crate) mode: Arm64JscCallFrameAddressMode,
}

#[allow(dead_code)]
pub(crate) fn arm64_jsc_call_frame_address_for_virtual_register(
    register: VirtualRegister,
) -> Result<Arm64JscCallFrameAddress, Arm64JscCallFrameAddressingError> {
    let layout = CallFrameSlotLayout::JSC_RUST;
    let (slot, raw_offset_slots) = match register.classify(layout.this_argument_offset) {
        RegisterClass::Invalid => {
            return Err(Arm64JscCallFrameAddressingError::InvalidRegister { register });
        }
        RegisterClass::Local(local_index) => {
            let offset_slots = i32::try_from(local_index)
                .ok()
                .and_then(|index| index.checked_add(1))
                .and_then(|index| index.checked_neg())
                .ok_or(Arm64JscCallFrameAddressingError::ByteOffsetOverflow { register })?;
            (
                Arm64JscCallFrameSlotKind::Local { local_index },
                offset_slots,
            )
        }
        RegisterClass::CallFrameHeader(raw_slot) => {
            let offset_slots = i32::try_from(raw_slot)
                .map_err(|_| Arm64JscCallFrameAddressingError::ByteOffsetOverflow { register })?;
            (Arm64JscCallFrameSlotKind::Header { raw_slot }, offset_slots)
        }
        RegisterClass::ArgumentIncludingThis(argument_index) => {
            let offset_slots = layout
                .this_argument_offset
                .0
                .checked_add(i32::try_from(argument_index).map_err(|_| {
                    Arm64JscCallFrameAddressingError::ByteOffsetOverflow { register }
                })?)
                .ok_or(Arm64JscCallFrameAddressingError::ByteOffsetOverflow { register })?;
            (
                Arm64JscCallFrameSlotKind::ArgumentIncludingThis { argument_index },
                offset_slots,
            )
        }
        RegisterClass::Constant(constant_index) => {
            return Err(
                Arm64JscCallFrameAddressingError::ConstantRegisterUnsupported {
                    register,
                    constant_index,
                },
            );
        }
    };
    let byte_offset_from_call_frame = raw_offset_slots
        .checked_mul(JSC_REGISTER_BYTES)
        .ok_or(Arm64JscCallFrameAddressingError::ByteOffsetOverflow { register })?;
    let mode = arm64_single_instruction_address_mode(byte_offset_from_call_frame).ok_or(
        Arm64JscCallFrameAddressingError::OffsetNotEncodableAsSingleInstruction {
            register,
            byte_offset_from_call_frame,
        },
    )?;

    Ok(Arm64JscCallFrameAddress {
        register,
        slot,
        base: register_contract::CALL_FRAME_REGISTER,
        byte_offset_from_call_frame,
        mode,
    })
}

#[allow(dead_code)]
pub(crate) fn arm64_jsc_call_frame_load64_word(
    address: Arm64JscCallFrameAddress,
    destination: Arm64Gpr,
) -> u32 {
    match address.mode {
        Arm64JscCallFrameAddressMode::SignedUnscaledImmediate9 => {
            encode_arm64_load_store_unscaled_64(
                0xf840_0000,
                destination,
                address.base,
                address.byte_offset_from_call_frame,
            )
        }
        Arm64JscCallFrameAddressMode::UnsignedScaledImmediate12 => {
            encode_arm64_load_store_unsigned_scaled_64(
                0xf940_0000,
                destination,
                address.base,
                address.byte_offset_from_call_frame,
            )
        }
    }
}

#[allow(dead_code)]
pub(crate) fn arm64_jsc_call_frame_store64_word(
    address: Arm64JscCallFrameAddress,
    source: Arm64Gpr,
) -> u32 {
    match address.mode {
        Arm64JscCallFrameAddressMode::SignedUnscaledImmediate9 => {
            encode_arm64_load_store_unscaled_64(
                0xf800_0000,
                source,
                address.base,
                address.byte_offset_from_call_frame,
            )
        }
        Arm64JscCallFrameAddressMode::UnsignedScaledImmediate12 => {
            encode_arm64_load_store_unsigned_scaled_64(
                0xf900_0000,
                source,
                address.base,
                address.byte_offset_from_call_frame,
            )
        }
    }
}

const fn arm64_single_instruction_address_mode(
    byte_offset: i32,
) -> Option<Arm64JscCallFrameAddressMode> {
    if byte_offset >= ARM64_UNSCALED_SIGNED_IMMEDIATE_MIN
        && byte_offset <= ARM64_UNSCALED_SIGNED_IMMEDIATE_MAX
    {
        Some(Arm64JscCallFrameAddressMode::SignedUnscaledImmediate9)
    } else if byte_offset >= 0
        && byte_offset % JSC_REGISTER_BYTES == 0
        && byte_offset <= ARM64_UNSIGNED_SCALED_64_MAX_BYTE_OFFSET
    {
        Some(Arm64JscCallFrameAddressMode::UnsignedScaledImmediate12)
    } else {
        None
    }
}

fn encode_arm64_load_store_unscaled_64(
    opcode: u32,
    rt: Arm64Gpr,
    rn: Arm64Gpr,
    byte_offset: i32,
) -> u32 {
    debug_assert!(byte_offset >= ARM64_UNSCALED_SIGNED_IMMEDIATE_MIN);
    debug_assert!(byte_offset <= ARM64_UNSCALED_SIGNED_IMMEDIATE_MAX);
    let imm9 = (byte_offset as u32) & 0x01ff;
    opcode | (imm9 << 12) | (u32::from(rn.index) << 5) | u32::from(rt.index)
}

fn encode_arm64_load_store_unsigned_scaled_64(
    opcode: u32,
    rt: Arm64Gpr,
    rn: Arm64Gpr,
    byte_offset: i32,
) -> u32 {
    debug_assert!(byte_offset >= 0);
    debug_assert!(byte_offset % JSC_REGISTER_BYTES == 0);
    debug_assert!(byte_offset <= ARM64_UNSIGNED_SCALED_64_MAX_BYTE_OFFSET);
    let imm12 = (byte_offset / JSC_REGISTER_BYTES) as u32;
    opcode | (imm12 << 10) | (u32::from(rn.index) << 5) | u32::from(rt.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm64_jsc_call_frame_address_maps_locals_below_fp() {
        let address = arm64_jsc_call_frame_address_for_virtual_register(VirtualRegister::local(0))
            .expect("local0 JSC CallFrame address");

        assert_eq!(
            address,
            Arm64JscCallFrameAddress {
                register: VirtualRegister::local(0),
                slot: Arm64JscCallFrameSlotKind::Local { local_index: 0 },
                base: register_contract::CALL_FRAME_REGISTER,
                byte_offset_from_call_frame: -8,
                mode: Arm64JscCallFrameAddressMode::SignedUnscaledImmediate9,
            }
        );
        assert_eq!(
            arm64_jsc_call_frame_load64_word(address, register_contract::X9),
            0xf85f_83a9
        );
        assert_eq!(
            arm64_jsc_call_frame_store64_word(address, register_contract::X10),
            0xf81f_83aa
        );
    }

    #[test]
    fn arm64_jsc_call_frame_address_maps_header_this_and_arguments_above_fp() {
        let code_block = arm64_jsc_call_frame_address_for_virtual_register(
            VirtualRegister::argument_or_header(2),
        )
        .expect("CodeBlock slot address");
        assert_eq!(
            code_block.slot,
            Arm64JscCallFrameSlotKind::Header { raw_slot: 2 }
        );
        assert_eq!(code_block.byte_offset_from_call_frame, 16);
        assert_eq!(
            code_block.mode,
            Arm64JscCallFrameAddressMode::SignedUnscaledImmediate9
        );
        assert_eq!(
            arm64_jsc_call_frame_load64_word(code_block, register_contract::X26),
            0xf841_03ba
        );

        let this_value = arm64_jsc_call_frame_address_for_virtual_register(
            VirtualRegister::argument_including_this(
                0,
                CallFrameSlotLayout::JSC_RUST.this_argument_offset,
            ),
        )
        .expect("this argument address");
        assert_eq!(
            this_value.slot,
            Arm64JscCallFrameSlotKind::ArgumentIncludingThis { argument_index: 0 }
        );
        assert_eq!(this_value.byte_offset_from_call_frame, 40);

        let first_argument = arm64_jsc_call_frame_address_for_virtual_register(
            VirtualRegister::argument_including_this(
                1,
                CallFrameSlotLayout::JSC_RUST.this_argument_offset,
            ),
        )
        .expect("first argument address");
        assert_eq!(first_argument.byte_offset_from_call_frame, 48);
    }

    #[test]
    fn arm64_jsc_call_frame_address_uses_unsigned_scaled_for_large_positive_slots() {
        let address = arm64_jsc_call_frame_address_for_virtual_register(
            VirtualRegister::argument_including_this(
                64,
                CallFrameSlotLayout::JSC_RUST.this_argument_offset,
            ),
        )
        .expect("large argument address");

        assert_eq!(address.byte_offset_from_call_frame, 552);
        assert_eq!(
            address.mode,
            Arm64JscCallFrameAddressMode::UnsignedScaledImmediate12
        );
        assert_eq!(
            arm64_jsc_call_frame_load64_word(address, register_contract::X0),
            0xf941_17a0
        );
    }

    #[test]
    fn arm64_jsc_call_frame_address_rejects_invalid_constants_and_large_negative_locals() {
        assert_eq!(
            arm64_jsc_call_frame_address_for_virtual_register(VirtualRegister::INVALID),
            Err(Arm64JscCallFrameAddressingError::InvalidRegister {
                register: VirtualRegister::INVALID,
            })
        );
        assert_eq!(
            arm64_jsc_call_frame_address_for_virtual_register(VirtualRegister::constant(3)),
            Err(
                Arm64JscCallFrameAddressingError::ConstantRegisterUnsupported {
                    register: VirtualRegister::constant(3),
                    constant_index: 3,
                }
            )
        );

        let local32 = VirtualRegister::local(32);
        assert_eq!(
            arm64_jsc_call_frame_address_for_virtual_register(local32),
            Err(
                Arm64JscCallFrameAddressingError::OffsetNotEncodableAsSingleInstruction {
                    register: local32,
                    byte_offset_from_call_frame: -264,
                }
            )
        );
    }
}
