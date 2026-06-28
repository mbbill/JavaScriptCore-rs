//! Reserved ABI boundary for future LLInt, generated-code, and host bridges.
//!
//! This module names the metadata that code blocks, ICs, and Wasm bridges will
//! need before any machine-code generator exists. It deliberately stores
//! symbolic locations and calling conventions instead of function pointers or
//! executable memory.

#![allow(dead_code)]

use crate::jit::JitCodeId;
use crate::runtime::{CodeBlockId, NativeCodeId};

/// Abstract execution entry kind attached to code-block-equivalent state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntrypointKind {
    /// No executable entry has been attached.
    None,
    /// Interpreter or LLInt-compatible thunk reserved before JIT exists.
    InterpreterThunk,
    /// Future generated machine-code entry.
    GeneratedCode,
    /// Host/native callback bridge entry.
    HostBridge,
    /// Future Wasm bridge or thunk entry.
    WasmBridge,
}

/// ABI family for a reserved entrypoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryAbi {
    /// Rust-owned call boundary with no layout compatibility promise.
    Rust,
    /// Reserved compatibility boundary for LLInt/JIT-visible frames.
    LlIntCompatible,
    /// Future generated-code ABI with register and stack conventions.
    GeneratedCode,
    /// ABI is intentionally not selected by the skeleton.
    Deferred,
    /// Reserved JS-to-Wasm or Wasm-to-JS ABI.
    Wasm,
}

/// Opaque entrypoint descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entrypoint {
    pub kind: EntrypointKind,
    pub abi: EntryAbi,
    pub code: Option<JitCodeId>,
    pub boundary: Option<CallBoundaryId>,
}

/// Stable identity for a call boundary metadata record.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CallBoundaryId(pub u64);

/// Symbolic register families used by ABI metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterRole {
    Argument,
    Return,
    CalleeSave,
    CallerSave,
    Scratch,
    PinnedVm,
    PinnedCallFrame,
    PinnedWasmContext,
}

/// Register descriptor without naming a physical architecture register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterBinding {
    pub role: RegisterRole,
    pub index: u8,
    pub value: AbiValue,
}

/// Value category carried across a reserved ABI edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiValue {
    JsValue,
    Cell,
    Int32,
    Int64,
    Float32,
    Float64,
    Pointer,
    WasmExternRef,
    WasmFuncRef,
    Void,
}

/// Stack slot role in a future frame layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameSlotRole {
    ReturnAddress,
    CallerFrame,
    CalleeSaves,
    Arguments,
    Locals,
    Spill,
    ExceptionHandler,
    WasmScratch,
}

/// Symbolic frame slot. Offsets remain optional until layout code exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameSlot {
    pub role: FrameSlotRole,
    pub index: u32,
    pub byte_offset: Option<i32>,
}

/// Metadata for a single executable or bridge boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallBoundaryMetadata {
    pub id: CallBoundaryId,
    pub owner: Option<CodeBlockId>,
    pub abi: EntryAbi,
    pub entry_kind: EntrypointKind,
    pub native_symbol: Option<NativeCodeId>,
    pub arguments: Vec<AbiValue>,
    pub returns: Vec<AbiValue>,
    pub registers: Vec<RegisterBinding>,
    pub frame_slots: Vec<FrameSlot>,
    pub requires_vm_entry_scope: bool,
    pub may_call_js: bool,
    pub may_throw: bool,
}

/// Patchable location category reserved for generated code and thunks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchpointKind {
    Entrypoint,
    SlowPathCall,
    InlineCacheData,
    DirectCallTarget,
    WasmEntrypointLoad,
    ExceptionHandler,
}

/// Symbolic patchpoint owned by generated code metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchpointDescriptor {
    pub kind: PatchpointKind,
    pub owner_code: Option<JitCodeId>,
    pub byte_offset: Option<u32>,
    pub boundary: Option<CallBoundaryId>,
}

