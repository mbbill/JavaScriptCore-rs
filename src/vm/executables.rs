use std::collections::HashMap;
use std::sync::Arc;

use crate::bytecode::{
    CodeBlock, CodeSpecialization, ConstructAbility, ExecutableBase, ExecutableEntryCacheRecord,
    ExecutableEntryPublicationError, ExecutableEntryPublicationRequest, ExecutableEntrypoints,
    ExecutableFunctionParseMetadata, ExecutableParseGoal, ExecutableParseSemanticMetadata,
    FunctionExecutable, FunctionExecutableRareData, FunctionMode, InterpreterEntrypointSlot,
    ParseMode, ParseRecord, ScriptExecutable, ScriptExecutableKind, UnlinkedCodeBlock,
    UnlinkedFunctionExecutable, UnlinkedFunctionExecutableRareData, UnlinkedFunctionKind,
};
use crate::runtime::{CodeBlockId, CodeSpecializationKind, ExecutableId};

#[derive(Clone, Debug, Default)]
pub(crate) struct ExecutableRegistry {
    scripts: HashMap<ExecutableId, ScriptExecutableRecord>,
    functions: HashMap<ExecutableId, FunctionExecutableRecord>,
    code_block_owners: HashMap<CodeBlockId, ExecutableId>,
    install_records: Vec<ExecutableInstallRecord>,
}

impl ExecutableRegistry {
    pub(crate) fn register_script(
        &mut self,
        executable_id: ExecutableId,
        kind: ScriptExecutableKind,
        unlinked: Arc<UnlinkedCodeBlock>,
    ) -> Result<(), ExecutableRegistryError> {
        if self.contains_executable(executable_id) {
            return Err(ExecutableRegistryError::DuplicateExecutable {
                executable: executable_id,
            });
        }

        let executable = ScriptExecutable {
            base: ExecutableBase {
                identity: Some(executable_id),
                ..ExecutableBase::default()
            },
            kind,
            source: unlinked.source().clone(),
            parse_record: ParseRecord {
                features: unlinked.features(),
                semantic: ExecutableParseSemanticMetadata {
                    goal: parse_goal_for_script_kind(kind),
                    ..ExecutableParseSemanticMetadata::default()
                },
                ..ParseRecord::default()
            },
            unlinked: Some(unlinked),
            installed_code: None,
        };
        self.scripts.insert(
            executable_id,
            ScriptExecutableRecord {
                executable,
                installed_code: None,
                publications: Vec::new(),
            },
        );
        Ok(())
    }

    pub(crate) fn register_function(
        &mut self,
        executable_id: ExecutableId,
        call_unlinked: Arc<UnlinkedCodeBlock>,
    ) -> Result<(), ExecutableRegistryError> {
        if self.contains_executable(executable_id) {
            return Err(ExecutableRegistryError::DuplicateExecutable {
                executable: executable_id,
            });
        }

        let unlinked = Arc::new(synthesize_unlinked_function_executable(
            call_unlinked.clone(),
        ));
        let script = ScriptExecutable {
            base: ExecutableBase {
                identity: Some(executable_id),
                ..ExecutableBase::default()
            },
            kind: ScriptExecutableKind::Function,
            source: call_unlinked.source().clone(),
            parse_record: ParseRecord {
                features: call_unlinked.features(),
                semantic: ExecutableParseSemanticMetadata {
                    goal: ExecutableParseGoal::Function,
                    function: Some(ExecutableFunctionParseMetadata {
                        parameter_count_excluding_this: unlinked.parameter_count_excluding_this,
                        has_non_simple_parameters: call_unlinked
                            .features()
                            .has_non_simple_parameters,
                        needs_arguments_object: call_unlinked.features().uses_arguments,
                        private_brand_requirement: unlinked
                            .executable_info
                            .private_brand_requirement,
                        needs_class_field_initializer: unlinked
                            .executable_info
                            .needs_class_field_initializer,
                    }),
                    ..ExecutableParseSemanticMetadata::default()
                },
                ..ParseRecord::default()
            },
            unlinked: None,
            installed_code: None,
        };
        let executable = FunctionExecutable {
            script,
            unlinked,
            call_code: None,
            construct_code: None,
            rare: FunctionExecutableRareData::default(),
        };
        self.functions.insert(
            executable_id,
            FunctionExecutableRecord {
                executable,
                installed_call_code: None,
                installed_construct_code: None,
                publications: Vec::new(),
            },
        );
        Ok(())
    }

