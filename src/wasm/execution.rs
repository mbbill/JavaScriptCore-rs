//! WebAssembly execution boundary records.
//!
//! These descriptors name host-call, import, export, trap, result, and
//! instance-entry boundaries. They do not decode Wasm bytes, compile functions,
//! allocate instances, or run a Wasm interpreter.

use crate::gc::{RootKind, RootSetMutationAuthority};
use crate::runtime::{HostHookId, ObjectId};
use crate::wasm::{
    describe_wasm_link_state_semantics, describe_wasm_module_linking_semantics, BridgeEntrypoint,
    BridgeExceptionPolicy, JsToWasmBridge, WasmExportIndex, WasmExportKind, WasmFunctionIndex,
    WasmFunctionSignature, WasmImportIndex, WasmImportKind, WasmInstanceId, WasmLinkState,
    WasmLinkStateSemanticDescriptor, WasmModuleId, WasmModuleInfo, WasmModuleValidationError,
    WasmToJsBridge, WasmTypeSignatureIndex, WasmValueType,
};

/// Boundary value slot used for arity and type-shape validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmBoundaryValueSlot {
    pub value_type: WasmValueType,
    pub slot: u32,
}

/// WebAssembly call boundary family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCallBoundaryKind {
    HostCall,
    Import,
    Export,
    StartFunction,
    InstanceEntry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmRootBoundaryKind {
    InstanceObject,
    ModuleObject,
    TableObject,
    MemoryObject,
    JsWrapper,
    ImportedCallee,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmRootBoundaryRecord {
    pub kind: WasmRootBoundaryKind,
    pub root_kind: RootKind,
    pub mutation_authority: RootSetMutationAuthority,
    pub object: Option<ObjectId>,
    pub precise: bool,
}

/// Host call descriptor for runtime services called from Wasm code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmHostCallInvocationDescriptor {
    pub instance: WasmInstanceId,
    pub module: WasmModuleId,
    pub function: Option<WasmFunctionIndex>,
    pub host_hook: HostHookId,
    pub entry: BridgeEntrypoint,
    pub signature: WasmFunctionSignature,
    pub arguments: Vec<WasmBoundaryValueSlot>,
    pub exception_policy: BridgeExceptionPolicy,
    pub root_boundaries: Vec<WasmRootBoundaryRecord>,
}

/// Import invocation descriptor for Wasm calling a linked JS or host target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmImportInvocationDescriptor {
    pub instance: WasmInstanceId,
    pub module: WasmModuleId,
    pub import: WasmImportIndex,
    pub function: WasmFunctionIndex,
    pub callee: Option<ObjectId>,
    pub host_hook: Option<HostHookId>,
    pub bridge: WasmToJsBridge,
    pub arguments: Vec<WasmBoundaryValueSlot>,
    pub root_boundaries: Vec<WasmRootBoundaryRecord>,
}

/// Export invocation descriptor for JS entering a Wasm function wrapper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmExportInvocationDescriptor {
    pub instance: WasmInstanceId,
    pub module: WasmModuleId,
    pub export: WasmExportIndex,
    pub function: WasmFunctionIndex,
    pub wrapper: Option<ObjectId>,
    pub bridge: JsToWasmBridge,
    pub arguments: Vec<WasmBoundaryValueSlot>,
    pub root_boundaries: Vec<WasmRootBoundaryRecord>,
}

/// Trap category crossing a Wasm execution boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmTrapKind {
    Unreachable,
    StackOverflow,
    OutOfBoundsMemory,
    OutOfBoundsTable,
    IndirectCallTypeMismatch,
    IntegerDivideByZero,
    InvalidConversionToInteger,
    UninitializedElement,
    HostException,
    RuntimeLimit,
}

/// Trap record returned through a boundary result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmTrapRecord {
    pub kind: WasmTrapKind,
    pub instance: WasmInstanceId,
    pub function: Option<WasmFunctionIndex>,
    pub bytecode_offset: Option<u32>,
    pub exception_policy: BridgeExceptionPolicy,
}

