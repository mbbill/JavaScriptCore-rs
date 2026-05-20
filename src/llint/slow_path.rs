use crate::bytecode::code_block::{BytecodeIndex, RuntimeSlot};
use crate::bytecode::ic::CallLinkInfo;
use crate::bytecode::opcode::Opcode;
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;
use crate::jit::CallBoundaryId;

/// Declarative registry of LLInt slow paths referenced from generated code.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntSlowPathRegistry {
    pub paths: Vec<LLIntSlowPath>,
    pub helpers: Vec<LLIntHelperPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LLIntSlowPathValidationError {
    EmptySymbol,
    EmptyProvenance(&'static str),
    DuplicatePathId(LLIntSlowPathId),
    DuplicatePathSymbol(&'static str),
    EmptyParameters(&'static str),
    CallLinkPolicyMissingParameter(&'static str),
    NoReturnHelper,
    CallSiteLinkMismatch,
    BoundaryPathMismatch,
    BoundaryResultMismatch,
    BoundaryResumeMismatch,
}

impl LLIntSlowPathRegistry {
    pub fn from_static(registry: StaticLLIntSlowPathRegistry) -> Self {
        Self {
            paths: registry
                .paths()
                .iter()
                .map(|schema| LLIntSlowPath {
                    id: schema.id,
                    symbol: schema.symbol,
                    kind: schema.kind,
                    signature: LLIntSlowPathSignature {
                        parameters: schema.parameters.to_vec(),
                        result: schema.result,
                        abi: schema.abi,
                    },
                    origin_policy: schema.origin_policy,
                })
                .collect(),
            helpers: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), LLIntSlowPathValidationError> {
        for (index, path) in self.paths.iter().enumerate() {
            path.validate()?;
            if self.paths[index + 1..]
                .iter()
                .any(|other| other.id == path.id)
            {
                return Err(LLIntSlowPathValidationError::DuplicatePathId(path.id));
            }
            if self.paths[index + 1..]
                .iter()
                .any(|other| other.symbol == path.symbol)
            {
                return Err(LLIntSlowPathValidationError::DuplicatePathSymbol(
                    path.symbol,
                ));
            }
        }

        for helper in &self.helpers {
            if helper.symbol.is_empty() {
                return Err(LLIntSlowPathValidationError::EmptySymbol);
            }
            if helper.signature.result == LLIntSlowPathResult::NoReturn {
                return Err(LLIntSlowPathValidationError::NoReturnHelper);
            }
        }

        Ok(())
    }

    pub fn select_by_kinds(
        &self,
        kinds: &[LLIntSlowPathKind],
    ) -> Result<Vec<LLIntSlowPath>, LLIntSlowPathValidationError> {
        self.validate()?;
        let mut selected = Vec::new();
        for kind in kinds {
            if let Some(path) = self.paths.iter().find(|path| path.kind == *kind) {
                selected.push(path.clone());
            }
        }
        Ok(selected)
    }
}

impl LLIntSlowPath {
    pub fn validate(&self) -> Result<(), LLIntSlowPathValidationError> {
        if self.symbol.is_empty() {
            return Err(LLIntSlowPathValidationError::EmptySymbol);
        }
        validate_slow_path_signature(self.symbol, &self.signature.parameters, self.origin_policy)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntSlowPath {
    pub id: LLIntSlowPathId,
    pub symbol: &'static str,
    pub kind: LLIntSlowPathKind,
    pub signature: LLIntSlowPathSignature,
    pub origin_policy: SlowPathOriginPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct LLIntSlowPathId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathKind {
    Trace,
    EntryOsr,
    LoopOsr,
    Replacement,
    ObjectAllocation,
    ArrayAllocation,
    RegExpAllocation,
    PropertyAccess,
    PrivateName,
    PrivateBrand,
    Iterator,
    Branch,
    Compare,
    Switch,
    FunctionAllocation,
    VarargsFrame,
    Call,
    DirectEval,
    ArgumentsObject,
    StringConcat,
    Conversion,
    Throw,
    Trap,
    Debug,
    Exception,
    Scope,
    CatchProfile,
    ShadowChicken,
    OutOfLineJumpTarget,
    ArityCheck,
    CheckpointOsrExit,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntSlowPathSignature {
    pub parameters: Vec<LLIntSlowPathParameter>,
    pub result: LLIntSlowPathResult,
    pub abi: LLIntAbi,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathParameter {
    CallFrame,
    ProgramCounter,
    Vm,
    ProtoCallFrame,
    NewStackPointer,
    EncodedValue,
    VirtualRegister(VirtualRegister),
    OperandIndex(i32),
    CallLinkInfo,
    Cell,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathResult {
    #[default]
    UGeneralPurposePair,
    Void,
    NoReturn,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntAbi {
    #[default]
    SysV,
    CLoop,
    PlatformDefault,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum SlowPathOriginPolicy {
    #[default]
    CurrentBytecode,
    CurrentCheckpoint,
    CallLink(CodeOrigin),
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntHelperPath {
    pub symbol: &'static str,
    pub purpose: LLIntHelperPurpose,
    pub signature: LLIntSlowPathSignature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntHelperPurpose {
    TraceOperand,
    TraceValue,
    DefaultCall,
    VirtualCall,
    PolymorphicCall,
    WriteBarrier,
    StackCheck,
    VmEntryPermission,
    Crash,
}

/// Metadata passed by generated code when a slow path is entered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntSlowPathCallSite {
    pub bytecode_index: BytecodeIndex,
    pub opcode: Opcode,
    pub origin: CodeOrigin,
    pub metadata_slot: Option<RuntimeSlot>,
    pub call_link_info: Option<CallLinkInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntSlowPathBoundaryRecord {
    pub boundary: CallBoundaryId,
    pub path_id: LLIntSlowPathId,
    pub kind: LLIntSlowPathBoundaryKind,
    pub call_site: LLIntSlowPathCallSite,
    pub result: LLIntSlowPathResult,
    pub resume: LLIntSlowPathResume,
    pub preserves_current_pc: bool,
    pub may_throw: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathBoundaryKind {
    RuntimeHelper,
    CallLink,
    DirectEval,
    VarargsCall,
    Exception,
    CheckpointExit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathResume {
    ContinueAtCurrentBytecode,
    ReturnToCaller,
    EnterCalleeFrame,
    Throw,
    FallbackToInterpreter,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathSchemaOwner {
    #[default]
    OfflineAsmGeneratedData,
    LLIntSlowPathRegistry,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    LinkTimeInstallation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticLLIntSlowPathSchema {
    pub id: LLIntSlowPathId,
    pub symbol: &'static str,
    pub kind: LLIntSlowPathKind,
    pub parameters: &'static [LLIntSlowPathParameter],
    pub result: LLIntSlowPathResult,
    pub abi: LLIntAbi,
    pub origin_policy: SlowPathOriginPolicy,
    pub owner: LLIntSlowPathSchemaOwner,
    pub mutation_authority: LLIntSlowPathRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticLLIntSlowPathRegistry {
    pub paths: &'static [StaticLLIntSlowPathSchema],
}

impl StaticLLIntSlowPathRegistry {
    pub const fn new(paths: &'static [StaticLLIntSlowPathSchema]) -> Self {
        Self { paths }
    }

    pub const fn paths(self) -> &'static [StaticLLIntSlowPathSchema] {
        self.paths
    }

    pub fn path_for_symbol(self, symbol: &str) -> Option<&'static StaticLLIntSlowPathSchema> {
        self.paths.iter().find(|path| path.symbol == symbol)
    }

    pub fn validate(self) -> Result<(), LLIntSlowPathValidationError> {
        for (index, path) in self.paths.iter().enumerate() {
            path.validate()?;
            if self.paths[index + 1..]
                .iter()
                .any(|other| other.id == path.id)
            {
                return Err(LLIntSlowPathValidationError::DuplicatePathId(path.id));
            }
            if self.paths[index + 1..]
                .iter()
                .any(|other| other.symbol == path.symbol)
            {
                return Err(LLIntSlowPathValidationError::DuplicatePathSymbol(
                    path.symbol,
                ));
            }
        }

        Ok(())
    }
}

impl StaticLLIntSlowPathSchema {
    pub fn validate(&self) -> Result<(), LLIntSlowPathValidationError> {
        if self.symbol.is_empty() {
            return Err(LLIntSlowPathValidationError::EmptySymbol);
        }
        if self.provenance.is_empty() {
            return Err(LLIntSlowPathValidationError::EmptyProvenance(self.symbol));
        }
        validate_slow_path_signature(self.symbol, self.parameters, self.origin_policy)
    }
}

impl LLIntSlowPathCallSite {
    pub fn validate_for_path(
        &self,
        path: &LLIntSlowPath,
    ) -> Result<(), LLIntSlowPathValidationError> {
        let needs_call_link = path
            .signature
            .parameters
            .contains(&LLIntSlowPathParameter::CallLinkInfo);
        if needs_call_link != self.call_link_info.is_some() {
            return Err(LLIntSlowPathValidationError::CallSiteLinkMismatch);
        }

        Ok(())
    }
}

impl LLIntSlowPathBoundaryRecord {
    pub fn for_path(
        boundary: CallBoundaryId,
        path: &LLIntSlowPath,
        call_site: LLIntSlowPathCallSite,
        kind: LLIntSlowPathBoundaryKind,
        resume: LLIntSlowPathResume,
    ) -> Result<Self, LLIntSlowPathValidationError> {
        call_site.validate_for_path(path)?;
        let record = Self {
            boundary,
            path_id: path.id,
            kind,
            call_site,
            result: path.signature.result,
            resume,
            preserves_current_pc: matches!(
                resume,
                LLIntSlowPathResume::ContinueAtCurrentBytecode
                    | LLIntSlowPathResume::FallbackToInterpreter
            ),
            may_throw: matches!(
                path.kind,
                LLIntSlowPathKind::Call
                    | LLIntSlowPathKind::DirectEval
                    | LLIntSlowPathKind::Throw
                    | LLIntSlowPathKind::Exception
                    | LLIntSlowPathKind::Trap
            ),
        };
        record.validate_for_path(path)?;
        Ok(record)
    }

    pub fn validate_for_path(
        &self,
        path: &LLIntSlowPath,
    ) -> Result<(), LLIntSlowPathValidationError> {
        if self.path_id != path.id {
            return Err(LLIntSlowPathValidationError::BoundaryPathMismatch);
        }
        self.call_site.validate_for_path(path)?;
        if self.result != path.signature.result {
            return Err(LLIntSlowPathValidationError::BoundaryResultMismatch);
        }
        match (self.result, self.resume) {
            (LLIntSlowPathResult::NoReturn, LLIntSlowPathResume::Throw) => Ok(()),
            (LLIntSlowPathResult::NoReturn, _) => {
                Err(LLIntSlowPathValidationError::BoundaryResumeMismatch)
            }
            (_, LLIntSlowPathResume::Throw) => {
                Err(LLIntSlowPathValidationError::BoundaryResumeMismatch)
            }
            (LLIntSlowPathResult::UGeneralPurposePair, LLIntSlowPathResume::EnterCalleeFrame)
            | (LLIntSlowPathResult::UGeneralPurposePair, LLIntSlowPathResume::ReturnToCaller)
            | (
                LLIntSlowPathResult::UGeneralPurposePair,
                LLIntSlowPathResume::FallbackToInterpreter,
            )
            | (LLIntSlowPathResult::Void, LLIntSlowPathResume::ContinueAtCurrentBytecode)
            | (LLIntSlowPathResult::Void, LLIntSlowPathResume::FallbackToInterpreter) => Ok(()),
            _ => Err(LLIntSlowPathValidationError::BoundaryResumeMismatch),
        }
    }
}

fn validate_slow_path_signature(
    symbol: &'static str,
    parameters: &[LLIntSlowPathParameter],
    origin_policy: SlowPathOriginPolicy,
) -> Result<(), LLIntSlowPathValidationError> {
    if parameters.is_empty() {
        return Err(LLIntSlowPathValidationError::EmptyParameters(symbol));
    }
    if matches!(origin_policy, SlowPathOriginPolicy::CallLink(_))
        && !parameters.contains(&LLIntSlowPathParameter::CallLinkInfo)
    {
        return Err(LLIntSlowPathValidationError::CallLinkPolicyMissingParameter(symbol));
    }

    Ok(())
}

const CALL_FRAME_PC_PARAMS: &[LLIntSlowPathParameter] = &[
    LLIntSlowPathParameter::CallFrame,
    LLIntSlowPathParameter::ProgramCounter,
];
const CALL_FRAME_VM_PARAMS: &[LLIntSlowPathParameter] = &[
    LLIntSlowPathParameter::CallFrame,
    LLIntSlowPathParameter::Vm,
];
const CALL_LINK_PARAMS: &[LLIntSlowPathParameter] = &[
    LLIntSlowPathParameter::CallFrame,
    LLIntSlowPathParameter::ProgramCounter,
    LLIntSlowPathParameter::CallLinkInfo,
];

pub const STATIC_LLINT_SLOW_PATH_SCHEMAS: &[StaticLLIntSlowPathSchema] = &[
    StaticLLIntSlowPathSchema {
        id: LLIntSlowPathId(0),
        symbol: "llint_slow_path_call",
        kind: LLIntSlowPathKind::Call,
        parameters: CALL_LINK_PARAMS,
        result: LLIntSlowPathResult::UGeneralPurposePair,
        abi: LLIntAbi::PlatformDefault,
        origin_policy: SlowPathOriginPolicy::CurrentBytecode,
        owner: LLIntSlowPathSchemaOwner::OfflineAsmGeneratedData,
        mutation_authority: LLIntSlowPathRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt slow-path schema",
    },
    StaticLLIntSlowPathSchema {
        id: LLIntSlowPathId(1),
        symbol: "llint_slow_path_throw",
        kind: LLIntSlowPathKind::Throw,
        parameters: CALL_FRAME_PC_PARAMS,
        result: LLIntSlowPathResult::NoReturn,
        abi: LLIntAbi::PlatformDefault,
        origin_policy: SlowPathOriginPolicy::CurrentBytecode,
        owner: LLIntSlowPathSchemaOwner::OfflineAsmGeneratedData,
        mutation_authority: LLIntSlowPathRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt slow-path schema",
    },
    StaticLLIntSlowPathSchema {
        id: LLIntSlowPathId(2),
        symbol: "llint_slow_path_stack_check",
        kind: LLIntSlowPathKind::Trap,
        parameters: CALL_FRAME_VM_PARAMS,
        result: LLIntSlowPathResult::Void,
        abi: LLIntAbi::PlatformDefault,
        origin_policy: SlowPathOriginPolicy::None,
        owner: LLIntSlowPathSchemaOwner::LLIntSlowPathRegistry,
        mutation_authority: LLIntSlowPathRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust LLInt slow-path schema",
    },
];

pub const STATIC_LLINT_SLOW_PATH_REGISTRY: StaticLLIntSlowPathRegistry =
    StaticLLIntSlowPathRegistry::new(STATIC_LLINT_SLOW_PATH_SCHEMAS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_slow_path_registry_validates() {
        assert_eq!(STATIC_LLINT_SLOW_PATH_REGISTRY.validate(), Ok(()));
        assert_eq!(
            LLIntSlowPathRegistry::from_static(STATIC_LLINT_SLOW_PATH_REGISTRY).validate(),
            Ok(())
        );
    }

    #[test]
    fn call_link_policy_requires_call_link_parameter() {
        let schema = StaticLLIntSlowPathSchema {
            id: LLIntSlowPathId(99),
            symbol: "bad_call_link",
            kind: LLIntSlowPathKind::Call,
            parameters: CALL_FRAME_PC_PARAMS,
            result: LLIntSlowPathResult::Void,
            abi: LLIntAbi::PlatformDefault,
            origin_policy: SlowPathOriginPolicy::CallLink(CodeOrigin::new(
                BytecodeIndex::from_offset(0),
            )),
            owner: LLIntSlowPathSchemaOwner::LLIntSlowPathRegistry,
            mutation_authority: LLIntSlowPathRegistryMutationAuthority::GeneratedStaticDataRefresh,
            provenance: "test",
        };

        assert_eq!(
            schema.validate(),
            Err(LLIntSlowPathValidationError::CallLinkPolicyMissingParameter("bad_call_link"))
        );
    }

    #[test]
    fn slow_path_selection_filters_static_registry_by_kind() {
        let registry = LLIntSlowPathRegistry::from_static(STATIC_LLINT_SLOW_PATH_REGISTRY);

        assert_eq!(
            registry
                .select_by_kinds(&[LLIntSlowPathKind::Call, LLIntSlowPathKind::Trap])
                .unwrap()
                .iter()
                .map(|path| path.symbol)
                .collect::<Vec<_>>(),
            vec!["llint_slow_path_call", "llint_slow_path_stack_check"]
        );
    }

    #[test]
    fn slow_path_boundary_records_interpreter_resume() {
        let registry = LLIntSlowPathRegistry::from_static(STATIC_LLINT_SLOW_PATH_REGISTRY);
        let path = registry
            .paths
            .iter()
            .find(|path| path.kind == LLIntSlowPathKind::Trap)
            .unwrap();
        let call_site = LLIntSlowPathCallSite {
            bytecode_index: BytecodeIndex::from_offset(16),
            opcode: Opcode::Reserved,
            origin: CodeOrigin::new(BytecodeIndex::from_offset(16)),
            metadata_slot: None,
            call_link_info: None,
        };

        let record = LLIntSlowPathBoundaryRecord::for_path(
            CallBoundaryId(4),
            path,
            call_site,
            LLIntSlowPathBoundaryKind::RuntimeHelper,
            LLIntSlowPathResume::ContinueAtCurrentBytecode,
        )
        .unwrap();

        assert_eq!(record.path_id, LLIntSlowPathId(2));
        assert!(record.preserves_current_pc);
        assert!(record.may_throw);
    }

    #[test]
    fn no_return_boundary_must_resume_by_throw() {
        let registry = LLIntSlowPathRegistry::from_static(STATIC_LLINT_SLOW_PATH_REGISTRY);
        let path = registry
            .paths
            .iter()
            .find(|path| path.kind == LLIntSlowPathKind::Throw)
            .unwrap();
        let call_site = LLIntSlowPathCallSite {
            bytecode_index: BytecodeIndex::from_offset(20),
            opcode: Opcode::Reserved,
            origin: CodeOrigin::new(BytecodeIndex::from_offset(20)),
            metadata_slot: None,
            call_link_info: None,
        };

        assert_eq!(
            LLIntSlowPathBoundaryRecord::for_path(
                CallBoundaryId(5),
                path,
                call_site,
                LLIntSlowPathBoundaryKind::Exception,
                LLIntSlowPathResume::ContinueAtCurrentBytecode
            ),
            Err(LLIntSlowPathValidationError::BoundaryResumeMismatch)
        );
    }
}