    fn contains_executable(&self, executable: ExecutableId) -> bool {
        self.scripts.contains_key(&executable) || self.functions.contains_key(&executable)
    }

    #[allow(dead_code)]
    pub(crate) fn script(&self, executable: ExecutableId) -> Option<&ScriptExecutableRecord> {
        self.scripts.get(&executable)
    }

    #[allow(dead_code)]
    pub(crate) fn function(&self, executable: ExecutableId) -> Option<&FunctionExecutableRecord> {
        self.functions.get(&executable)
    }

    #[allow(dead_code)]
    pub(crate) fn function_records(&self) -> impl Iterator<Item = &FunctionExecutableRecord> {
        self.functions.values()
    }

    #[allow(dead_code)]
    pub(crate) fn executable_for_code_block(
        &self,
        code_block: CodeBlockId,
    ) -> Option<ExecutableId> {
        self.code_block_owners.get(&code_block).copied()
    }

    pub(crate) fn install_script_code(
        &mut self,
        executable: ExecutableId,
        code_block: CodeBlockId,
        specialization: CodeSpecialization,
        linked_code: &CodeBlock,
    ) -> ExecutableInstallRecord {
        let ordinal = self.install_records.len() as u64 + 1;
        let previous_code_block = self
            .scripts
            .get(&executable)
            .and_then(|record| record.installed_code.map(|installed| installed.code_block));
        let outcome = self
            .install_script_code_inner(executable, code_block, specialization, linked_code)
            .map_or_else(ExecutableInstallOutcome::Rejected, |installed| {
                ExecutableInstallOutcome::Installed { installed }
            });
        let record = ExecutableInstallRecord {
            ordinal,
            executable,
            code_block,
            specialization,
            previous_code_block,
            outcome,
        };
        self.install_records.push(record);
        record
    }

    pub(crate) fn install_function_code(
        &mut self,
        executable: ExecutableId,
        code_block: CodeBlockId,
        specialization: CodeSpecialization,
        linked_code: &CodeBlock,
    ) -> ExecutableInstallRecord {
        let ordinal = self.install_records.len() as u64 + 1;
        let previous_code_block = self.functions.get(&executable).and_then(|record| {
            record
                .installed_code_for(specialization)
                .map(|installed| installed.code_block)
        });
        let outcome = self
            .install_function_code_inner(executable, code_block, specialization, linked_code)
            .map_or_else(ExecutableInstallOutcome::Rejected, |installed| {
                ExecutableInstallOutcome::Installed { installed }
            });
        let record = ExecutableInstallRecord {
            ordinal,
            executable,
            code_block,
            specialization,
            previous_code_block,
            outcome,
        };
        self.install_records.push(record);
        record
    }

    #[allow(dead_code)]
    pub(crate) fn install_records(&self) -> &[ExecutableInstallRecord] {
        &self.install_records
    }