/// Header slots shared by the interpreter-compatible baseline frame.
///
/// The faithful JSC call-frame header is exactly five slots — callerFrame@0,
/// returnPC@1, codeBlock@2, callee@3, argumentCountIncludingThis@4
/// (`interpreter/CallFrame.h:176-191`, `headerSizeInRegisters` == 5). Slot 3 is
/// `CallFrameSlot::callee` (a `CalleeBits`), NOT a callee-save area: callee-saves
/// are spilled BELOW the locals, not in the call-frame header. The remaining
/// roles below (`ThisValue` and the baseline metadata carriers) are a Rust-only
/// baseline apparatus, not JSC call-frame header slots (in JSC slot 5 is
/// `thisArgument` and slots 6+ are arguments); see
/// `BASELINE_FRAME_HEADER_SLOTS`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineFrameHeaderSlotRole {
    CallerFrame,
    ReturnAddress,
    CodeBlock,
    // C++ JSC `CallFrameSlot::callee` (CallFrame.h:178): the boxed callee
    // (`CalleeBits`). Previously modeled (incorrectly) as `CalleeSaveArea`, which
    // omitted the callee slot entirely; corrected to `Callee` to match the
    // faithful 5-slot header.
    Callee,
    ArgumentCount,
    ThisValue,
    BytecodeIndex,
    Checkpoint,
    ExceptionHandler,
}

impl BaselineFrameHeaderSlotRole {
    pub fn expected_value(self) -> AbiValue {
        match self {
            BaselineFrameHeaderSlotRole::CallerFrame
            | BaselineFrameHeaderSlotRole::ReturnAddress
            | BaselineFrameHeaderSlotRole::CodeBlock
            // `callee` holds a boxed `CalleeBits` pointer (CallFrame.h:202).
            | BaselineFrameHeaderSlotRole::Callee
            | BaselineFrameHeaderSlotRole::ExceptionHandler => AbiValue::Pointer,
            BaselineFrameHeaderSlotRole::ArgumentCount
            | BaselineFrameHeaderSlotRole::BytecodeIndex
            | BaselineFrameHeaderSlotRole::Checkpoint => AbiValue::Int32,
            BaselineFrameHeaderSlotRole::ThisValue => AbiValue::JsValue,
        }
    }
}

/// Symbolic baseline frame header slot. Slot indexes are stable ABI ordinals,
/// not architecture byte offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineFrameHeaderSlot {
    pub role: BaselineFrameHeaderSlotRole,
    pub slot_index: u8,
    pub value: AbiValue,
}

/// Location used to carry bytecode-index and checkpoint metadata across calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineCarrierLocation {
    FrameHeaderSlot(BaselineFrameHeaderSlotRole),
    PinnedRegister(RegisterRole),
    RuntimeCallArgument(u8),
}

/// Required metadata carrier for resuming or validating a baseline frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineBytecodeCheckpointCarrier {
    pub bytecode_index: BaselineCarrierLocation,
    pub checkpoint: BaselineCarrierLocation,
}

/// Stack alignment promised by the baseline frame ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineStackAlignment {
    pub minimum_bytes: u16,
    pub applies_at_entry: bool,
    pub applies_at_runtime_calls: bool,
}

/// Carrier for a normal baseline return value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineReturnCarrier {
    Register(RegisterRole),
    FrameHeaderSlot(BaselineFrameHeaderSlotRole),
}

/// Normal return convention from baseline code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineReturnConvention {
    pub value: AbiValue,
    pub carrier: BaselineReturnCarrier,
}

/// Carrier for a thrown exception value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineThrowExceptionCarrier {
    VmExceptionState,
    Register(RegisterRole),
    FrameHeaderSlot(BaselineFrameHeaderSlotRole),
}

/// Target used when baseline code exits through an exception path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineThrowTarget {
    FrameHeaderSlot(BaselineFrameHeaderSlotRole),
    UnwindToCaller,
}

