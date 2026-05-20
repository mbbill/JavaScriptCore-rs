//! Shared compiler semantic summaries.
//!
//! These summaries describe observable compiler metadata. They do not dispatch
//! bytecode, call host hooks, patch generated code, or decide whether a tier is
//! profitable.

/// Conservative summary of effects that a compiler descriptor may expose to
/// validators, diagnostics, and tier planning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EffectSummary {
    pub reads_heap: bool,
    pub writes_heap: bool,
    pub allocates: bool,
    pub may_call_js: bool,
    pub may_throw: bool,
    pub may_exit: bool,
    pub terminates: bool,
    pub reads_local_state: bool,
    pub writes_local_state: bool,
    pub reads_pinned: bool,
    pub writes_pinned: bool,
    pub fence: bool,
}

impl EffectSummary {
    pub const fn pure() -> Self {
        Self {
            reads_heap: false,
            writes_heap: false,
            allocates: false,
            may_call_js: false,
            may_throw: false,
            may_exit: false,
            terminates: false,
            reads_local_state: false,
            writes_local_state: false,
            reads_pinned: false,
            writes_pinned: false,
            fence: false,
        }
    }

    pub const fn for_call() -> Self {
        Self {
            reads_heap: true,
            writes_heap: true,
            allocates: false,
            may_call_js: true,
            may_throw: true,
            may_exit: true,
            terminates: false,
            reads_local_state: false,
            writes_local_state: false,
            reads_pinned: true,
            writes_pinned: true,
            fence: true,
        }
    }

    pub const fn for_check() -> Self {
        Self {
            reads_heap: true,
            writes_heap: false,
            allocates: false,
            may_call_js: false,
            may_throw: false,
            may_exit: true,
            terminates: false,
            reads_local_state: false,
            writes_local_state: false,
            reads_pinned: false,
            writes_pinned: false,
            fence: false,
        }
    }

    pub const fn observes_world(self) -> bool {
        self.reads_heap || self.may_call_js || self.may_throw || self.may_exit
    }

    pub const fn mutates_world(self) -> bool {
        self.writes_heap || self.allocates || self.may_call_js
    }

    pub const fn must_preserve_order(self) -> bool {
        self.terminates
            || self.may_exit
            || self.may_throw
            || self.writes_heap
            || self.allocates
            || self.writes_local_state
            || self.writes_pinned
            || self.fence
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            reads_heap: self.reads_heap || other.reads_heap,
            writes_heap: self.writes_heap || other.writes_heap,
            allocates: self.allocates || other.allocates,
            may_call_js: self.may_call_js || other.may_call_js,
            may_throw: self.may_throw || other.may_throw,
            may_exit: self.may_exit || other.may_exit,
            terminates: self.terminates || other.terminates,
            reads_local_state: self.reads_local_state || other.reads_local_state,
            writes_local_state: self.writes_local_state || other.writes_local_state,
            reads_pinned: self.reads_pinned || other.reads_pinned,
            writes_pinned: self.writes_pinned || other.writes_pinned,
            fence: self.fence || other.fence,
        }
    }
}