    pub(crate) fn publish_baseline_native_entry(
        &mut self,
        executable: ExecutableId,
        request: ExecutableEntryPublicationRequest,
        code_block: &CodeBlock,
    ) -> ExecutableRegistryEntryPublication {
        if let Some(record) = self.scripts.get_mut(&executable) {
            let Some(installed) = record.installed_code else {
                return ExecutableRegistryEntryPublication::Rejected(
                    ExecutableRegistryPublicationError::NoInstalledCode { executable },
                );
            };
            if installed.code_block != request.launch_descriptor.code_block {
                return ExecutableRegistryEntryPublication::Rejected(
                    ExecutableRegistryPublicationError::InstalledCodeBlockMismatch {
                        expected: installed.code_block,
                        actual: request.launch_descriptor.code_block,
                    },
                );
            }

            let request = request.with_executable(Some(executable));
            return match record
                .executable
                .base
                .entrypoints
                .publish_baseline_native_entry(request, installed.code_block, code_block)
            {
                Ok(publication) => {
                    record.publications.push(publication);
                    ExecutableRegistryEntryPublication::Published(publication)
                }
                Err(error) => ExecutableRegistryEntryPublication::Rejected(
                    ExecutableRegistryPublicationError::ExecutablePublication(error),
                ),
            };
        }

        if let Some(record) = self.functions.get_mut(&executable) {
            let Some(installed) = record.installed_code_for(code_specialization_for_kind(
                request.launch_descriptor.call_frame.specialization,
            )) else {
                return ExecutableRegistryEntryPublication::Rejected(
                    ExecutableRegistryPublicationError::NoInstalledCode { executable },
                );
            };
            if installed.code_block != request.launch_descriptor.code_block {
                return ExecutableRegistryEntryPublication::Rejected(
                    ExecutableRegistryPublicationError::InstalledCodeBlockMismatch {
                        expected: installed.code_block,
                        actual: request.launch_descriptor.code_block,
                    },
                );
            }

            return match record
                .executable
                .publish_baseline_native_entry_for(request.with_executable(Some(executable)))
            {
                Ok(publication) => {
                    record.publications.push(publication);
                    ExecutableRegistryEntryPublication::Published(publication)
                }
                Err(error) => ExecutableRegistryEntryPublication::Rejected(
                    ExecutableRegistryPublicationError::ExecutablePublication(error),
                ),
            };
        }

        ExecutableRegistryEntryPublication::NotRegistered
    }

    fn install_script_code_inner(
        &mut self,
        executable: ExecutableId,
        code_block: CodeBlockId,
        specialization: CodeSpecialization,
        linked_code: &CodeBlock,
    ) -> Result<InstalledExecutableCode, ExecutableInstallRejection> {
        let Some(record) = self.scripts.get_mut(&executable) else {
            return Err(ExecutableInstallRejection::ExecutableNotRegistered { executable });
        };
        if linked_code.link_context().owner_executable != Some(executable) {
            return Err(ExecutableInstallRejection::CodeBlockExecutableMismatch {
                expected: executable,
                actual: linked_code.link_context().owner_executable,
            });
        }
        if !code_specialization_matches_install(specialization, linked_code) {
            return Err(
                ExecutableInstallRejection::CodeBlockSpecializationMismatch {
                    expected: specialization,
                    actual: linked_code.link_context().specialization,
                },
            );
        }

        let installed = InstalledExecutableCode {
            code_block,
            specialization,
        };
        record.installed_code = Some(installed);
        record.executable.installed_code = Some(linked_code.clone());
        install_interpreter_entrypoint(
            &mut record.executable.base.entrypoints,
            specialization,
            linked_code,
        );
        self.code_block_owners.insert(code_block, executable);
        Ok(installed)
    }

