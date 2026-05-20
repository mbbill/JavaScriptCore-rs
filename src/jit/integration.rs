//! Cross-tier compiler integration records.
//!
//! Wave I connects the existing tier descriptors without making generated code
//! executable. Every planned artifact here is either metadata-only or carries an
//! explicit fallback record that resumes interpreter semantics.

use crate::b3::{AirLoweringPlan, B3ProcedureId, B3ValidationError};
use crate::bytecode::{BytecodeIndex, CodeKind};
use crate::dfg::{DfgGraph, DfgGraphId, DfgValidationError};
use crate::domjit::{DomJitStructurePlan, DomJitValidationError};
use crate::ftl::{
    FtlArtifactDescriptor, FtlCompilationDescriptor, FtlValidationError, LoweringFailureReason,
};
use crate::interpreter::{InterpreterFrameRecord, InterpreterStackId};
use crate::jit::{
    CodeFinalizationAuthority, CodeInvalidationReason, CodeLiveness, CodeOrigin, CodeOriginKind,
    CodeOwnership, CompilationMode, CompilationOutcome, CompilationPlanId, CompilationProduct,
    CompilationRequest, CompilationRequestKind, EffectSummary, EntryAbi, Entrypoint,
    EntrypointKind, InlineCacheMissHandoffDescriptor, InlineCacheValidationError, JitCodeId,
    JitPlanValidationError, JitType, TierFallbackReason, TierFallbackResultRecord,
    TierFallbackSemantics, TierFallbackTarget, TieringSnapshot, TieringValidationError,
};
use crate::llint::{
    select_llint_entry_for_interpreter_frame, LLIntEntrypointTable, LLIntEntrypointValidationError,
    LLIntInterpreterEntryHandoff, LLIntInterpreterHandoffError, LLIntInterpreterHandoffReason,
};
use crate::offlineasm::{
    plan_offlineasm_symbolic_lowering, OfflineAsmBackend, OfflineAsmLoweringPlan,
    OfflineAsmProgramId, OfflineAsmSchemaRegistry, OfflineAsmValidationError,
};
use crate::runtime::{CodeBlockId, ExecutableId};

/// Compiler or tool boundary that contributed to an integration record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerIntegrationComponent {
    LlInt,
    OfflineAsm,
    BaselineJit,
    InlineCache,
    Dfg,
    B3,
    Ftl,
    DomJit,
    Disassembler,
    Lol,
}

/// Whether an artifact can be entered directly or must fall back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptimizedArtifactAvailability {
    DescriptorOnly,
    GeneratedCodeUnavailable,
    ReadyToInstallDescriptor,
}

/// Coarse semantic result used for differential records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DifferentialOutcome {
    NotObserved,
    ReturnedOrdinal(u32),
    ThrewOrdinal(u32),
    Terminated,
}

/// Interpreter-vs-optimized semantic comparison recorded before tier install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterpreterDifferentialRecord {
    pub owner: CodeBlockId,
    pub bytecode_index: BytecodeIndex,
    pub interpreter_outcome: DifferentialOutcome,
    pub optimized_outcome: DifferentialOutcome,
    pub effects: EffectSummary,
    pub fallback: TierFallbackResultRecord,
}

/// LLInt/offlineasm linkage for interpreter entry and generated thunk metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntOfflineAsmIntegrationRecord {
    pub owner: CodeBlockId,
    pub program: OfflineAsmProgramId,
    pub lowering: OfflineAsmLoweringPlan,
    pub entrypoints: LLIntEntrypointTable,
    pub handoff: Option<LLIntInterpreterEntryHandoff>,
    pub generated_code_available: bool,
}

/// Inputs required to connect LLInt entrypoint metadata to offlineasm lowering.
#[derive(Clone, Debug)]
pub struct LLIntOfflineAsmIntegrationRequest<'a> {
    pub owner: CodeBlockId,
    pub frame: Option<&'a InterpreterFrameRecord>,
    pub code_kind: CodeKind,
    pub construct_entry: bool,
    pub entrypoints: LLIntEntrypointTable,
    pub program: OfflineAsmProgramId,
    pub backend: OfflineAsmBackend,
    pub registry: OfflineAsmSchemaRegistry,
}