/// Throw convention from baseline code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineThrowConvention {
    pub exception: AbiValue,
    pub exception_carrier: BaselineThrowExceptionCarrier,
    pub target: BaselineThrowTarget,
}

/// Normal-return and throw convention required before baseline entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineReturnThrowConvention {
    pub normal_return: BaselineReturnConvention,
    pub throw: BaselineThrowConvention,
}

/// Runtime call clobber policy visible to baseline code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineRuntimeCallClobbers {
    pub clobbered_roles: &'static [RegisterRole],
    pub preserved_roles: &'static [RegisterRole],
    pub clobbers_condition_flags: bool,
    pub clobbers_stack_argument_area: bool,
    pub may_allocate: bool,
    pub may_throw: bool,
}

impl BaselineRuntimeCallClobbers {
    pub fn clobbers_role(&self, role: RegisterRole) -> bool {
        self.clobbered_roles.contains(&role)
    }

    pub fn preserves_role(&self, role: RegisterRole) -> bool {
        self.preserved_roles.contains(&role)
    }
}

/// Data-only ABI contract for the first baseline tier. This does not describe
/// native entry code; it records the metadata a baseline artifact must satisfy
/// before an entrypoint can be installed later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineAbiDescriptor {
    pub name: &'static str,
    pub entry_kind: EntrypointKind,
    pub entry_abi: EntryAbi,
    pub frame_abi: EntryAbi,
    pub pinned_registers: &'static [RegisterBinding],
    pub frame_header_slots: &'static [BaselineFrameHeaderSlot],
    pub bytecode_checkpoint_carrier: Option<BaselineBytecodeCheckpointCarrier>,
    pub stack_alignment: BaselineStackAlignment,
    pub return_throw: Option<BaselineReturnThrowConvention>,
    pub runtime_call_clobbers: BaselineRuntimeCallClobbers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineAbiValidationError {
    UnexpectedEntryKind(EntrypointKind),
    UnexpectedEntryAbi(EntryAbi),
    UnexpectedFrameAbi(EntryAbi),
    MissingPinnedVmRegister,
    MissingPinnedCallFrameBaseRegister,
    MissingFrameHeaderSlot(BaselineFrameHeaderSlotRole),
    DuplicateFrameHeaderSlotRole(BaselineFrameHeaderSlotRole),
    DuplicateFrameHeaderSlotIndex(u8),
    UnexpectedFrameHeaderSlotOrdinal {
        role: BaselineFrameHeaderSlotRole,
        expected: u8,
        actual: u8,
    },
    UnexpectedFrameHeaderSlotValue {
        role: BaselineFrameHeaderSlotRole,
        expected: AbiValue,
        actual: AbiValue,
    },
    MissingBytecodeIndexCheckpointCarrier,
    CarrierReferencesMissingFrameHeaderSlot(BaselineFrameHeaderSlotRole),
    CarrierReferencesMissingPinnedRegister(RegisterRole),
    InvalidStackAlignment,
    MissingReturnThrowConvention,
    InvalidReturnValue(AbiValue),
    InvalidThrowValue(AbiValue),
    MissingRuntimeCallClobber(RegisterRole),
    RuntimeCallDoesNotPreservePinnedRegister(RegisterRole),
}