/// Boundary result status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmExecutionResultKind {
    Returned,
    Trapped,
}

/// Boundary result record for host/import/export calls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmExecutionResultRecord {
    pub boundary: WasmCallBoundaryKind,
    pub instance: WasmInstanceId,
    pub function: Option<WasmFunctionIndex>,
    pub status: WasmExecutionResultKind,
    pub results: Vec<WasmBoundaryValueSlot>,
    pub trap: Option<WasmTrapRecord>,
}

/// Instance entry category before any Wasm code is entered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmInstanceEntryKind {
    Constructor,
    StartFunction,
    ExportedFunction,
    ImportThunk,
    HostCallback,
}

/// Instance entry boundary record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmInstanceEntryBoundaryRecord {
    pub instance: WasmInstanceId,
    pub module: WasmModuleId,
    pub state: WasmLinkState,
    pub kind: WasmInstanceEntryKind,
    pub function: Option<WasmFunctionIndex>,
    pub type_signature: Option<WasmTypeSignatureIndex>,
}

/// Described instance entry boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmInstanceEntryBoundaryDescriptor {
    pub instance: WasmInstanceId,
    pub module: WasmModuleId,
    pub state: WasmLinkStateSemanticDescriptor,
    pub kind: WasmInstanceEntryKind,
    pub function: Option<WasmFunctionIndex>,
    pub type_signature: Option<WasmTypeSignatureIndex>,
    pub can_enter_runtime: bool,
}

/// Boundary validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WasmExecutionBoundaryError {
    Module(WasmModuleValidationError),
    ModuleMismatch {
        expected: WasmModuleId,
        actual: WasmModuleId,
    },
    UnknownImport(WasmImportIndex),
    UnknownExport(WasmExportIndex),
    ImportKindMismatch {
        index: WasmImportIndex,
        kind: WasmImportKind,
    },
    ExportKindMismatch {
        index: WasmExportIndex,
        kind: WasmExportKind,
    },
    FunctionMismatch {
        expected: WasmFunctionIndex,
        actual: WasmFunctionIndex,
    },
    ArgumentCountMismatch {
        expected: usize,
        actual: usize,
    },
    ArgumentTypeMismatch {
        index: usize,
        expected: WasmValueType,
        actual: WasmValueType,
    },
    ResultCountMismatch {
        expected: usize,
        actual: usize,
    },
    ResultTypeMismatch {
        index: usize,
        expected: WasmValueType,
        actual: WasmValueType,
    },
    ReturnedWithTrap,
    TrappedWithoutTrap,
    MissingEntryFunction(WasmInstanceEntryKind),
    RootBoundaryAuthorityMismatch {
        kind: WasmRootBoundaryKind,
        root_kind: RootKind,
        authority: RootSetMutationAuthority,
    },
}

pub fn describe_wasm_host_call_invocation(
    descriptor: &WasmHostCallInvocationDescriptor,
) -> Result<WasmCallBoundaryKind, WasmExecutionBoundaryError> {
    validate_value_slots(&descriptor.signature.params, &descriptor.arguments)?;
    validate_wasm_root_boundaries(&descriptor.root_boundaries)?;
    Ok(WasmCallBoundaryKind::HostCall)
}