/// Descriptor-only artifact produced when code generation is unavailable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptimizedArtifactDescriptor {
    pub code: JitCodeId,
    pub tier: JitType,
    pub origin: CodeOrigin,
    pub availability: OptimizedArtifactAvailability,
    pub liveness: CodeLiveness,
    pub fallback: Option<TierFallbackResultRecord>,
}

/// Diagnostic attached to fallback or descriptor-only optimized tiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptimizedTierDiagnostic {
    pub component: CompilerIntegrationComponent,
    pub owner: CodeBlockId,
    pub plan: Option<CompilationPlanId>,
    pub tier: JitType,
    pub reason: TierFallbackReason,
    pub bytecode_index: Option<BytecodeIndex>,
    pub message_ordinal: u32,
}

/// Baseline-to-optimizing compiler integration record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptimizedTierIntegrationRecord {
    pub owner: CodeBlockId,
    pub executable: Option<ExecutableId>,
    pub request: CompilationRequest,
    pub tiering_snapshot: Option<TieringSnapshot>,
    pub llint: Option<LLIntOfflineAsmIntegrationRecord>,
    pub inline_cache_handoffs: Vec<InlineCacheMissHandoffDescriptor>,
    pub dfg_graph: Option<DfgGraphId>,
    pub b3_procedure: Option<B3ProcedureId>,
    pub air_lowering: Option<AirLoweringPlan>,
    pub ftl: Option<FtlCompilationDescriptor>,
    pub domjit_plans: Vec<DomJitStructurePlan>,
    pub artifact: OptimizedArtifactDescriptor,
    pub differential: InterpreterDifferentialRecord,
    pub diagnostics: Vec<OptimizedTierDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerIntegrationError {
    RequestInvalid(JitPlanValidationError),
    ProductInvalid(JitPlanValidationError),
    TierInvalid(TieringValidationError),
    LlIntEntrypointInvalid(LLIntEntrypointValidationError),
    LlIntHandoffInvalid(LLIntInterpreterHandoffError),
    OfflineAsmInvalid(OfflineAsmValidationError),
    InlineCacheInvalid(InlineCacheValidationError),
    DfgInvalid(DfgValidationError),
    B3Invalid(B3ValidationError),
    FtlInvalid(FtlValidationError),
    DomJitInvalid(DomJitValidationError),
    OwnerMismatch(CompilerIntegrationComponent),
    TierMismatch,
    MissingFallback,
    MissingDifferential,
    GeneratedCodeUnavailableWithoutFallback,
}

impl InterpreterDifferentialRecord {
    pub fn validate(&self) -> Result<(), CompilerIntegrationError> {
        self.fallback
            .validate()
            .map_err(CompilerIntegrationError::TierInvalid)?;
        if self.fallback.owner != self.owner
            || self.fallback.bytecode_index != Some(self.bytecode_index)
            || self.fallback.target != TierFallbackTarget::ReturnToInterpreter
        {
            return Err(CompilerIntegrationError::MissingDifferential);
        }
        if self.interpreter_outcome != self.optimized_outcome
            && self.fallback.reason != TierFallbackReason::UnsupportedTier
        {
            return Err(CompilerIntegrationError::MissingFallback);
        }
        Ok(())
    }
}

impl LLIntOfflineAsmIntegrationRecord {
    pub fn validate(&self) -> Result<(), CompilerIntegrationError> {
        self.entrypoints
            .validate_against(crate::llint::LLINT_ENTRYPOINT_SCHEMA_REGISTRY)
            .map_err(CompilerIntegrationError::LlIntEntrypointInvalid)?;
        self.lowering
            .validate_against(crate::offlineasm::OFFLINEASM_SCHEMA_REGISTRY)
            .map_err(CompilerIntegrationError::OfflineAsmInvalid)?;
        if self
            .handoff
            .is_some_and(|handoff| handoff.code_block != self.owner)
        {
            return Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::LlInt,
            ));
        }
        if !self.generated_code_available
            && self.entrypoints.install_state
                == crate::llint::LLIntEntrypointState::InstalledOnCodeBlock
        {
            return Err(CompilerIntegrationError::GeneratedCodeUnavailableWithoutFallback);
        }
        Ok(())
    }
}

