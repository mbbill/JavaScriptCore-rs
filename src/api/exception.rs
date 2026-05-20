use crate::api::handles::ApiValueRef;
use crate::bytecode::BytecodeIndex;
use crate::gc::{CollectionKind, GcPhase, HeapId, HeapSnapshotId};
use crate::jit::{JitType, TierFallbackResultRecord, TierFallbackResumeKind, TieringState};
use crate::runtime::CodeBlockId;

/// How an API operation reported exception state.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiThrowDisposition {
    DidNotThrow,
    PendingException,
    Terminated,
}

/// Nullability and ownership contract for an exception out-parameter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiExceptionSlotPolicy {
    IgnoredWhenNull,
    ClearedBeforeCall,
    WrittenOnlyOnThrow,
}

/// Exception out-parameter slot.
///
/// This models `JSValueRef* exception` without exposing a raw pointer. The C ABI
/// shim owns pointer validation and null checks; Rust entry code should traffic
/// in this structured slot after the API lock has been acquired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExceptionSlot {
    policy: ApiExceptionSlotPolicy,
    current: Option<ApiValueRef>,
}

impl ApiExceptionSlot {
    pub const fn new(policy: ApiExceptionSlotPolicy) -> Self {
        Self {
            policy,
            current: None,
        }
    }

    pub const fn with_current(policy: ApiExceptionSlotPolicy, current: ApiValueRef) -> Self {
        Self {
            policy,
            current: Some(current),
        }
    }

    pub const fn policy(self) -> ApiExceptionSlotPolicy {
        self.policy
    }

    pub const fn current(self) -> Option<ApiValueRef> {
        self.current
    }

    pub fn apply(
        self,
        result: ApiExceptionResult,
    ) -> Result<ApiExceptionSlotUpdate, ApiExceptionSemanticError> {
        result.validate()?;

        let (next, writes_slot) = match (self.policy, result.disposition) {
            (ApiExceptionSlotPolicy::IgnoredWhenNull, _) => (self.current, false),
            (ApiExceptionSlotPolicy::ClearedBeforeCall, ApiThrowDisposition::PendingException) => {
                (result.exception, true)
            }
            (ApiExceptionSlotPolicy::ClearedBeforeCall, _) => (None, self.current.is_some()),
            (ApiExceptionSlotPolicy::WrittenOnlyOnThrow, ApiThrowDisposition::PendingException) => {
                (result.exception, true)
            }
            (ApiExceptionSlotPolicy::WrittenOnlyOnThrow, _) => (self.current, false),
        };

        Ok(ApiExceptionSlotUpdate {
            policy: self.policy,
            disposition: result.disposition,
            previous: self.current,
            next,
            writes_slot,
        })
    }
}

/// Exception out-parameter bridge.
///
/// This mirrors the public C API pattern without deciding final `JSValueRef`
/// bit-compatibility. Setting this result must stay synchronized with VM
/// pending-exception state.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExceptionResult {
    disposition: ApiThrowDisposition,
    exception: Option<ApiValueRef>,
}

impl ApiExceptionResult {
    pub const fn none() -> Self {
        Self {
            disposition: ApiThrowDisposition::DidNotThrow,
            exception: None,
        }
    }

    pub const fn pending(exception: ApiValueRef) -> Self {
        Self {
            disposition: ApiThrowDisposition::PendingException,
            exception: Some(exception),
        }
    }

    pub const fn terminated() -> Self {
        Self {
            disposition: ApiThrowDisposition::Terminated,
            exception: None,
        }
    }

    pub const fn disposition(self) -> ApiThrowDisposition {
        self.disposition
    }

    pub const fn exception(self) -> Option<ApiValueRef> {
        self.exception
    }

    pub fn validate(self) -> Result<(), ApiExceptionSemanticError> {
        match (self.disposition, self.exception) {
            (ApiThrowDisposition::DidNotThrow, None) => Ok(()),
            (ApiThrowDisposition::DidNotThrow, Some(_)) => {
                Err(ApiExceptionSemanticError::NonThrowingResultCarriesException)
            }
            (ApiThrowDisposition::PendingException, Some(_)) => Ok(()),
            (ApiThrowDisposition::PendingException, None) => {
                Err(ApiExceptionSemanticError::PendingExceptionMissingValue)
            }
            (ApiThrowDisposition::Terminated, None) => Ok(()),
            (ApiThrowDisposition::Terminated, Some(_)) => {
                Err(ApiExceptionSemanticError::TerminatedResultCarriesException)
            }
        }
    }
}

/// Pure slot update produced from an API exception result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExceptionSlotUpdate {
    pub policy: ApiExceptionSlotPolicy,
    pub disposition: ApiThrowDisposition,
    pub previous: Option<ApiValueRef>,
    pub next: Option<ApiValueRef>,
    pub writes_slot: bool,
}