pub fn describe_wasm_import_invocation(
    module: &WasmModuleInfo,
    descriptor: &WasmImportInvocationDescriptor,
) -> Result<WasmCallBoundaryKind, WasmExecutionBoundaryError> {
    if module.id != descriptor.module {
        return Err(WasmExecutionBoundaryError::ModuleMismatch {
            expected: module.id,
            actual: descriptor.module,
        });
    }
    describe_wasm_module_linking_semantics(module).map_err(WasmExecutionBoundaryError::Module)?;
    let import = module
        .imports
        .iter()
        .find(|import| import.index == descriptor.import)
        .ok_or(WasmExecutionBoundaryError::UnknownImport(descriptor.import))?;
    if import.kind != WasmImportKind::Function {
        return Err(WasmExecutionBoundaryError::ImportKindMismatch {
            index: import.index,
            kind: import.kind,
        });
    }
    if import.function != Some(descriptor.function) {
        return Err(WasmExecutionBoundaryError::FunctionMismatch {
            expected: import.function.unwrap_or(descriptor.function),
            actual: descriptor.function,
        });
    }
    validate_value_slots(&descriptor.bridge.signature.params, &descriptor.arguments)?;
    validate_wasm_root_boundaries(&descriptor.root_boundaries)?;
    Ok(WasmCallBoundaryKind::Import)
}

pub fn describe_wasm_export_invocation(
    module: &WasmModuleInfo,
    descriptor: &WasmExportInvocationDescriptor,
) -> Result<WasmCallBoundaryKind, WasmExecutionBoundaryError> {
    if module.id != descriptor.module {
        return Err(WasmExecutionBoundaryError::ModuleMismatch {
            expected: module.id,
            actual: descriptor.module,
        });
    }
    describe_wasm_module_linking_semantics(module).map_err(WasmExecutionBoundaryError::Module)?;
    let export = module
        .exports
        .iter()
        .find(|export| export.index == descriptor.export)
        .ok_or(WasmExecutionBoundaryError::UnknownExport(descriptor.export))?;
    if export.kind != WasmExportKind::Function {
        return Err(WasmExecutionBoundaryError::ExportKindMismatch {
            index: export.index,
            kind: export.kind,
        });
    }
    if export.function != Some(descriptor.function) {
        return Err(WasmExecutionBoundaryError::FunctionMismatch {
            expected: export.function.unwrap_or(descriptor.function),
            actual: descriptor.function,
        });
    }
    validate_value_slots(&descriptor.bridge.signature.params, &descriptor.arguments)?;
    validate_wasm_root_boundaries(&descriptor.root_boundaries)?;
    Ok(WasmCallBoundaryKind::Export)
}

pub fn describe_wasm_execution_result(
    record: &WasmExecutionResultRecord,
    signature: &WasmFunctionSignature,
) -> Result<WasmExecutionResultKind, WasmExecutionBoundaryError> {
    match (record.status, record.trap) {
        (WasmExecutionResultKind::Returned, Some(_)) => {
            return Err(WasmExecutionBoundaryError::ReturnedWithTrap);
        }
        (WasmExecutionResultKind::Trapped, None) => {
            return Err(WasmExecutionBoundaryError::TrappedWithoutTrap);
        }
        _ => {}
    }
    if record.status == WasmExecutionResultKind::Returned {
        validate_result_slots(&signature.results, &record.results)?;
    }
    Ok(record.status)
}

pub fn describe_wasm_instance_entry_boundary(
    record: WasmInstanceEntryBoundaryRecord,
) -> Result<WasmInstanceEntryBoundaryDescriptor, WasmExecutionBoundaryError> {
    if matches!(
        record.kind,
        WasmInstanceEntryKind::StartFunction
            | WasmInstanceEntryKind::ExportedFunction
            | WasmInstanceEntryKind::ImportThunk
    ) && record.function.is_none()
    {
        return Err(WasmExecutionBoundaryError::MissingEntryFunction(
            record.kind,
        ));
    }

    let state = describe_wasm_link_state_semantics(record.state);
    let can_enter_runtime = match record.kind {
        WasmInstanceEntryKind::Constructor => state.runtime_objects_available,
        WasmInstanceEntryKind::StartFunction => state.runtime_objects_available,
        WasmInstanceEntryKind::ExportedFunction => state.exports_available,
        WasmInstanceEntryKind::ImportThunk | WasmInstanceEntryKind::HostCallback => {
            state.imports_available
        }
    };

    Ok(WasmInstanceEntryBoundaryDescriptor {
        instance: record.instance,
        module: record.module,
        state,
        kind: record.kind,
        function: record.function,
        type_signature: record.type_signature,
        can_enter_runtime,
    })
}