impl OptimizedArtifactDescriptor {
    pub fn validate(&self) -> Result<(), CompilerIntegrationError> {
        if self.origin.owner.is_some_and(|owner| {
            self.fallback
                .as_ref()
                .is_some_and(|fallback| fallback.owner != owner)
        }) {
            return Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::BaselineJit,
            ));
        }
        if matches!(
            self.availability,
            OptimizedArtifactAvailability::DescriptorOnly
                | OptimizedArtifactAvailability::GeneratedCodeUnavailable
        ) && self.fallback.is_none()
        {
            return Err(CompilerIntegrationError::GeneratedCodeUnavailableWithoutFallback);
        }
        if matches!(
            self.availability,
            OptimizedArtifactAvailability::GeneratedCodeUnavailable
        ) && self.liveness != CodeLiveness::Unallocated
        {
            return Err(CompilerIntegrationError::GeneratedCodeUnavailableWithoutFallback);
        }
        if let Some(fallback) = &self.fallback {
            fallback
                .validate()
                .map_err(CompilerIntegrationError::TierInvalid)?;
        }
        Ok(())
    }
}

impl OptimizedTierDiagnostic {
    pub fn validate(&self) -> Result<(), CompilerIntegrationError> {
        if self.message_ordinal == 0 {
            return Err(CompilerIntegrationError::MissingFallback);
        }
        Ok(())
    }
}