/// Semantic API exception boundary failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiExceptionSemanticError {
    PendingExceptionMissingValue,
    NonThrowingResultCarriesException,
    TerminatedResultCarriesException,
    ReturnedValueMissingValue,
    VoidResultCarriesValue,
    ThrowResultCarriesReturnValue,
    TerminatedResultCarriesReturnValue,
    ExecutionResultDispositionMismatch,
}

/// API operation result that can carry either a value or exception metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiOperationResult<T> {
    Value(T),
    Exception(ApiExceptionResult),
}

/// Public API result class after a VM entry has returned to the embedding API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiExecutionResultKind {
    ReturnedValue,
    ReturnedVoid,
    ThrewException,
    Terminated,
}

/// Non-owning API result observation for entry/exit diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExecutionResultRecord {
    pub kind: ApiExecutionResultKind,
    pub value: Option<ApiValueRef>,
    pub exception: ApiExceptionResult,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiGcEventResultKind {
    NotRequested,
    Scheduled,
    Completed,
    RejectedByNoGcScope,
    SnapshotObserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiGcEventResultRecord {
    pub kind: ApiGcEventResultKind,
    pub heap: Option<HeapId>,
    pub collection: Option<CollectionKind>,
    pub phase: Option<GcPhase>,
    pub snapshot: Option<HeapSnapshotId>,
    pub protected_value_count: usize,
    pub forced_by_api: bool,
}

/// API-visible execution summary for diagnostics and embedders.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiExecutionDiagnosticSummary {
    pub kind: ApiExecutionResultKind,
    pub returned_value: bool,
    pub exception_visible: bool,
    pub terminated: bool,
    pub gc_observed: Option<ApiGcDiagnosticSummary>,
}

impl ApiExecutionDiagnosticSummary {
    pub fn from_result(
        result: ApiExecutionResultRecord,
        gc: Option<ApiGcEventResultRecord>,
    ) -> Result<Self, ApiExceptionSemanticError> {
        result.validate()?;
        Ok(Self {
            kind: result.kind,
            returned_value: result.value.is_some(),
            exception_visible: result.exception.exception().is_some(),
            terminated: result.kind == ApiExecutionResultKind::Terminated,
            gc_observed: gc.map(ApiGcDiagnosticSummary::from_record),
        })
    }
}

/// API-visible GC summary derived from existing API GC observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiGcDiagnosticSummary {
    pub kind: ApiGcEventResultKind,
    pub heap: Option<HeapId>,
    pub collection: Option<CollectionKind>,
    pub phase: Option<GcPhase>,
    pub snapshot: Option<HeapSnapshotId>,
    pub protected_value_count: usize,
    pub forced_by_api: bool,
    pub completed: bool,
    pub rejected_by_no_gc_scope: bool,
}

impl ApiGcDiagnosticSummary {
    pub const fn from_record(record: ApiGcEventResultRecord) -> Self {
        Self {
            kind: record.kind,
            heap: record.heap,
            collection: record.collection,
            phase: record.phase,
            snapshot: record.snapshot,
            protected_value_count: record.protected_value_count,
            forced_by_api: record.forced_by_api,
            completed: matches!(
                record.kind,
                ApiGcEventResultKind::Completed | ApiGcEventResultKind::SnapshotObserved
            ),
            rejected_by_no_gc_scope: matches!(
                record.kind,
                ApiGcEventResultKind::RejectedByNoGcScope
            ),
        }
    }
}

/// API-visible tier summary derived from tiering state or fallback records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiTierDiagnosticSummary {
    pub owner: Option<CodeBlockId>,
    pub current_tier: Option<JitType>,
    pub requested_tier: Option<JitType>,
    pub fallback_resume: Option<TierFallbackResumeKind>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub active_request_visible: bool,
    pub fallback_clears_active_request: bool,
    pub profile_preserved: bool,
}

impl ApiTierDiagnosticSummary {
    pub const fn from_tiering_state(state: &TieringState) -> Self {
        Self {
            owner: state.owner,
            current_tier: Some(state.current_tier),
            requested_tier: state.requested_tier,
            fallback_resume: None,
            bytecode_index: None,
            active_request_visible: state.active_request.is_some(),
            fallback_clears_active_request: false,
            profile_preserved: true,
        }
    }

    pub const fn from_fallback(record: TierFallbackResultRecord) -> Self {
        Self {
            owner: Some(record.owner),
            current_tier: Some(record.from_tier),
            requested_tier: Some(record.attempted_tier),
            fallback_resume: Some(record.resume),
            bytecode_index: record.bytecode_index,
            active_request_visible: true,
            fallback_clears_active_request: record.clears_active_request,
            profile_preserved: record.preserves_profile,
        }
    }
}

impl ApiExecutionResultRecord {
    pub const fn returned_value(value: ApiValueRef) -> Self {
        Self {
            kind: ApiExecutionResultKind::ReturnedValue,
            value: Some(value),
            exception: ApiExceptionResult::none(),
        }
    }