impl BaselineAbiDescriptor {
    pub fn validate(&self) -> Result<(), BaselineAbiValidationError> {
        if self.entry_kind != EntrypointKind::GeneratedCode {
            return Err(BaselineAbiValidationError::UnexpectedEntryKind(
                self.entry_kind,
            ));
        }
        if self.entry_abi != EntryAbi::GeneratedCode {
            return Err(BaselineAbiValidationError::UnexpectedEntryAbi(
                self.entry_abi,
            ));
        }
        if self.frame_abi != EntryAbi::LlIntCompatible {
            return Err(BaselineAbiValidationError::UnexpectedFrameAbi(
                self.frame_abi,
            ));
        }
        if !self.has_pinned_register(RegisterRole::PinnedVm) {
            return Err(BaselineAbiValidationError::MissingPinnedVmRegister);
        }
        if !self.has_pinned_register(RegisterRole::PinnedCallFrame) {
            return Err(BaselineAbiValidationError::MissingPinnedCallFrameBaseRegister);
        }

        self.validate_frame_header_slots()?;

        let carrier = self
            .bytecode_checkpoint_carrier
            .ok_or(BaselineAbiValidationError::MissingBytecodeIndexCheckpointCarrier)?;
        self.validate_carrier_location(carrier.bytecode_index)?;
        self.validate_carrier_location(carrier.checkpoint)?;

        self.validate_stack_alignment()?;
        self.validate_return_throw_convention()?;
        self.validate_runtime_call_clobbers()?;

        Ok(())
    }

    pub fn has_pinned_register(&self, role: RegisterRole) -> bool {
        self.pinned_registers
            .iter()
            .any(|binding| binding.role == role && binding.value == AbiValue::Pointer)
    }

    pub fn frame_header_slot(
        &self,
        role: BaselineFrameHeaderSlotRole,
    ) -> Option<&BaselineFrameHeaderSlot> {
        self.frame_header_slots
            .iter()
            .find(|slot| slot.role == role)
    }

    fn validate_frame_header_slots(&self) -> Result<(), BaselineAbiValidationError> {
        for (slot_position, slot) in self.frame_header_slots.iter().enumerate() {
            for prior_slot in &self.frame_header_slots[..slot_position] {
                if prior_slot.role == slot.role {
                    return Err(BaselineAbiValidationError::DuplicateFrameHeaderSlotRole(
                        slot.role,
                    ));
                }
                if prior_slot.slot_index == slot.slot_index {
                    return Err(BaselineAbiValidationError::DuplicateFrameHeaderSlotIndex(
                        slot.slot_index,
                    ));
                }
            }
        }

        for (expected_ordinal, role) in BASELINE_REQUIRED_FRAME_HEADER_SLOTS.iter().enumerate() {
            let expected_ordinal =
                u8::try_from(expected_ordinal).expect("baseline header ordinal fits in u8");
            let stable_slot = BASELINE_FRAME_HEADER_SLOTS
                .iter()
                .find(|slot| slot.role == *role)
                .ok_or(BaselineAbiValidationError::MissingFrameHeaderSlot(*role))?;
            if stable_slot.slot_index != expected_ordinal {
                return Err(
                    BaselineAbiValidationError::UnexpectedFrameHeaderSlotOrdinal {
                        role: *role,
                        expected: expected_ordinal,
                        actual: stable_slot.slot_index,
                    },
                );
            }

            let slot = self
                .frame_header_slot(*role)
                .ok_or(BaselineAbiValidationError::MissingFrameHeaderSlot(*role))?;
            if slot.slot_index != stable_slot.slot_index {
                return Err(
                    BaselineAbiValidationError::UnexpectedFrameHeaderSlotOrdinal {
                        role: *role,
                        expected: stable_slot.slot_index,
                        actual: slot.slot_index,
                    },
                );
            }
            let expected = role.expected_value();
            if slot.value != expected {
                return Err(BaselineAbiValidationError::UnexpectedFrameHeaderSlotValue {
                    role: *role,
                    expected,
                    actual: slot.value,
                });
            }
        }

        Ok(())
    }

    fn validate_carrier_location(
        &self,
        location: BaselineCarrierLocation,
    ) -> Result<(), BaselineAbiValidationError> {
        match location {
            BaselineCarrierLocation::FrameHeaderSlot(role) => {
                if self.frame_header_slot(role).is_some() {
                    Ok(())
                } else {
                    Err(BaselineAbiValidationError::CarrierReferencesMissingFrameHeaderSlot(role))
                }
            }
            BaselineCarrierLocation::PinnedRegister(role) => {
                if self.has_pinned_register(role) {
                    Ok(())
                } else {
                    Err(BaselineAbiValidationError::CarrierReferencesMissingPinnedRegister(role))
                }
            }
            BaselineCarrierLocation::RuntimeCallArgument(_) => Ok(()),
        }
    }