fn validate_value_slots(
    expected: &[WasmValueType],
    actual: &[WasmBoundaryValueSlot],
) -> Result<(), WasmExecutionBoundaryError> {
    if expected.len() != actual.len() {
        return Err(WasmExecutionBoundaryError::ArgumentCountMismatch {
            expected: expected.len(),
            actual: actual.len(),
        });
    }
    for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
        if *expected != actual.value_type {
            return Err(WasmExecutionBoundaryError::ArgumentTypeMismatch {
                index,
                expected: *expected,
                actual: actual.value_type,
            });
        }
    }
    Ok(())
}

fn validate_result_slots(
    expected: &[WasmValueType],
    actual: &[WasmBoundaryValueSlot],
) -> Result<(), WasmExecutionBoundaryError> {
    if expected.len() != actual.len() {
        return Err(WasmExecutionBoundaryError::ResultCountMismatch {
            expected: expected.len(),
            actual: actual.len(),
        });
    }
    for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
        if *expected != actual.value_type {
            return Err(WasmExecutionBoundaryError::ResultTypeMismatch {
                index,
                expected: *expected,
                actual: actual.value_type,
            });
        }
    }
    Ok(())
}

fn validate_wasm_root_boundaries(
    boundaries: &[WasmRootBoundaryRecord],
) -> Result<(), WasmExecutionBoundaryError> {
    for boundary in boundaries {
        if !wasm_root_boundary_authority_is_valid(boundary.root_kind, boundary.mutation_authority) {
            return Err(WasmExecutionBoundaryError::RootBoundaryAuthorityMismatch {
                kind: boundary.kind,
                root_kind: boundary.root_kind,
                authority: boundary.mutation_authority,
            });
        }
    }
    Ok(())
}

