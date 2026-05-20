use crate::bytecode::code_block::{CodeBlockEntrypoints, CodeKind, InterpreterEntrySlot};
use crate::llint::dispatch::{LLIntCodePtr, OpcodeSizeClass};

/// LLInt entrypoint set installed on a linked code block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntEntrypointTable {
    pub call: Option<LLIntEntrypoint>,
    pub construct: Option<LLIntEntrypoint>,
    pub arity_check_call: Option<LLIntEntrypoint>,
    pub arity_check_construct: Option<LLIntEntrypoint>,
    pub return_points: Vec<LLIntReturnPoint>,
    pub thunks: LLIntThunkSet,
    pub install_state: LLIntEntrypointState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LLIntEntrypointValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicateEntrypointKind(LLIntEntrypointKind),
    EmptyAllowedOpcodeSizes(&'static str),
    MissingRequiredEntrypoint(LLIntEntrypointKind),
    EntrypointKindMismatch,
    InstalledWithoutCode,
    InstallMutationMismatch,
    ReturnPointWithoutThunk,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntEntrypointState {
    #[default]
    Uninstalled,
    InstalledOnCodeBlock,
    ReplacedByJit,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntEntrypoint {
    pub kind: LLIntEntrypointKind,
    pub slot: InterpreterEntrySlot,
    pub code: Option<LLIntCodePtr>,
    pub frame_register_count: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntEntrypointKind {
    Program,
    Eval,
    Module,
    FunctionForCall,
    FunctionForConstruct,
    HostCallReturnValue,
    FuzzerReturnEarlyFromLoopHint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntReturnPoint {
    pub opcode_size: OpcodeSizeClass,
    pub code: LLIntCodePtr,
    pub purpose: LLIntReturnPointPurpose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntReturnPointPurpose {
    Generic,
    ExceptionCatch,
    ExceptionUncaught,
    CheckpointOsrExit,
    ArraySortComparator,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct LLIntThunkSet {
    pub default_call: Option<LLIntCodePtr>,
    pub arity_fixup: Option<LLIntCodePtr>,
    pub handle_uncaught_exception: Option<LLIntCodePtr>,
    pub call_to_throw: Option<LLIntCodePtr>,
}

/// Pending entrypoint installation request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntEntrypointInstall {
    pub entrypoints: CodeBlockEntrypoints,
    pub table: LLIntEntrypointTable,
    pub frame_register_count: Option<u32>,
    /// LLInt entrypoint installation mutates code-block entry slots; generated
    /// LLInt thunks and slow paths only provide code references.
    pub mutates_code_block_entrypoints: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntEntrypointSchemaOwner {
    #[default]
    OfflineAsmGeneratedData,
    LLIntEntrypointRegistry,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntEntrypointRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    LinkTimeInstallation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticLLIntEntrypointSchema {
    pub kind: LLIntEntrypointKind,
    pub name: &'static str,
    pub allowed_opcode_sizes: &'static [OpcodeSizeClass],
    pub installs_code_block_slot: bool,
    pub owner: LLIntEntrypointSchemaOwner,
    pub mutation_authority: LLIntEntrypointRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LLIntEntrypointSchemaRegistry {
    pub entrypoints: &'static [StaticLLIntEntrypointSchema],
}

impl LLIntEntrypointSchemaRegistry {
    pub const fn new(entrypoints: &'static [StaticLLIntEntrypointSchema]) -> Self {
        Self { entrypoints }
    }

    pub const fn entrypoints(self) -> &'static [StaticLLIntEntrypointSchema] {
        self.entrypoints
    }

    pub fn schema_for_kind(
        self,
        kind: LLIntEntrypointKind,
    ) -> Option<&'static StaticLLIntEntrypointSchema> {
        self.entrypoints.iter().find(|schema| schema.kind == kind)
    }

    pub fn validate(self) -> Result<(), LLIntEntrypointValidationError> {
        for (index, entrypoint) in self.entrypoints.iter().enumerate() {
            entrypoint.validate()?;
            if self.entrypoints[index + 1..]
                .iter()
                .any(|other| other.kind == entrypoint.kind)
            {
                return Err(LLIntEntrypointValidationError::DuplicateEntrypointKind(
                    entrypoint.kind,
                ));
            }
        }

        Ok(())
    }
}

impl StaticLLIntEntrypointSchema {
    pub fn validate(&self) -> Result<(), LLIntEntrypointValidationError> {
        if self.name.is_empty() {
            return Err(LLIntEntrypointValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(LLIntEntrypointValidationError::EmptyProvenance(self.name));
        }
        if self.allowed_opcode_sizes.is_empty() {
            return Err(LLIntEntrypointValidationError::EmptyAllowedOpcodeSizes(
                self.name,
            ));
        }

        Ok(())
    }
}

impl LLIntEntrypointTable {
    pub fn from_entrypoints(entrypoints: impl IntoIterator<Item = LLIntEntrypoint>) -> Self {
        let mut table = Self::default();
        for entrypoint in entrypoints {
            match entrypoint.kind {
                LLIntEntrypointKind::Program | LLIntEntrypointKind::FunctionForCall => {
                    table.call = Some(entrypoint);
                }
                LLIntEntrypointKind::Eval
                | LLIntEntrypointKind::Module
                | LLIntEntrypointKind::HostCallReturnValue
                | LLIntEntrypointKind::FuzzerReturnEarlyFromLoopHint => {
                    table.call = Some(entrypoint);
                }
                LLIntEntrypointKind::FunctionForConstruct => {
                    table.construct = Some(entrypoint);
                }
            }
        }
        table
    }

    pub fn validate_against(
        &self,
        registry: LLIntEntrypointSchemaRegistry,
    ) -> Result<(), LLIntEntrypointValidationError> {
        registry.validate()?;

        for entrypoint in [
            self.call,
            self.construct,
            self.arity_check_call,
            self.arity_check_construct,
        ]
        .into_iter()
        .flatten()
        {
            if registry.schema_for_kind(entrypoint.kind).is_none() {
                return Err(LLIntEntrypointValidationError::EntrypointKindMismatch);
            }
            if self.install_state == LLIntEntrypointState::InstalledOnCodeBlock
                && entrypoint.code.is_none()
            {
                return Err(LLIntEntrypointValidationError::InstalledWithoutCode);
            }
        }

        if !self.return_points.is_empty() && self.thunks.default_call.is_none() {
            return Err(LLIntEntrypointValidationError::ReturnPointWithoutThunk);
        }

        Ok(())
    }

    pub fn has_entrypoint(&self, kind: LLIntEntrypointKind) -> bool {
        [
            self.call,
            self.construct,
            self.arity_check_call,
            self.arity_check_construct,
        ]
        .into_iter()
        .flatten()
        .any(|entrypoint| entrypoint.kind == kind)
    }
}

pub fn select_llint_entrypoint_kinds(
    code_kind: CodeKind,
    constructable: bool,
) -> Vec<LLIntEntrypointKind> {
    let mut kinds = match code_kind {
        CodeKind::Program => vec![LLIntEntrypointKind::Program],
        CodeKind::Eval => vec![LLIntEntrypointKind::Eval],
        CodeKind::Module => vec![LLIntEntrypointKind::Module],
        CodeKind::Function => vec![LLIntEntrypointKind::FunctionForCall],
    };
    if constructable {
        kinds.push(LLIntEntrypointKind::FunctionForConstruct);
    }
    kinds
}

pub fn select_llint_entrypoint_table(
    available: impl IntoIterator<Item = LLIntEntrypoint>,
    required: &[LLIntEntrypointKind],
) -> Result<LLIntEntrypointTable, LLIntEntrypointValidationError> {
    let table = LLIntEntrypointTable::from_entrypoints(available);
    for kind in required {
        if !table.has_entrypoint(*kind) {
            return Err(LLIntEntrypointValidationError::MissingRequiredEntrypoint(
                *kind,
            ));
        }
    }
    table.validate_against(LLINT_ENTRYPOINT_SCHEMA_REGISTRY)?;
    Ok(table)
}

impl LLIntEntrypointInstall {
    pub fn validate(&self) -> Result<(), LLIntEntrypointValidationError> {
        self.table
            .validate_against(LLINT_ENTRYPOINT_SCHEMA_REGISTRY)?;
        if self.mutates_code_block_entrypoints
            && self.table.install_state == LLIntEntrypointState::Uninstalled
        {
            return Err(LLIntEntrypointValidationError::InstallMutationMismatch);
        }

        Ok(())
    }
}

const ALL_OPCODE_SIZE_CLASSES: &[OpcodeSizeClass] = &[
    OpcodeSizeClass::Narrow,
    OpcodeSizeClass::Wide16,
    OpcodeSizeClass::Wide32,
];

pub const STATIC_LLINT_ENTRYPOINT_SCHEMAS: &[StaticLLIntEntrypointSchema] = &[
    StaticLLIntEntrypointSchema {
        kind: LLIntEntrypointKind::Program,
        name: "program-entry",
        allowed_opcode_sizes: ALL_OPCODE_SIZE_CLASSES,
        installs_code_block_slot: true,
        owner: LLIntEntrypointSchemaOwner::OfflineAsmGeneratedData,
        mutation_authority: LLIntEntrypointRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt entrypoint schema",
    },
    StaticLLIntEntrypointSchema {
        kind: LLIntEntrypointKind::Eval,
        name: "eval-entry",
        allowed_opcode_sizes: ALL_OPCODE_SIZE_CLASSES,
        installs_code_block_slot: true,
        owner: LLIntEntrypointSchemaOwner::OfflineAsmGeneratedData,
        mutation_authority: LLIntEntrypointRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt entrypoint schema",
    },
    StaticLLIntEntrypointSchema {
        kind: LLIntEntrypointKind::Module,
        name: "module-entry",
        allowed_opcode_sizes: ALL_OPCODE_SIZE_CLASSES,
        installs_code_block_slot: true,
        owner: LLIntEntrypointSchemaOwner::OfflineAsmGeneratedData,
        mutation_authority: LLIntEntrypointRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt entrypoint schema",
    },
    StaticLLIntEntrypointSchema {
        kind: LLIntEntrypointKind::FunctionForCall,
        name: "function-call-entry",
        allowed_opcode_sizes: ALL_OPCODE_SIZE_CLASSES,
        installs_code_block_slot: true,
        owner: LLIntEntrypointSchemaOwner::OfflineAsmGeneratedData,
        mutation_authority: LLIntEntrypointRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt entrypoint schema",
    },
    StaticLLIntEntrypointSchema {
        kind: LLIntEntrypointKind::FunctionForConstruct,
        name: "function-construct-entry",
        allowed_opcode_sizes: ALL_OPCODE_SIZE_CLASSES,
        installs_code_block_slot: true,
        owner: LLIntEntrypointSchemaOwner::OfflineAsmGeneratedData,
        mutation_authority: LLIntEntrypointRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt entrypoint schema",
    },
];

pub const LLINT_ENTRYPOINT_SCHEMA_REGISTRY: LLIntEntrypointSchemaRegistry =
    LLIntEntrypointSchemaRegistry::new(STATIC_LLINT_ENTRYPOINT_SCHEMAS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_entrypoint_registry_validates() {
        assert_eq!(LLINT_ENTRYPOINT_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn installed_table_requires_generated_code_references() {
        let table = LLIntEntrypointTable {
            call: Some(LLIntEntrypoint {
                kind: LLIntEntrypointKind::Program,
                slot: InterpreterEntrySlot(0),
                code: None,
                frame_register_count: Some(1),
            }),
            install_state: LLIntEntrypointState::InstalledOnCodeBlock,
            ..LLIntEntrypointTable::default()
        };

        assert_eq!(
            table.validate_against(LLINT_ENTRYPOINT_SCHEMA_REGISTRY),
            Err(LLIntEntrypointValidationError::InstalledWithoutCode)
        );
    }

    #[test]
    fn entrypoint_selection_maps_function_constructability() {
        assert_eq!(
            select_llint_entrypoint_kinds(CodeKind::Function, true),
            vec![
                LLIntEntrypointKind::FunctionForCall,
                LLIntEntrypointKind::FunctionForConstruct
            ]
        );
        assert_eq!(
            select_llint_entrypoint_kinds(CodeKind::Eval, false),
            vec![LLIntEntrypointKind::Eval]
        );
    }

    #[test]
    fn entrypoint_table_selection_rejects_missing_required_kind() {
        let available = [LLIntEntrypoint {
            kind: LLIntEntrypointKind::FunctionForCall,
            slot: InterpreterEntrySlot(1),
            code: Some(LLIntCodePtr(10)),
            frame_register_count: Some(2),
        }];

        assert_eq!(
            select_llint_entrypoint_table(
                available,
                &[
                    LLIntEntrypointKind::FunctionForCall,
                    LLIntEntrypointKind::FunctionForConstruct
                ]
            ),
            Err(LLIntEntrypointValidationError::MissingRequiredEntrypoint(
                LLIntEntrypointKind::FunctionForConstruct
            ))
        );
    }
}