    fn validate_stack_alignment(&self) -> Result<(), BaselineAbiValidationError> {
        if self.stack_alignment.minimum_bytes < BASELINE_MINIMUM_STACK_ALIGNMENT_BYTES
            || !self.stack_alignment.minimum_bytes.is_power_of_two()
            || !self.stack_alignment.applies_at_entry
            || !self.stack_alignment.applies_at_runtime_calls
        {
            return Err(BaselineAbiValidationError::InvalidStackAlignment);
        }
        Ok(())
    }

    fn validate_return_throw_convention(&self) -> Result<(), BaselineAbiValidationError> {
        let convention = self
            .return_throw
            .ok_or(BaselineAbiValidationError::MissingReturnThrowConvention)?;
        if convention.normal_return.value != AbiValue::JsValue {
            return Err(BaselineAbiValidationError::InvalidReturnValue(
                convention.normal_return.value,
            ));
        }
        self.validate_return_carrier(convention.normal_return.carrier)?;

        if convention.throw.exception != AbiValue::JsValue {
            return Err(BaselineAbiValidationError::InvalidThrowValue(
                convention.throw.exception,
            ));
        }
        self.validate_throw_exception_carrier(convention.throw.exception_carrier)?;
        self.validate_throw_target(convention.throw.target)
    }

    fn validate_return_carrier(
        &self,
        carrier: BaselineReturnCarrier,
    ) -> Result<(), BaselineAbiValidationError> {
        match carrier {
            BaselineReturnCarrier::Register(_) => Ok(()),
            BaselineReturnCarrier::FrameHeaderSlot(role) => {
                self.validate_frame_header_reference(role)
            }
        }
    }

    fn validate_throw_exception_carrier(
        &self,
        carrier: BaselineThrowExceptionCarrier,
    ) -> Result<(), BaselineAbiValidationError> {
        match carrier {
            BaselineThrowExceptionCarrier::VmExceptionState
            | BaselineThrowExceptionCarrier::Register(_) => Ok(()),
            BaselineThrowExceptionCarrier::FrameHeaderSlot(role) => {
                self.validate_frame_header_reference(role)
            }
        }
    }

    fn validate_throw_target(
        &self,
        target: BaselineThrowTarget,
    ) -> Result<(), BaselineAbiValidationError> {
        match target {
            BaselineThrowTarget::FrameHeaderSlot(role) => {
                self.validate_frame_header_reference(role)
            }
            BaselineThrowTarget::UnwindToCaller => Ok(()),
        }
    }

    fn validate_frame_header_reference(
        &self,
        role: BaselineFrameHeaderSlotRole,
    ) -> Result<(), BaselineAbiValidationError> {
        if self.frame_header_slot(role).is_some() {
            Ok(())
        } else {
            Err(BaselineAbiValidationError::CarrierReferencesMissingFrameHeaderSlot(role))
        }
    }

    fn validate_runtime_call_clobbers(&self) -> Result<(), BaselineAbiValidationError> {
        for role in BASELINE_RUNTIME_CALL_REQUIRED_CLOBBERS {
            if !self.runtime_call_clobbers.clobbers_role(*role) {
                return Err(BaselineAbiValidationError::MissingRuntimeCallClobber(*role));
            }
        }
        for role in BASELINE_RUNTIME_CALL_REQUIRED_PRESERVES {
            if !self.runtime_call_clobbers.preserves_role(*role) {
                return Err(
                    BaselineAbiValidationError::RuntimeCallDoesNotPreservePinnedRegister(*role),
                );
            }
        }
        Ok(())
    }
}

pub const BASELINE_MINIMUM_STACK_ALIGNMENT_BYTES: u16 = 16;