const fn wasm_root_boundary_authority_is_valid(
    root_kind: RootKind,
    authority: RootSetMutationAuthority,
) -> bool {
    matches!(
        (root_kind, authority),
        (
            RootKind::VMRegister,
            RootSetMutationAuthority::VmRegisterFile
        ) | (
            RootKind::ExplicitRoot,
            RootSetMutationAuthority::ExplicitRootRegistry
        ) | (RootKind::Handle, RootSetMutationAuthority::HandleScope)
            | (RootKind::Host, RootSetMutationAuthority::HostIntegration)
            | (RootKind::JitCode, RootSetMutationAuthority::JitCodeRegistry)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::EntryAbi;
    use crate::wasm::{
        WasmFunctionCodeIndex, WasmFunctionTypeDescriptor, WasmModuleInfoBuilder, WasmSourceKind,
        WasmTypeDescriptor, WasmTypeKind, WasmValidationState,
    };

    fn signature() -> WasmFunctionSignature {
        WasmFunctionSignature {
            params: vec![WasmValueType::I32],
            results: vec![WasmValueType::I64],
            module_type_index: Some(0),
            canonical_type_index: Some(WasmTypeSignatureIndex(0)),
        }
    }

    fn entry() -> BridgeEntrypoint {
        BridgeEntrypoint {
            entry_slot: Some(0),
            boundary: None,
            entrypoint: None,
            code: None,
            abi: EntryAbi::Wasm,
        }
    }

    fn module() -> WasmModuleInfo {
        let type_index = WasmTypeSignatureIndex(0);
        WasmModuleInfoBuilder::new(WasmModuleId(3), WasmSourceKind::BinaryModule)
            .type_descriptor(WasmTypeDescriptor {
                index: type_index,
                kind: WasmTypeKind::Function,
                function: Some(WasmFunctionTypeDescriptor {
                    index: type_index,
                    params: vec![WasmValueType::I32],
                    results: vec![WasmValueType::I64],
                    supertype: None,
                    is_final: true,
                }),
                struct_type: None,
                array_type: None,
                recursive_group: None,
            })
            .function_signature(signature())
            .function_summary(crate::wasm::WasmFunctionValidationSummary {
                code_index: WasmFunctionCodeIndex(0),
                start_offset: 0,
                end_offset: 4,
                finished_validating: true,
                uses_simd: false,
                uses_exceptions: false,
                uses_atomics: false,
                declared: true,
                referenced: true,
            })
            .export(crate::wasm::WasmExportDescriptor {
                index: WasmExportIndex(0),
                kind: WasmExportKind::Function,
                name: Some(9),
                function: Some(WasmFunctionIndex(0)),
                memory: None,
                table: None,
                global: None,
                tag: None,
                type_signature: Some(type_index),
            })
            .function_counts(0, 1)
            .validation_state(WasmValidationState::Complete)
            .build()
            .unwrap()
    }

    #[test]
    fn wasm_export_invocation_uses_module_linking_validation() {
        let module = module();
        let descriptor = WasmExportInvocationDescriptor {
            instance: WasmInstanceId(11),
            module: WasmModuleId(3),
            export: WasmExportIndex(0),
            function: WasmFunctionIndex(0),
            wrapper: None,
            bridge: JsToWasmBridge {
                abi: crate::wasm::BridgeAbi::JsToWasm,
                module: WasmModuleId(3),
                function: WasmFunctionIndex(0),
                entry: entry(),
                signature: signature(),
                conversion: crate::wasm::BridgeConversionPolicy::ExportWrapper,
            },
            arguments: vec![WasmBoundaryValueSlot {
                value_type: WasmValueType::I32,
                slot: 0,
            }],
            root_boundaries: vec![WasmRootBoundaryRecord {
                kind: WasmRootBoundaryKind::InstanceObject,
                root_kind: RootKind::VMRegister,
                mutation_authority: RootSetMutationAuthority::VmRegisterFile,
                object: None,
                precise: true,
            }],
        };

        assert_eq!(
            describe_wasm_export_invocation(&module, &descriptor).unwrap(),
            WasmCallBoundaryKind::Export
        );
    }

    #[test]
    fn wasm_result_records_validate_return_arity_without_execution() {
        let record = WasmExecutionResultRecord {
            boundary: WasmCallBoundaryKind::Export,
            instance: WasmInstanceId(11),
            function: Some(WasmFunctionIndex(0)),
            status: WasmExecutionResultKind::Returned,
            results: vec![WasmBoundaryValueSlot {
                value_type: WasmValueType::I64,
                slot: 0,
            }],
            trap: None,
        };

        assert_eq!(
            describe_wasm_execution_result(&record, &signature()).unwrap(),
            WasmExecutionResultKind::Returned
        );
    }

    #[test]
    fn wasm_trap_result_requires_trap_record() {
        let record = WasmExecutionResultRecord {
            boundary: WasmCallBoundaryKind::HostCall,
            instance: WasmInstanceId(1),
            function: None,
            status: WasmExecutionResultKind::Trapped,
            results: Vec::new(),
            trap: None,
        };

        assert_eq!(
            describe_wasm_execution_result(&record, &signature()).unwrap_err(),
            WasmExecutionBoundaryError::TrappedWithoutTrap
        );
    }

    #[test]
    fn wasm_instance_entry_reports_link_state_gate() {
        let descriptor = describe_wasm_instance_entry_boundary(WasmInstanceEntryBoundaryRecord {
            instance: WasmInstanceId(4),
            module: WasmModuleId(5),
            state: WasmLinkState::Linked,
            kind: WasmInstanceEntryKind::ExportedFunction,
            function: Some(WasmFunctionIndex(0)),
            type_signature: Some(WasmTypeSignatureIndex(0)),
        })
        .unwrap();

        assert!(descriptor.can_enter_runtime);
        assert!(descriptor.state.exports_available);
    }
}