    pub const fn returned_void() -> Self {
        Self {
            kind: ApiExecutionResultKind::ReturnedVoid,
            value: None,
            exception: ApiExceptionResult::none(),
        }
    }

    pub const fn threw(exception: ApiValueRef) -> Self {
        Self {
            kind: ApiExecutionResultKind::ThrewException,
            value: None,
            exception: ApiExceptionResult::pending(exception),
        }
    }

    pub const fn terminated() -> Self {
        Self {
            kind: ApiExecutionResultKind::Terminated,
            value: None,
            exception: ApiExceptionResult::terminated(),
        }
    }

    pub fn validate(self) -> Result<(), ApiExceptionSemanticError> {
        self.exception.validate()?;
        match (self.kind, self.value, self.exception.disposition()) {
            (ApiExecutionResultKind::ReturnedValue, Some(_), ApiThrowDisposition::DidNotThrow)
            | (ApiExecutionResultKind::ReturnedVoid, None, ApiThrowDisposition::DidNotThrow)
            | (
                ApiExecutionResultKind::ThrewException,
                None,
                ApiThrowDisposition::PendingException,
            )
            | (ApiExecutionResultKind::Terminated, None, ApiThrowDisposition::Terminated) => Ok(()),
            (ApiExecutionResultKind::ReturnedValue, None, _) => {
                Err(ApiExceptionSemanticError::ReturnedValueMissingValue)
            }
            (ApiExecutionResultKind::ReturnedVoid, Some(_), _) => {
                Err(ApiExceptionSemanticError::VoidResultCarriesValue)
            }
            (ApiExecutionResultKind::ThrewException, Some(_), _) => {
                Err(ApiExceptionSemanticError::ThrowResultCarriesReturnValue)
            }
            (ApiExecutionResultKind::Terminated, Some(_), _) => {
                Err(ApiExceptionSemanticError::TerminatedResultCarriesReturnValue)
            }
            _ => Err(ApiExceptionSemanticError::ExecutionResultDispositionMismatch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::handles::ApiOpaqueHandle;
    use core::ffi::c_void;
    use core::ptr::NonNull;

    fn value_ref() -> ApiValueRef {
        let raw = NonNull::<c_void>::dangling();
        let handle = unsafe { ApiOpaqueHandle::from_raw(raw) };
        unsafe { ApiValueRef::from_opaque(handle) }
    }

    #[test]
    fn exception_result_requires_pending_value() {
        let invalid = ApiExceptionResult {
            disposition: ApiThrowDisposition::PendingException,
            exception: None,
        };

        assert_eq!(
            invalid.validate(),
            Err(ApiExceptionSemanticError::PendingExceptionMissingValue)
        );
    }

    #[test]
    fn written_only_on_throw_preserves_slot_without_throw() {
        let existing = value_ref();
        let slot =
            ApiExceptionSlot::with_current(ApiExceptionSlotPolicy::WrittenOnlyOnThrow, existing);

        let update = slot.apply(ApiExceptionResult::none()).expect("slot update");

        assert_eq!(update.next, Some(existing));
        assert!(!update.writes_slot);
    }

    #[test]
    fn classifies_api_execution_result_records() {
        let value = value_ref();
        let returned = ApiExecutionResultRecord::returned_value(value);
        let thrown = ApiExecutionResultRecord::threw(value);

        assert_eq!(returned.validate(), Ok(()));
        assert_eq!(returned.kind, ApiExecutionResultKind::ReturnedValue);
        assert_eq!(thrown.validate(), Ok(()));
        assert_eq!(
            thrown.exception.disposition(),
            ApiThrowDisposition::PendingException
        );
    }

    #[test]
    fn exposes_api_execution_and_gc_diagnostic_summary() {
        let summary = ApiExecutionDiagnosticSummary::from_result(
            ApiExecutionResultRecord::returned_void(),
            Some(ApiGcEventResultRecord {
                kind: ApiGcEventResultKind::RejectedByNoGcScope,
                heap: Some(HeapId(1)),
                collection: None,
                phase: None,
                snapshot: None,
                protected_value_count: 2,
                forced_by_api: true,
            }),
        )
        .expect("summary");

        assert_eq!(summary.kind, ApiExecutionResultKind::ReturnedVoid);
        assert!(!summary.returned_value);
        assert!(summary
            .gc_observed
            .is_some_and(|gc| gc.rejected_by_no_gc_scope && gc.forced_by_api));
    }

    #[test]
    fn rejects_mismatched_api_execution_result_record() {
        let invalid = ApiExecutionResultRecord {
            kind: ApiExecutionResultKind::ReturnedValue,
            value: None,
            exception: ApiExceptionResult::none(),
        };

        assert_eq!(
            invalid.validate(),
            Err(ApiExceptionSemanticError::ReturnedValueMissingValue)
        );
    }
}