impl OptimizedTierIntegrationRecord {
    pub fn attach_inline_cache_handoff(
        &mut self,
        handoff: InlineCacheMissHandoffDescriptor,
    ) -> Result<(), CompilerIntegrationError> {
        handoff
            .validate()
            .map_err(CompilerIntegrationError::InlineCacheInvalid)?;
        if handoff.owner != self.owner {
            return Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::InlineCache,
            ));
        }
        self.inline_cache_handoffs.push(handoff);
        Ok(())
    }

    pub fn attach_dfg_graph(&mut self, graph: &DfgGraph) -> Result<(), CompilerIntegrationError> {
        graph
            .validate()
            .map_err(CompilerIntegrationError::DfgInvalid)?;
        if graph.owner != self.owner {
            return Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::Dfg,
            ));
        }
        self.dfg_graph = Some(graph.id);
        Ok(())
    }

    pub fn attach_air_lowering(
        &mut self,
        lowering: AirLoweringPlan,
    ) -> Result<(), CompilerIntegrationError> {
        lowering
            .validate()
            .map_err(CompilerIntegrationError::B3Invalid)?;
        self.b3_procedure = lowering.source;
        self.air_lowering = Some(lowering);
        Ok(())
    }

    pub fn attach_ftl_compilation(
        &mut self,
        compilation: FtlCompilationDescriptor,
    ) -> Result<(), CompilerIntegrationError> {
        compilation
            .validate()
            .map_err(CompilerIntegrationError::FtlInvalid)?;
        if compilation.owner != self.owner {
            return Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::Ftl,
            ));
        }
        self.dfg_graph = Some(compilation.graph);
        self.b3_procedure = compilation.procedure;
        self.ftl = Some(compilation);
        Ok(())
    }

    pub fn attach_domjit_plan(
        &mut self,
        plan: DomJitStructurePlan,
    ) -> Result<(), CompilerIntegrationError> {
        plan.validate()
            .map_err(CompilerIntegrationError::DomJitInvalid)?;
        self.domjit_plans.push(plan);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), CompilerIntegrationError> {
        self.request
            .validate()
            .map_err(CompilerIntegrationError::RequestInvalid)?;
        if self.request.owner != Some(self.owner) {
            return Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::BaselineJit,
            ));
        }
        if self.artifact.tier != self.request.requested_tier
            || self.differential.fallback.attempted_tier != self.request.requested_tier
        {
            return Err(CompilerIntegrationError::TierMismatch);
        }
        if self.artifact.origin.owner != Some(self.owner) {
            return Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::BaselineJit,
            ));
        }
        if let Some(snapshot) = self.tiering_snapshot {
            if snapshot.owner != self.owner || snapshot.to_tier != self.request.requested_tier {
                return Err(CompilerIntegrationError::TierMismatch);
            }
        }
        if let Some(llint) = &self.llint {
            llint.validate()?;
        }
        for handoff in &self.inline_cache_handoffs {
            handoff
                .validate()
                .map_err(CompilerIntegrationError::InlineCacheInvalid)?;
            if handoff.owner != self.owner {
                return Err(CompilerIntegrationError::OwnerMismatch(
                    CompilerIntegrationComponent::InlineCache,
                ));
            }
        }
        if let Some(air_lowering) = &self.air_lowering {
            air_lowering
                .validate()
                .map_err(CompilerIntegrationError::B3Invalid)?;
        }
        if let Some(ftl) = &self.ftl {
            ftl.validate()
                .map_err(CompilerIntegrationError::FtlInvalid)?;
            if ftl.owner != self.owner {
                return Err(CompilerIntegrationError::OwnerMismatch(
                    CompilerIntegrationComponent::Ftl,
                ));
            }
        }
        for domjit_plan in &self.domjit_plans {
            domjit_plan
                .validate()
                .map_err(CompilerIntegrationError::DomJitInvalid)?;
        }
        self.artifact.validate()?;
        self.differential.validate()?;
        for diagnostic in &self.diagnostics {
            diagnostic.validate()?;
            if diagnostic.owner != self.owner || diagnostic.tier != self.request.requested_tier {
                return Err(CompilerIntegrationError::TierMismatch);
            }
        }
        Ok(())
    }

    pub fn compilation_product(&self) -> Result<CompilationProduct, CompilerIntegrationError> {
        self.validate()?;
        let product = CompilationProduct {
            plan: self.request.id,
            outcome: CompilationOutcome::Deferred,
            code: None,
            replacement_for: None,
            invalidation: None,
        };
        product
            .validate()
            .map_err(CompilerIntegrationError::ProductInvalid)?;
        Ok(product)
    }
}

pub fn plan_llint_offlineasm_integration(
    request: LLIntOfflineAsmIntegrationRequest<'_>,
) -> Result<LLIntOfflineAsmIntegrationRecord, CompilerIntegrationError> {
    let lowering =
        plan_offlineasm_symbolic_lowering(request.program, request.backend, request.registry)
            .map_err(CompilerIntegrationError::OfflineAsmInvalid)?;
    let handoff = request
        .frame
        .map(|frame| {
            select_llint_entry_for_interpreter_frame(
                frame,
                request.code_kind,
                request.construct_entry,
                &request.entrypoints,
                LLIntInterpreterHandoffReason::InitialInterpreterEntry,
            )
        })
        .transpose()
        .map_err(CompilerIntegrationError::LlIntHandoffInvalid)?;
    let generated_code_available = request
        .registry
        .backend_schema(request.backend)
        .is_some_and(|schema| schema.status == crate::offlineasm::OfflineAsmBackendStatus::Working);
    let record = LLIntOfflineAsmIntegrationRecord {
        owner: request.owner,
        program: request.program,
        lowering,
        entrypoints: request.entrypoints,
        handoff,
        generated_code_available,
    };
    record.validate()?;
    Ok(record)
}