pub const BASELINE_PINNED_VM_REGISTER: RegisterBinding = RegisterBinding {
    role: RegisterRole::PinnedVm,
    index: 0,
    value: AbiValue::Pointer,
};

pub const BASELINE_PINNED_CALL_FRAME_BASE_REGISTER: RegisterBinding = RegisterBinding {
    role: RegisterRole::PinnedCallFrame,
    index: 0,
    value: AbiValue::Pointer,
};

pub const BASELINE_PINNED_REGISTERS: &[RegisterBinding] = &[
    BASELINE_PINNED_VM_REGISTER,
    BASELINE_PINNED_CALL_FRAME_BASE_REGISTER,
];

pub const BASELINE_REQUIRED_FRAME_HEADER_SLOTS: &[BaselineFrameHeaderSlotRole] = &[
    BaselineFrameHeaderSlotRole::CallerFrame,
    BaselineFrameHeaderSlotRole::ReturnAddress,
    BaselineFrameHeaderSlotRole::CodeBlock,
    // CallFrameSlot::callee @ slot 3 (CallFrame.h:178), not a callee-save area.
    BaselineFrameHeaderSlotRole::Callee,
    BaselineFrameHeaderSlotRole::ArgumentCount,
    BaselineFrameHeaderSlotRole::ThisValue,
    BaselineFrameHeaderSlotRole::BytecodeIndex,
    BaselineFrameHeaderSlotRole::Checkpoint,
    BaselineFrameHeaderSlotRole::ExceptionHandler,
];

pub const BASELINE_FRAME_HEADER_SLOTS: &[BaselineFrameHeaderSlot] = &[
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::CallerFrame,
        slot_index: 0,
        value: AbiValue::Pointer,
    },
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::ReturnAddress,
        slot_index: 1,
        value: AbiValue::Pointer,
    },
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::CodeBlock,
        slot_index: 2,
        value: AbiValue::Pointer,
    },
    // C++ JSC `CallFrameSlot::callee` (CallFrame.h:178): the boxed callee at slot
    // 3. The previous `CalleeSaveArea` here was a defect — it omitted the callee
    // slot, and callee-saves are spilled below the locals, not in the header.
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::Callee,
        slot_index: 3,
        value: AbiValue::Pointer,
    },
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::ArgumentCount,
        slot_index: 4,
        value: AbiValue::Int32,
    },
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::ThisValue,
        slot_index: 5,
        value: AbiValue::JsValue,
    },
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::BytecodeIndex,
        slot_index: 6,
        value: AbiValue::Int32,
    },
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::Checkpoint,
        slot_index: 7,
        value: AbiValue::Int32,
    },
    BaselineFrameHeaderSlot {
        role: BaselineFrameHeaderSlotRole::ExceptionHandler,
        slot_index: 8,
        value: AbiValue::Pointer,
    },
];

pub const BASELINE_BYTECODE_CHECKPOINT_CARRIER: BaselineBytecodeCheckpointCarrier =
    BaselineBytecodeCheckpointCarrier {
        bytecode_index: BaselineCarrierLocation::FrameHeaderSlot(
            BaselineFrameHeaderSlotRole::BytecodeIndex,
        ),
        checkpoint: BaselineCarrierLocation::FrameHeaderSlot(
            BaselineFrameHeaderSlotRole::Checkpoint,
        ),
    };

pub const BASELINE_STACK_ALIGNMENT: BaselineStackAlignment = BaselineStackAlignment {
    minimum_bytes: BASELINE_MINIMUM_STACK_ALIGNMENT_BYTES,
    applies_at_entry: true,
    applies_at_runtime_calls: true,
};

pub const BASELINE_RETURN_THROW_CONVENTION: BaselineReturnThrowConvention =
    BaselineReturnThrowConvention {
        normal_return: BaselineReturnConvention {
            value: AbiValue::JsValue,
            carrier: BaselineReturnCarrier::Register(RegisterRole::Return),
        },
        throw: BaselineThrowConvention {
            exception: AbiValue::JsValue,
            exception_carrier: BaselineThrowExceptionCarrier::VmExceptionState,
            target: BaselineThrowTarget::FrameHeaderSlot(
                BaselineFrameHeaderSlotRole::ExceptionHandler,
            ),
        },
    };

