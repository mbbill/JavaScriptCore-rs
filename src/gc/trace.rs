//! Tracing interfaces used by the collector.

use crate::gc::GcRef;
use crate::gc::JsCell;

/// Payload contract for visiting strong GC children.
pub trait Trace {
    fn trace(&self, tracer: &mut dyn Tracer);
}

/// Marking visitor interface.
pub trait Tracer {
    fn visit_cell(&mut self, cell: GcRef<JsCell>);
    fn visit_weak_cell(&mut self, cell: GcRef<JsCell>);
    fn note_external_memory(&mut self, bytes: usize);
}

/// Why a root is being marked. This mirrors the separation between ordinary
/// tracing, conservative stack discovery, and verifier/debug visitors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkReason {
    StrongRoot,
    ConservativeRoot,
    WriteBarrier,
    WeakValidation,
    Verifier,
}

/// Marking constraint scheduling policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstraintMode {
    StopTheWorld,
    Concurrent,
    Sequential,
}

/// When a marking constraint should be executed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConstraintExecutionPhase {
    #[default]
    MainThread,
    Fixpoint,
    Parallel,
    Verifier,
}

/// Named marking constraint. The callback body is intentionally absent; future
/// code can attach generated or hand-written visitors behind this descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarkingConstraint {
    pub abbreviated_name: &'static str,
    pub name: &'static str,
    pub mode: ConstraintMode,
    pub phase: ConstraintExecutionPhase,
}

/// Ordered collection of marking constraints.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkingConstraintSet {
    constraints: Vec<MarkingConstraint>,
}

impl MarkingConstraintSet {
    pub fn constraints(&self) -> &[MarkingConstraint] {
        &self.constraints
    }
}