pub fn descriptor_only_artifact_with_interpreter_fallback(
    code: JitCodeId,
    owner: CodeBlockId,
    executable: Option<ExecutableId>,
    bytecode_index: BytecodeIndex,
    from_tier: JitType,
    attempted_tier: JitType,
    reason: TierFallbackReason,
) -> Result<OptimizedArtifactDescriptor, CompilerIntegrationError> {
    let fallback =
        interpreter_fallback_record(owner, bytecode_index, from_tier, attempted_tier, reason)?;
    let artifact = OptimizedArtifactDescriptor {
        code,
        tier: attempted_tier,
        origin: CodeOrigin {
            kind: origin_kind_for_tier(attempted_tier),
            owner: Some(owner),
            executable,
            bytecode_index: Some(bytecode_index.offset()),
        },
        availability: OptimizedArtifactAvailability::GeneratedCodeUnavailable,
        liveness: CodeLiveness::Unallocated,
        fallback: Some(fallback),
    };
    artifact.validate()?;
    Ok(artifact)
}

pub fn descriptor_only_integration_record(
    owner: CodeBlockId,
    executable: Option<ExecutableId>,
    request: CompilationRequest,
    bytecode_index: BytecodeIndex,
    code: JitCodeId,
    effects: EffectSummary,
) -> Result<OptimizedTierIntegrationRecord, CompilerIntegrationError> {
    request
        .validate()
        .map_err(CompilerIntegrationError::RequestInvalid)?;
    let requested_tier = request.requested_tier;
    let artifact = descriptor_only_artifact_with_interpreter_fallback(
        code,
        owner,
        executable,
        bytecode_index,
        JitType::None,
        request.requested_tier,
        TierFallbackReason::UnsupportedTier,
    )?;
    let fallback = artifact
        .fallback
        .ok_or(CompilerIntegrationError::MissingFallback)?;
    let differential = InterpreterDifferentialRecord {
        owner,
        bytecode_index,
        interpreter_outcome: DifferentialOutcome::NotObserved,
        optimized_outcome: DifferentialOutcome::NotObserved,
        effects,
        fallback,
    };
    let record = OptimizedTierIntegrationRecord {
        owner,
        executable,
        request,
        tiering_snapshot: None,
        llint: None,
        inline_cache_handoffs: Vec::new(),
        dfg_graph: None,
        b3_procedure: None,
        air_lowering: None,
        ftl: None,
        domjit_plans: Vec::new(),
        artifact,
        differential,
        diagnostics: vec![OptimizedTierDiagnostic {
            component: CompilerIntegrationComponent::BaselineJit,
            owner,
            plan: None,
            tier: requested_tier,
            reason: TierFallbackReason::UnsupportedTier,
            bytecode_index: Some(bytecode_index),
            message_ordinal: 1,
        }],
    };
    record.validate()?;
    Ok(record)
}

