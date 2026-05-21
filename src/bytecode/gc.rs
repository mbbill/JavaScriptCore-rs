//! Bytecode-level root map metadata.
//!
//! These records describe where a future collector or stack visitor can find
//! precise roots for a linked code block. They do not scan frames, mark cells,
//! keep values alive, or mutate the heap.

use crate::bytecode::code_block::{BytecodeIndex, RuntimeSlot};
use crate::bytecode::instruction::{DecodedInstruction, OperandAccessError};
use crate::bytecode::opcode::CoreOpcode;
use crate::bytecode::VirtualRegister;
use crate::gc::{RootKind, RootSetMutationAuthority};
use crate::runtime::CodeBlockId;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct BytecodeRootMapId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BytecodeRootSlotKind {
    VirtualRegister,
    Argument,
    Constant,
    MetadataSlot,
    InlineCache,
    ValueProfile,
    CallSite,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BytecodeRootSlotStorage {
    Register(VirtualRegister),
    RuntimeSlot(RuntimeSlot),
    ConstantIndex(u32),
    CallSite(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeRootSlotDescriptor {
    pub bytecode_index: BytecodeIndex,
    pub kind: BytecodeRootSlotKind,
    pub storage: BytecodeRootSlotStorage,
    pub root_kind: RootKind,
    pub mutation_authority: RootSetMutationAuthority,
    pub precise: bool,
}

impl BytecodeRootSlotDescriptor {
    pub const fn virtual_register(
        bytecode_index: BytecodeIndex,
        register: VirtualRegister,
        kind: BytecodeRootSlotKind,
    ) -> Self {
        Self {
            bytecode_index,
            kind,
            storage: BytecodeRootSlotStorage::Register(register),
            root_kind: RootKind::VMRegister,
            mutation_authority: RootSetMutationAuthority::VmRegisterFile,
            precise: true,
        }
    }

    pub const fn runtime_slot(
        bytecode_index: BytecodeIndex,
        slot: RuntimeSlot,
        kind: BytecodeRootSlotKind,
    ) -> Self {
        Self {
            bytecode_index,
            kind,
            storage: BytecodeRootSlotStorage::RuntimeSlot(slot),
            root_kind: RootKind::JitCode,
            mutation_authority: RootSetMutationAuthority::JitCodeRegistry,
            precise: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeRootMap {
    pub id: BytecodeRootMapId,
    pub owner: Option<CodeBlockId>,
    pub bytecode_range_start: BytecodeIndex,
    pub bytecode_range_end: BytecodeIndex,
    pub slots: Vec<BytecodeRootSlotDescriptor>,
    pub complete: bool,
}

impl BytecodeRootMap {
    pub fn validate(&self) -> Result<(), BytecodeRootMapValidationError> {
        if self.id.0 == 0 {
            return Err(BytecodeRootMapValidationError::ZeroRootMapId);
        }
        if self.bytecode_range_start > self.bytecode_range_end {
            return Err(BytecodeRootMapValidationError::InvalidBytecodeRange);
        }
        if self.complete && self.slots.is_empty() {
            return Err(BytecodeRootMapValidationError::CompleteMapWithoutSlots);
        }
        for (index, slot) in self.slots.iter().enumerate() {
            if !slot.bytecode_index.is_valid()
                || slot.bytecode_index < self.bytecode_range_start
                || slot.bytecode_index > self.bytecode_range_end
            {
                return Err(BytecodeRootMapValidationError::SlotOutsideRange(
                    slot.bytecode_index,
                ));
            }
            if self.slots[..index].iter().any(|previous| {
                previous.bytecode_index == slot.bytecode_index
                    && previous.kind == slot.kind
                    && previous.storage == slot.storage
            }) {
                return Err(BytecodeRootMapValidationError::DuplicateSlot {
                    bytecode_index: slot.bytecode_index,
                    kind: slot.kind,
                    storage: slot.storage,
                });
            }
            validate_root_slot_storage(*slot)?;
            validate_root_slot_authority(*slot)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BytecodeRootMapBuildError {
    MissingDestinationRegister {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        source: OperandAccessError,
    },
    MissingSourceRegister {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        source: OperandAccessError,
    },
    InvalidGeneratedRootMap {
        root_map: BytecodeRootMapId,
        error: BytecodeRootMapValidationError,
    },
}

pub fn build_p6_no_js_call_helper_root_maps<'a>(
    instructions: impl IntoIterator<Item = DecodedInstruction<'a>>,
    first_id: BytecodeRootMapId,
) -> Result<Vec<BytecodeRootMap>, BytecodeRootMapBuildError> {
    let mut next_id = first_id.0.max(1);
    let mut root_maps = Vec::new();
    for instruction in instructions {
        let Some(root_map) =
            build_p6_no_js_call_helper_root_map(BytecodeRootMapId(next_id), instruction)?
        else {
            continue;
        };
        root_maps.push(root_map);
        next_id = next_id.saturating_add(1);
    }
    Ok(root_maps)
}

pub fn build_p6_no_js_call_helper_root_map(
    id: BytecodeRootMapId,
    instruction: DecodedInstruction<'_>,
) -> Result<Option<BytecodeRootMap>, BytecodeRootMapBuildError> {
    let Some(opcode) = CoreOpcode::from_opcode(instruction.opcode) else {
        return Ok(None);
    };
    let bytecode_index = instruction.bytecode_index;
    let slots = match opcode {
        CoreOpcode::NewObject
        | CoreOpcode::NewArray
        | CoreOpcode::LoadString
        | CoreOpcode::LoadBigInt => {
            let destination = instruction.register_operand(0).map_err(|source| {
                BytecodeRootMapBuildError::MissingDestinationRegister {
                    bytecode_index,
                    opcode,
                    source,
                }
            })?;
            vec![root_slot_for_register(bytecode_index, destination)]
        }
        CoreOpcode::TypeOf | CoreOpcode::ToString => {
            let destination = instruction.register_operand(0).map_err(|source| {
                BytecodeRootMapBuildError::MissingDestinationRegister {
                    bytecode_index,
                    opcode,
                    source,
                }
            })?;
            let source = instruction.register_operand(1).map_err(|source| {
                BytecodeRootMapBuildError::MissingSourceRegister {
                    bytecode_index,
                    opcode,
                    source,
                }
            })?;
            let mut slots = vec![root_slot_for_register(bytecode_index, destination)];
            if source != destination {
                slots.push(root_slot_for_register(bytecode_index, source));
            }
            slots
        }
        _ => return Ok(None),
    };

    let root_map = BytecodeRootMap {
        id,
        owner: None,
        bytecode_range_start: bytecode_index,
        bytecode_range_end: bytecode_index,
        slots,
        // This is helper-boundary completeness for the current no-JS-call
        // helper slice, not a whole-frame safepoint liveness contract.
        complete: true,
    };
    root_map
        .validate()
        .map_err(|error| BytecodeRootMapBuildError::InvalidGeneratedRootMap {
            root_map: id,
            error,
        })?;
    Ok(Some(root_map))
}

pub fn root_slot_for_register(
    bytecode_index: BytecodeIndex,
    register: VirtualRegister,
) -> BytecodeRootSlotDescriptor {
    BytecodeRootSlotDescriptor::virtual_register(
        bytecode_index,
        register,
        root_slot_kind_for_register(register),
    )
}

pub const fn root_slot_kind_for_register(register: VirtualRegister) -> BytecodeRootSlotKind {
    if register.is_constant() {
        BytecodeRootSlotKind::Constant
    } else if register.is_argument_or_header() {
        BytecodeRootSlotKind::Argument
    } else {
        BytecodeRootSlotKind::VirtualRegister
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BytecodeRootMapValidationError {
    ZeroRootMapId,
    InvalidBytecodeRange,
    CompleteMapWithoutSlots,
    SlotOutsideRange(BytecodeIndex),
    DuplicateSlot {
        bytecode_index: BytecodeIndex,
        kind: BytecodeRootSlotKind,
        storage: BytecodeRootSlotStorage,
    },
    InvalidVirtualRegister {
        bytecode_index: BytecodeIndex,
        kind: BytecodeRootSlotKind,
        register: VirtualRegister,
    },
    RootSlotKindStorageMismatch {
        bytecode_index: BytecodeIndex,
        kind: BytecodeRootSlotKind,
        storage: BytecodeRootSlotStorage,
    },
    PreciseConservativeRoot {
        bytecode_index: BytecodeIndex,
        root_kind: RootKind,
        authority: RootSetMutationAuthority,
    },
    RootKindAuthorityMismatch {
        root_kind: RootKind,
        authority: RootSetMutationAuthority,
    },
}

fn validate_root_slot_storage(
    slot: BytecodeRootSlotDescriptor,
) -> Result<(), BytecodeRootMapValidationError> {
    if matches!(
        (slot.root_kind, slot.mutation_authority),
        (
            RootKind::Stack,
            RootSetMutationAuthority::ConservativeScanner
        )
    ) && slot.precise
    {
        return Err(BytecodeRootMapValidationError::PreciseConservativeRoot {
            bytecode_index: slot.bytecode_index,
            root_kind: slot.root_kind,
            authority: slot.mutation_authority,
        });
    }

    match slot.storage {
        BytecodeRootSlotStorage::Register(register) => {
            if !register.is_valid() {
                return Err(BytecodeRootMapValidationError::InvalidVirtualRegister {
                    bytecode_index: slot.bytecode_index,
                    kind: slot.kind,
                    register,
                });
            }

            let matches_kind = match slot.kind {
                BytecodeRootSlotKind::VirtualRegister => !register.is_constant(),
                BytecodeRootSlotKind::Argument => register.is_argument_or_header(),
                BytecodeRootSlotKind::Constant => register.is_constant(),
                BytecodeRootSlotKind::MetadataSlot
                | BytecodeRootSlotKind::InlineCache
                | BytecodeRootSlotKind::ValueProfile
                | BytecodeRootSlotKind::CallSite => false,
            };
            if matches_kind {
                Ok(())
            } else {
                Err(
                    BytecodeRootMapValidationError::RootSlotKindStorageMismatch {
                        bytecode_index: slot.bytecode_index,
                        kind: slot.kind,
                        storage: slot.storage,
                    },
                )
            }
        }
        BytecodeRootSlotStorage::RuntimeSlot(_) => match slot.kind {
            BytecodeRootSlotKind::MetadataSlot
            | BytecodeRootSlotKind::InlineCache
            | BytecodeRootSlotKind::ValueProfile => Ok(()),
            BytecodeRootSlotKind::VirtualRegister
            | BytecodeRootSlotKind::Argument
            | BytecodeRootSlotKind::Constant
            | BytecodeRootSlotKind::CallSite => Err(
                BytecodeRootMapValidationError::RootSlotKindStorageMismatch {
                    bytecode_index: slot.bytecode_index,
                    kind: slot.kind,
                    storage: slot.storage,
                },
            ),
        },
        BytecodeRootSlotStorage::ConstantIndex(_) => {
            if slot.kind == BytecodeRootSlotKind::Constant {
                Ok(())
            } else {
                Err(
                    BytecodeRootMapValidationError::RootSlotKindStorageMismatch {
                        bytecode_index: slot.bytecode_index,
                        kind: slot.kind,
                        storage: slot.storage,
                    },
                )
            }
        }
        BytecodeRootSlotStorage::CallSite(_) => {
            if slot.kind == BytecodeRootSlotKind::CallSite {
                Ok(())
            } else {
                Err(
                    BytecodeRootMapValidationError::RootSlotKindStorageMismatch {
                        bytecode_index: slot.bytecode_index,
                        kind: slot.kind,
                        storage: slot.storage,
                    },
                )
            }
        }
    }
}

fn validate_root_slot_authority(
    slot: BytecodeRootSlotDescriptor,
) -> Result<(), BytecodeRootMapValidationError> {
    let accepted = matches!(
        (slot.root_kind, slot.mutation_authority),
        (
            RootKind::VMRegister,
            RootSetMutationAuthority::VmRegisterFile
        ) | (RootKind::JitCode, RootSetMutationAuthority::JitCodeRegistry)
            | (
                RootKind::ExplicitRoot,
                RootSetMutationAuthority::ExplicitRootRegistry
            )
            | (RootKind::Handle, RootSetMutationAuthority::HandleScope)
            | (RootKind::Host, RootSetMutationAuthority::HostIntegration)
            | (
                RootKind::Stack,
                RootSetMutationAuthority::ConservativeScanner
            )
    );
    if accepted {
        Ok(())
    } else {
        Err(BytecodeRootMapValidationError::RootKindAuthorityMismatch {
            root_kind: slot.root_kind,
            authority: slot.mutation_authority,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::instruction::{InstructionBuilder, Operand};
    use crate::bytecode::opcode::OperandWidth;
    use crate::gc::CellId;

    fn single_instruction_root_maps(
        opcode: CoreOpcode,
        operands: Vec<Operand>,
    ) -> Vec<BytecodeRootMap> {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(opcode.opcode(), OperandWidth::Narrow, operands);
        let stream = builder.finalize();
        build_p6_no_js_call_helper_root_maps(
            stream
                .decoded_instructions()
                .map(|instruction| instruction.expect("decoded instruction")),
            BytecodeRootMapId(1),
        )
        .expect("root maps")
    }

    #[test]
    fn validates_precise_bytecode_root_map() {
        let start = BytecodeIndex::from_offset(4);
        let end = BytecodeIndex::from_offset(8);
        let map = BytecodeRootMap {
            id: BytecodeRootMapId(1),
            owner: Some(CodeBlockId(CellId(9))),
            bytecode_range_start: start,
            bytecode_range_end: end,
            slots: vec![
                BytecodeRootSlotDescriptor::virtual_register(
                    start,
                    VirtualRegister::from_raw(2),
                    BytecodeRootSlotKind::VirtualRegister,
                ),
                BytecodeRootSlotDescriptor::runtime_slot(
                    end,
                    RuntimeSlot(7),
                    BytecodeRootSlotKind::InlineCache,
                ),
            ],
            complete: true,
        };

        assert_eq!(map.validate(), Ok(()));
    }

    #[test]
    fn rejects_duplicate_root_slots() {
        let index = BytecodeIndex::from_offset(4);
        let slot = BytecodeRootSlotDescriptor::virtual_register(
            index,
            VirtualRegister::from_raw(2),
            BytecodeRootSlotKind::VirtualRegister,
        );
        let map = BytecodeRootMap {
            id: BytecodeRootMapId(1),
            owner: None,
            bytecode_range_start: index,
            bytecode_range_end: index,
            slots: vec![slot, slot],
            complete: true,
        };

        assert_eq!(
            map.validate(),
            Err(BytecodeRootMapValidationError::DuplicateSlot {
                bytecode_index: index,
                kind: BytecodeRootSlotKind::VirtualRegister,
                storage: BytecodeRootSlotStorage::Register(VirtualRegister::from_raw(2)),
            })
        );
    }

    #[test]
    fn builds_destination_only_helper_root_maps() {
        for opcode in [CoreOpcode::NewObject, CoreOpcode::NewArray] {
            let destination = VirtualRegister::local(0);
            let maps = single_instruction_root_maps(opcode, vec![Operand::Register(destination)]);

            assert_eq!(maps.len(), 1);
            assert_eq!(maps[0].owner, None);
            assert_eq!(maps[0].bytecode_range_start, BytecodeIndex::from_offset(0));
            assert_eq!(maps[0].bytecode_range_end, BytecodeIndex::from_offset(0));
            assert!(maps[0].complete);
            assert_eq!(
                maps[0].slots,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    BytecodeIndex::from_offset(0),
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )]
            );
        }
    }

    #[test]
    fn builds_literal_load_helper_root_maps_without_literal_validation() {
        for opcode in [CoreOpcode::LoadString, CoreOpcode::LoadBigInt] {
            let destination = VirtualRegister::local(1);
            let maps = single_instruction_root_maps(
                opcode,
                vec![Operand::Register(destination), Operand::IdentifierIndex(17)],
            );

            assert_eq!(maps.len(), 1);
            assert_eq!(
                maps[0].slots,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    BytecodeIndex::from_offset(0),
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )]
            );
        }
    }

    #[test]
    fn builds_destination_and_source_root_map_with_argument_kind() {
        for opcode in [CoreOpcode::TypeOf, CoreOpcode::ToString] {
            let destination = VirtualRegister::local(2);
            let source = VirtualRegister::argument_or_header(5);
            let maps = single_instruction_root_maps(
                opcode,
                vec![Operand::Register(destination), Operand::Register(source)],
            );

            assert_eq!(maps.len(), 1);
            assert_eq!(
                maps[0].slots,
                vec![
                    BytecodeRootSlotDescriptor::virtual_register(
                        BytecodeIndex::from_offset(0),
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    BytecodeRootSlotDescriptor::virtual_register(
                        BytecodeIndex::from_offset(0),
                        source,
                        BytecodeRootSlotKind::Argument,
                    ),
                ]
            );
        }
    }

    #[test]
    fn dedupes_destination_source_root_map_when_source_matches_destination() {
        for opcode in [CoreOpcode::TypeOf, CoreOpcode::ToString] {
            let register = VirtualRegister::local(3);
            let maps = single_instruction_root_maps(
                opcode,
                vec![Operand::Register(register), Operand::Register(register)],
            );

            assert_eq!(maps.len(), 1);
            assert_eq!(maps[0].slots.len(), 1);
            assert_eq!(
                maps[0].slots[0].storage,
                BytecodeRootSlotStorage::Register(register)
            );
        }
    }

    #[test]
    fn does_not_build_maps_for_property_call_or_constructor_opcodes() {
        let destination = VirtualRegister::local(0);
        let source = VirtualRegister::local(1);
        for (opcode, operands) in [
            (
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(destination),
                    Operand::Register(source),
                    Operand::IdentifierIndex(1),
                ],
            ),
            (
                CoreOpcode::Call,
                vec![Operand::Register(destination), Operand::Register(source)],
            ),
            (
                CoreOpcode::LoadStringConstructor,
                vec![Operand::Register(destination)],
            ),
        ] {
            assert!(single_instruction_root_maps(opcode, operands).is_empty());
        }
    }
}