pub const BASELINE_RUNTIME_CALL_REQUIRED_CLOBBERS: &[RegisterRole] = &[
    RegisterRole::Argument,
    RegisterRole::Return,
    RegisterRole::CallerSave,
    RegisterRole::Scratch,
];

pub const BASELINE_RUNTIME_CALL_REQUIRED_PRESERVES: &[RegisterRole] =
    &[RegisterRole::PinnedVm, RegisterRole::PinnedCallFrame];

pub const BASELINE_RUNTIME_CALL_CLOBBERS: BaselineRuntimeCallClobbers =
    BaselineRuntimeCallClobbers {
        clobbered_roles: BASELINE_RUNTIME_CALL_REQUIRED_CLOBBERS,
        preserved_roles: &[
            RegisterRole::PinnedVm,
            RegisterRole::PinnedCallFrame,
            RegisterRole::CalleeSave,
        ],
        clobbers_condition_flags: true,
        clobbers_stack_argument_area: true,
        may_allocate: true,
        may_throw: true,
    };

pub const BASELINE_ABI_DESCRIPTOR: BaselineAbiDescriptor = BaselineAbiDescriptor {
    name: "baseline-first-tier",
    entry_kind: EntrypointKind::GeneratedCode,
    entry_abi: EntryAbi::GeneratedCode,
    frame_abi: EntryAbi::LlIntCompatible,
    pinned_registers: BASELINE_PINNED_REGISTERS,
    frame_header_slots: BASELINE_FRAME_HEADER_SLOTS,
    bytecode_checkpoint_carrier: Some(BASELINE_BYTECODE_CHECKPOINT_CARRIER),
    stack_alignment: BASELINE_STACK_ALIGNMENT,
    return_throw: Some(BASELINE_RETURN_THROW_CONVENTION),
    runtime_call_clobbers: BASELINE_RUNTIME_CALL_CLOBBERS,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor_with_frame_header_slots(
        slots: Vec<BaselineFrameHeaderSlot>,
    ) -> BaselineAbiDescriptor {
        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.frame_header_slots = Box::leak(slots.into_boxed_slice());
        descriptor
    }

    #[test]
    fn baseline_abi_descriptor_validates_pinned_registers_and_header_slots() {
        assert_eq!(BASELINE_ABI_DESCRIPTOR.validate(), Ok(()));
        assert!(BASELINE_ABI_DESCRIPTOR.has_pinned_register(RegisterRole::PinnedVm));
        assert!(BASELINE_ABI_DESCRIPTOR.has_pinned_register(RegisterRole::PinnedCallFrame));

        for (expected_slot_index, role) in BASELINE_REQUIRED_FRAME_HEADER_SLOTS.iter().enumerate() {
            let slot = BASELINE_ABI_DESCRIPTOR
                .frame_header_slot(*role)
                .expect("required baseline frame header slot");
            assert_eq!(slot.slot_index, expected_slot_index as u8);
            assert_eq!(slot.value, role.expected_value());
        }

        const ONLY_VM_REGISTER: &[RegisterBinding] = &[BASELINE_PINNED_VM_REGISTER];
        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.pinned_registers = ONLY_VM_REGISTER;
        assert_eq!(
            descriptor.validate(),
            Err(BaselineAbiValidationError::MissingPinnedCallFrameBaseRegister)
        );

        const MISSING_CALLER_FRAME_HEADER: &[BaselineFrameHeaderSlot] = &[
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::ReturnAddress,
                slot_index: 1,
                value: AbiValue::Pointer,
            },
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::CodeBlock,
                slot_index: 2,
                value: AbiValue::Pointer,
            },
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::Callee,
                slot_index: 3,
                value: AbiValue::Pointer,
            },
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::ArgumentCount,
                slot_index: 4,
                value: AbiValue::Int32,
            },
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::ThisValue,
                slot_index: 5,
                value: AbiValue::JsValue,
            },
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::BytecodeIndex,
                slot_index: 6,
                value: AbiValue::Int32,
            },
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::Checkpoint,
                slot_index: 7,
                value: AbiValue::Int32,
            },
            BaselineFrameHeaderSlot {
                role: BaselineFrameHeaderSlotRole::ExceptionHandler,
                slot_index: 8,
                value: AbiValue::Pointer,
            },
        ];
        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.frame_header_slots = MISSING_CALLER_FRAME_HEADER;
        assert_eq!(
            descriptor.validate(),
            Err(BaselineAbiValidationError::MissingFrameHeaderSlot(
                BaselineFrameHeaderSlotRole::CallerFrame
            ))
        );
    }

    #[test]
    fn baseline_abi_rejects_duplicate_header_roles_and_ordinals() {
        let mut duplicate_role = BASELINE_FRAME_HEADER_SLOTS.to_vec();
        duplicate_role[1].role = BaselineFrameHeaderSlotRole::CallerFrame;
        assert_eq!(
            descriptor_with_frame_header_slots(duplicate_role).validate(),
            Err(BaselineAbiValidationError::DuplicateFrameHeaderSlotRole(
                BaselineFrameHeaderSlotRole::CallerFrame
            ))
        );

        let mut duplicate_slot_index = BASELINE_FRAME_HEADER_SLOTS.to_vec();
        duplicate_slot_index[1].slot_index = 0;
        assert_eq!(
            descriptor_with_frame_header_slots(duplicate_slot_index).validate(),
            Err(BaselineAbiValidationError::DuplicateFrameHeaderSlotIndex(0))
        );
    }

    #[test]
    fn baseline_abi_rejects_wrong_header_role_ordinal() {
        let mut wrong_caller_frame_ordinal = BASELINE_FRAME_HEADER_SLOTS.to_vec();
        wrong_caller_frame_ordinal[0].slot_index = 9;

        assert_eq!(
            descriptor_with_frame_header_slots(wrong_caller_frame_ordinal).validate(),
            Err(
                BaselineAbiValidationError::UnexpectedFrameHeaderSlotOrdinal {
                    role: BaselineFrameHeaderSlotRole::CallerFrame,
                    expected: 0,
                    actual: 9,
                }
            )
        );
    }

    #[test]
    fn baseline_abi_rejects_missing_bytecode_index_checkpoint_carrier() {
        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.bytecode_checkpoint_carrier = None;

        assert_eq!(
            descriptor.validate(),
            Err(BaselineAbiValidationError::MissingBytecodeIndexCheckpointCarrier)
        );
    }

    #[test]
    fn baseline_abi_rejects_missing_return_throw_convention() {
        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.return_throw = None;

        assert_eq!(
            descriptor.validate(),
            Err(BaselineAbiValidationError::MissingReturnThrowConvention)
        );
    }

    #[test]
    fn baseline_abi_describes_runtime_call_clobbers() {
        let clobbers = BASELINE_ABI_DESCRIPTOR.runtime_call_clobbers;

        assert!(clobbers.clobbers_role(RegisterRole::Argument));
        assert!(clobbers.clobbers_role(RegisterRole::Return));
        assert!(clobbers.clobbers_role(RegisterRole::CallerSave));
        assert!(clobbers.clobbers_role(RegisterRole::Scratch));
        assert!(clobbers.preserves_role(RegisterRole::PinnedVm));
        assert!(clobbers.preserves_role(RegisterRole::PinnedCallFrame));
        assert!(clobbers.clobbers_condition_flags);
        assert!(clobbers.clobbers_stack_argument_area);
        assert!(clobbers.may_allocate);
        assert!(clobbers.may_throw);
    }
}