pub fn ftl_descriptor_failure_artifact(
    compilation: FtlCompilationDescriptor,
    code: JitCodeId,
    bytecode_index: BytecodeIndex,
    reason: LoweringFailureReason,
) -> Result<FtlArtifactDescriptor, CompilerIntegrationError> {
    let mut failed = compilation;
    let owner = failed.owner;
    failed.stage = crate::ftl::FtlCompilationStage::Failed;
    failed.failure = Some(reason);
    let artifact = FtlArtifactDescriptor {
        compilation: failed,
        code: Some(crate::jit::JitCodeArtifact {
            id: code,
            tier: JitType::Ftl,
            origin: CodeOrigin {
                kind: CodeOriginKind::FtlReplacement,
                owner: Some(owner),
                executable: None,
                bytecode_index: Some(bytecode_index.offset()),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: None,
            machine_code: None,
            entrypoint: Entrypoint {
                kind: EntrypointKind::GeneratedCode,
                abi: EntryAbi::GeneratedCode,
                code: Some(code),
                boundary: None,
            },
            patchpoints: Vec::new(),
            dependencies: Vec::new(),
            byproducts: Vec::new(),
            disassembly: None,
            liveness: CodeLiveness::Compiling,
            finalization_authority: CodeFinalizationAuthority::MainThread,
        }),
        osr_entry_artifacts: Vec::new(),
    };
    artifact
        .validate()
        .map_err(CompilerIntegrationError::FtlInvalid)?;
    Ok(artifact)
}

pub fn interpreter_fallback_record(
    owner: CodeBlockId,
    bytecode_index: BytecodeIndex,
    from_tier: JitType,
    attempted_tier: JitType,
    reason: TierFallbackReason,
) -> Result<TierFallbackResultRecord, CompilerIntegrationError> {
    let semantics = TierFallbackSemantics {
        owner,
        from_tier,
        attempted_tier,
        reason,
        target: TierFallbackTarget::ReturnToInterpreter,
        preserves_profile: true,
        should_count_invalidation: matches!(
            reason,
            TierFallbackReason::WatchpointInvalidated | TierFallbackReason::FrequentExit
        ),
    };
    TierFallbackResultRecord::from_semantics(semantics, Some(bytecode_index))
        .map_err(CompilerIntegrationError::TierInvalid)
}

pub fn request_for_tier_fallback(
    id: CompilationPlanId,
    owner: CodeBlockId,
    executable: Option<ExecutableId>,
    mode: CompilationMode,
    kind: CompilationRequestKind,
    snapshot: Option<TieringSnapshot>,
) -> Result<CompilationRequest, CompilerIntegrationError> {
    let mut builder = CompilationRequest::builder(id, mode, kind).owner(owner);
    if let Some(executable) = executable {
        builder = builder.executable(executable);
    }
    if let Some(snapshot) = snapshot {
        builder = builder.tiering_snapshot(snapshot);
    }
    builder
        .build()
        .map_err(CompilerIntegrationError::RequestInvalid)
}

const fn origin_kind_for_tier(tier: JitType) -> CodeOriginKind {
    match tier {
        JitType::Baseline => CodeOriginKind::BaselineCodeBlock,
        JitType::Dfg => CodeOriginKind::DfgReplacement,
        JitType::Ftl => CodeOriginKind::FtlReplacement,
        _ => CodeOriginKind::HostThunk,
    }
}

pub fn fallback_invalidation_reason(reason: TierFallbackReason) -> CodeInvalidationReason {
    match reason {
        TierFallbackReason::WatchpointInvalidated => CodeInvalidationReason::WatchpointFired,
        TierFallbackReason::CompilationCancelled => CodeInvalidationReason::CompilationCancelled,
        _ => CodeInvalidationReason::TierReplacementInstalled,
    }
}

pub fn interpreter_stack_diagnostic_ordinal(stack: Option<InterpreterStackId>) -> u32 {
    stack.map_or(1, |stack| (stack.0 as u32).saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::InterpreterEntrySlot;
    use crate::dfg::{BranchTarget, DfgBasicBlock, DfgGraph};
    use crate::gc::CellId;
    use crate::llint::{LLIntCodePtr, LLIntEntrypoint, LLIntEntrypointKind};
    use crate::offlineasm::OFFLINEASM_SCHEMA_REGISTRY;

    fn owner() -> CodeBlockId {
        CodeBlockId(CellId(41))
    }

    #[test]
    fn descriptor_only_baseline_record_falls_back_to_interpreter() {
        let request = request_for_tier_fallback(
            CompilationPlanId(1),
            owner(),
            None,
            CompilationMode::Baseline,
            CompilationRequestKind::TierUp(crate::jit::TieringTrigger::EntryCounter),
            None,
        )
        .unwrap();

        let record = descriptor_only_integration_record(
            owner(),
            None,
            request,
            BytecodeIndex::from_offset(4),
            JitCodeId(99),
            EffectSummary::pure(),
        )
        .unwrap();

        assert_eq!(
            record.artifact.availability,
            OptimizedArtifactAvailability::GeneratedCodeUnavailable
        );
        assert_eq!(
            record.differential.fallback.bytecode_index,
            Some(BytecodeIndex::from_offset(4))
        );
        assert_eq!(
            record.compilation_product().unwrap().outcome,
            CompilationOutcome::Deferred
        );
    }

    #[test]
    fn differential_rejects_non_interpreter_fallback() {
        let fallback = TierFallbackResultRecord::from_semantics(
            TierFallbackSemantics {
                owner: owner(),
                from_tier: JitType::Baseline,
                attempted_tier: JitType::Dfg,
                reason: TierFallbackReason::PolicyDisabled,
                target: TierFallbackTarget::StayInCurrentTier,
                preserves_profile: true,
                should_count_invalidation: false,
            },
            None,
        )
        .unwrap();
        let differential = InterpreterDifferentialRecord {
            owner: owner(),
            bytecode_index: BytecodeIndex::from_offset(8),
            interpreter_outcome: DifferentialOutcome::ReturnedOrdinal(1),
            optimized_outcome: DifferentialOutcome::ReturnedOrdinal(1),
            effects: EffectSummary::pure(),
            fallback,
        };

        assert_eq!(
            differential.validate(),
            Err(CompilerIntegrationError::MissingDifferential)
        );
    }

    #[test]
    fn llint_offlineasm_record_stays_symbolic_when_backend_is_reserved() {
        let table = LLIntEntrypointTable::from_entrypoints([LLIntEntrypoint {
            kind: LLIntEntrypointKind::Program,
            slot: InterpreterEntrySlot(0),
            code: Some(LLIntCodePtr(0x100)),
            frame_register_count: Some(4),
        }]);

        let record = plan_llint_offlineasm_integration(LLIntOfflineAsmIntegrationRequest {
            owner: owner(),
            frame: None,
            code_kind: CodeKind::Program,
            construct_entry: false,
            entrypoints: table,
            program: OfflineAsmProgramId(7),
            backend: OfflineAsmBackend::CLoop,
            registry: OFFLINEASM_SCHEMA_REGISTRY,
        })
        .unwrap();

        assert!(!record.generated_code_available);
        assert_eq!(record.lowering.program, OfflineAsmProgramId(7));
    }

    #[test]
    fn integration_rejects_inline_cache_handoff_for_other_owner() {
        let request = request_for_tier_fallback(
            CompilationPlanId(2),
            owner(),
            None,
            CompilationMode::Baseline,
            CompilationRequestKind::TierUp(crate::jit::TieringTrigger::EntryCounter),
            None,
        )
        .unwrap();
        let mut record = descriptor_only_integration_record(
            owner(),
            None,
            request,
            BytecodeIndex::from_offset(12),
            JitCodeId(100),
            EffectSummary::pure(),
        )
        .unwrap();
        record
            .inline_cache_handoffs
            .push(InlineCacheMissHandoffDescriptor {
                slot: crate::jit::InlineCacheSlotId(1),
                owner: CodeBlockId(CellId(99)),
                bytecode_index: 12,
                cache_kind: crate::jit::InlineCacheKind::PropertyLoad,
                miss_kind: crate::jit::InlineCacheMissKind::Cold,
                fallback: crate::jit::InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            });

        assert_eq!(
            record.validate(),
            Err(CompilerIntegrationError::OwnerMismatch(
                CompilerIntegrationComponent::InlineCache
            ))
        );
    }

    #[test]
    fn integration_attaches_valid_dfg_graph_by_owner() {
        let request = request_for_tier_fallback(
            CompilationPlanId(3),
            owner(),
            None,
            CompilationMode::Dfg,
            CompilationRequestKind::TierUp(crate::jit::TieringTrigger::LoopCounter),
            None,
        )
        .unwrap();
        let mut record = descriptor_only_integration_record(
            owner(),
            None,
            request,
            BytecodeIndex::from_offset(16),
            JitCodeId(101),
            EffectSummary::pure(),
        )
        .unwrap();
        let graph = DfgGraph::builder(DfgGraphId(5), owner())
            .block(DfgBasicBlock {
                id: crate::dfg::DfgBasicBlockId(0),
                nodes: Vec::new(),
                predecessors: Vec::new(),
                successors: vec![BranchTarget::Fallthrough],
                bytecode_begin: Some(0),
                bytecode_end: Some(0),
                execution_count: Some(1),
                is_osr_entry: false,
                is_catch_entry: false,
            })
            .build()
            .unwrap();

        record.attach_dfg_graph(&graph).unwrap();

        assert_eq!(record.dfg_graph, Some(DfgGraphId(5)));
        assert_eq!(record.validate(), Ok(()));
    }
}