    fn install_function_code_inner(
        &mut self,
        executable: ExecutableId,
        code_block: CodeBlockId,
        specialization: CodeSpecialization,
        linked_code: &CodeBlock,
    ) -> Result<InstalledExecutableCode, ExecutableInstallRejection> {
        let Some(record) = self.functions.get_mut(&executable) else {
            return Err(ExecutableInstallRejection::ExecutableNotRegistered { executable });
        };
        if linked_code.link_context().owner_executable != Some(executable) {
            return Err(ExecutableInstallRejection::CodeBlockExecutableMismatch {
                expected: executable,
                actual: linked_code.link_context().owner_executable,
            });
        }
        if !matches!(
            specialization,
            CodeSpecialization::Call | CodeSpecialization::Construct
        ) || !code_specialization_matches_install(specialization, linked_code)
        {
            return Err(
                ExecutableInstallRejection::CodeBlockSpecializationMismatch {
                    expected: specialization,
                    actual: linked_code.link_context().specialization,
                },
            );
        }

        let installed = InstalledExecutableCode {
            code_block,
            specialization,
        };
        match specialization {
            CodeSpecialization::Call => {
                record.installed_call_code = Some(installed);
                record.executable.call_code = Some(linked_code.clone());
            }
            CodeSpecialization::Construct => {
                record.installed_construct_code = Some(installed);
                record.executable.construct_code = Some(linked_code.clone());
            }
            CodeSpecialization::None => unreachable!("function install rejects unspecialized code"),
        }
        install_interpreter_entrypoint(
            &mut record.executable.script.base.entrypoints,
            specialization,
            linked_code,
        );
        self.code_block_owners.insert(code_block, executable);
        Ok(installed)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ScriptExecutableRecord {
    executable: ScriptExecutable,
    installed_code: Option<InstalledExecutableCode>,
    publications: Vec<ExecutableEntryCacheRecord>,
}

impl ScriptExecutableRecord {
    #[allow(dead_code)]
    pub(crate) fn executable(&self) -> &ScriptExecutable {
        &self.executable
    }

    #[allow(dead_code)]
    pub(crate) fn installed_code(&self) -> Option<InstalledExecutableCode> {
        self.installed_code
    }

    #[allow(dead_code)]
    pub(crate) fn publications(&self) -> &[ExecutableEntryCacheRecord] {
        &self.publications
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FunctionExecutableRecord {
    executable: FunctionExecutable,
    installed_call_code: Option<InstalledExecutableCode>,
    installed_construct_code: Option<InstalledExecutableCode>,
    publications: Vec<ExecutableEntryCacheRecord>,
}

impl FunctionExecutableRecord {
    #[allow(dead_code)]
    pub(crate) fn executable(&self) -> &FunctionExecutable {
        &self.executable
    }

    #[allow(dead_code)]
    pub(crate) fn installed_call_code(&self) -> Option<InstalledExecutableCode> {
        self.installed_call_code
    }

    #[allow(dead_code)]
    pub(crate) fn installed_construct_code(&self) -> Option<InstalledExecutableCode> {
        self.installed_construct_code
    }

    #[allow(dead_code)]
    pub(crate) fn installed_code_for(
        &self,
        specialization: CodeSpecialization,
    ) -> Option<InstalledExecutableCode> {
        match specialization {
            CodeSpecialization::Call => self.installed_call_code,
            CodeSpecialization::Construct => self.installed_construct_code,
            CodeSpecialization::None => self.installed_call_code.or(self.installed_construct_code),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn publications(&self) -> &[ExecutableEntryCacheRecord] {
        &self.publications
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InstalledExecutableCode {
    pub code_block: CodeBlockId,
    pub specialization: CodeSpecialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExecutableInstallRecord {
    pub ordinal: u64,
    pub executable: ExecutableId,
    pub code_block: CodeBlockId,
    pub specialization: CodeSpecialization,
    pub previous_code_block: Option<CodeBlockId>,
    pub outcome: ExecutableInstallOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutableInstallOutcome {
    Installed { installed: InstalledExecutableCode },
    Rejected(ExecutableInstallRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableInstallRejection {
    ExecutableNotRegistered {
        executable: ExecutableId,
    },
    CodeBlockExecutableMismatch {
        expected: ExecutableId,
        actual: Option<ExecutableId>,
    },
    CodeBlockSpecializationMismatch {
        expected: CodeSpecialization,
        actual: CodeSpecialization,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableRegistryError {
    DuplicateExecutable { executable: ExecutableId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutableRegistryEntryPublication {
    NotRegistered,
    Published(ExecutableEntryCacheRecord),
    Rejected(ExecutableRegistryPublicationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutableRegistryPublicationError {
    NoInstalledCode {
        executable: ExecutableId,
    },
    InstalledCodeBlockMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    ExecutablePublication(ExecutableEntryPublicationError),
}

fn parse_goal_for_script_kind(kind: ScriptExecutableKind) -> ExecutableParseGoal {
    match kind {
        ScriptExecutableKind::Program => ExecutableParseGoal::Program,
        ScriptExecutableKind::Eval => ExecutableParseGoal::Eval,
        ScriptExecutableKind::ModuleProgram => ExecutableParseGoal::Module,
        ScriptExecutableKind::Function => ExecutableParseGoal::Function,
    }
}

fn synthesize_unlinked_function_executable(
    call_code: Arc<UnlinkedCodeBlock>,
) -> UnlinkedFunctionExecutable {
    let executable_info = call_code.executable_info().clone();
    let parameter_count_excluding_this = call_code
        .frame()
        .num_parameters_including_this
        .saturating_sub(1);
    let function_mode = function_mode_for_executable_info(&executable_info);
    let unlinked_kind = if executable_info.is_builtin_function {
        UnlinkedFunctionKind::Builtin
    } else {
        UnlinkedFunctionKind::Normal
    };
    let construct_ability = if executable_info.is_constructor {
        ConstructAbility::CanConstruct
    } else {
        ConstructAbility::CannotConstruct
    };

    UnlinkedFunctionExecutable {
        name_hint: None,
        ecma_name: None,
        executable_info,
        source: call_code.source().clone(),
        function_source: Default::default(),
        body_source: Default::default(),
        parameters_start: None,
        parameter_count_excluding_this,
        line_count: 0,
        function_mode,
        unlinked_kind,
        construct_ability,
        call_code: Some(call_code),
        construct_code: None,
        rare: UnlinkedFunctionExecutableRareData::default(),
    }
}

fn function_mode_for_executable_info(info: &crate::bytecode::ExecutableInfo) -> FunctionMode {
    match info.parse_mode {
        Some(ParseMode::GeneratorBody) => FunctionMode::Generator,
        Some(ParseMode::AsyncFunctionBody) => FunctionMode::Async,
        Some(ParseMode::AsyncGeneratorBody) => FunctionMode::AsyncGenerator,
        Some(ParseMode::Method | ParseMode::Getter | ParseMode::Setter) => FunctionMode::Method,
        _ if info.is_class_context || info.is_builtin_default_class_constructor => {
            FunctionMode::ClassConstructor
        }
        _ => FunctionMode::Normal,
    }
}

fn code_specialization_for_kind(kind: CodeSpecializationKind) -> CodeSpecialization {
    match kind {
        CodeSpecializationKind::Call => CodeSpecialization::Call,
        CodeSpecializationKind::Construct => CodeSpecialization::Construct,
    }
}

fn code_specialization_matches_install(
    specialization: CodeSpecialization,
    code_block: &CodeBlock,
) -> bool {
    let actual = code_block.link_context().specialization;
    actual == specialization
        || (actual == CodeSpecialization::None && specialization == CodeSpecialization::None)
}

fn install_interpreter_entrypoint(
    entrypoints: &mut ExecutableEntrypoints,
    specialization: CodeSpecialization,
    code_block: &CodeBlock,
) {
    let Some(slot) = code_block.entrypoints().interpreter else {
        return;
    };
    let slot = Some(InterpreterEntrypointSlot(slot.0));
    match specialization {
        CodeSpecialization::Call | CodeSpecialization::None => {
            entrypoints.call.interpreter = slot;
        }
        CodeSpecialization::Construct => {
            entrypoints.construct.interpreter = slot;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::instruction::InstructionBuilder;
    use crate::bytecode::{
        CodeBlockEntrypoints, CodeBlockLifecycleState, CodeKind, InterpreterEntrySlot, LinkContext,
        Opcode, OperandWidth,
    };
    use crate::gc::CellId;

    #[test]
    fn registry_installs_top_level_script_code_with_interpreter_entry() {
        let executable = ExecutableId(CellId(10));
        let code_block_id = CodeBlockId(CellId(11));
        let unlinked = Arc::new(UnlinkedCodeBlock::new(
            CodeKind::Program,
            empty_instruction_stream(),
        ));
        let code_block = code_block_for(executable, CodeSpecialization::None, &unlinked)
            .with_entrypoints(CodeBlockEntrypoints {
                interpreter: Some(InterpreterEntrySlot(7)),
                ..CodeBlockEntrypoints::default()
            })
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter);
        let mut registry = ExecutableRegistry::default();

        registry
            .register_script(executable, ScriptExecutableKind::Program, unlinked)
            .expect("script executable registered");
        let install = registry.install_script_code(
            executable,
            code_block_id,
            CodeSpecialization::None,
            &code_block,
        );

        assert_eq!(
            install.outcome,
            ExecutableInstallOutcome::Installed {
                installed: InstalledExecutableCode {
                    code_block: code_block_id,
                    specialization: CodeSpecialization::None,
                }
            }
        );
        let record = registry.script(executable).expect("script record");
        assert_eq!(
            record.installed_code(),
            Some(InstalledExecutableCode {
                code_block: code_block_id,
                specialization: CodeSpecialization::None,
            })
        );
        assert_eq!(
            record.executable().base.entrypoints.call.interpreter,
            Some(InterpreterEntrypointSlot(7))
        );
        assert_eq!(
            registry.executable_for_code_block(code_block_id),
            Some(executable)
        );
    }

    #[test]
    fn registry_rejects_install_for_mismatched_executable_identity() {
        let executable = ExecutableId(CellId(20));
        let wrong_executable = ExecutableId(CellId(21));
        let code_block_id = CodeBlockId(CellId(22));
        let unlinked = Arc::new(UnlinkedCodeBlock::new(
            CodeKind::Program,
            empty_instruction_stream(),
        ));
        let code_block = code_block_for(wrong_executable, CodeSpecialization::None, &unlinked);
        let mut registry = ExecutableRegistry::default();

        registry
            .register_script(executable, ScriptExecutableKind::Program, unlinked)
            .expect("script executable registered");
        let install = registry.install_script_code(
            executable,
            code_block_id,
            CodeSpecialization::None,
            &code_block,
        );

        assert_eq!(
            install.outcome,
            ExecutableInstallOutcome::Rejected(
                ExecutableInstallRejection::CodeBlockExecutableMismatch {
                    expected: executable,
                    actual: Some(wrong_executable),
                }
            )
        );
        assert_eq!(registry.executable_for_code_block(code_block_id), None);
        assert!(registry
            .script(executable)
            .expect("script record")
            .installed_code()
            .is_none());
    }

    #[test]
    fn registry_installs_function_call_code_with_executable_entrypoint() {
        let executable = ExecutableId(CellId(30));
        let code_block_id = CodeBlockId(CellId(31));
        let unlinked = Arc::new(UnlinkedCodeBlock::new(
            CodeKind::Function,
            empty_instruction_stream(),
        ));
        let code_block = code_block_for(executable, CodeSpecialization::Call, &unlinked)
            .with_entrypoints(CodeBlockEntrypoints {
                interpreter: Some(InterpreterEntrySlot(3)),
                ..CodeBlockEntrypoints::default()
            })
            .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter);
        let mut registry = ExecutableRegistry::default();

        registry
            .register_function(executable, unlinked)
            .expect("function executable registered");
        let install = registry.install_function_code(
            executable,
            code_block_id,
            CodeSpecialization::Call,
            &code_block,
        );

        assert_eq!(
            install.outcome,
            ExecutableInstallOutcome::Installed {
                installed: InstalledExecutableCode {
                    code_block: code_block_id,
                    specialization: CodeSpecialization::Call,
                }
            }
        );
        let record = registry.function(executable).expect("function record");
        assert_eq!(
            record.installed_call_code(),
            Some(InstalledExecutableCode {
                code_block: code_block_id,
                specialization: CodeSpecialization::Call,
            })
        );
        assert!(record.installed_construct_code().is_none());
        assert!(record.executable().call_code.is_some());
        assert!(record.executable().construct_code.is_none());
        assert_eq!(
            record.executable().script.base.entrypoints.call.interpreter,
            Some(InterpreterEntrypointSlot(3))
        );
        assert_eq!(
            registry.executable_for_code_block(code_block_id),
            Some(executable)
        );
    }

    fn code_block_for(
        executable: ExecutableId,
        specialization: CodeSpecialization,
        unlinked: &Arc<UnlinkedCodeBlock>,
    ) -> CodeBlock {
        CodeBlock::from_shared_unlinked(
            unlinked.clone(),
            LinkContext {
                owner_executable: Some(executable),
                specialization,
                ..LinkContext::default()
            },
        )
    }

    fn empty_instruction_stream() -> crate::bytecode::PackedInstructionStream {
        let mut builder = InstructionBuilder::new();
        builder.declare_instruction(Opcode::Reserved, OperandWidth::Narrow, Vec::new());
        builder.finalize()
    }
}
