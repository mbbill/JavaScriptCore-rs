//! Watchpoint contracts for structure and prototype invalidation.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WatchpointState {
    #[default]
    Clear,
    Watching,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchpointKind {
    StructureTransition,
    PrototypeMutation,
    PropertyReplacement,
    ImpureProperty,
    IndexedStorageMode,
}

#[derive(Clone, Debug, Default)]
pub struct Watchpoint {
    pub state: WatchpointState,
    pub kind: Option<WatchpointKind>,
    pub reason: Option<&'static str>,
}

/// Structure/prototype/cache invalidation state.
#[derive(Clone, Debug, Default)]
pub struct WatchpointSet {
    state: WatchpointState,
    generation: u64,
    kind: Option<WatchpointKind>,
}

impl WatchpointSet {
    pub fn state(&self) -> WatchpointState {
        self.state
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn kind(&self) -> Option<WatchpointKind> {
        self.kind
    }

    pub fn start_watching(&mut self, kind: WatchpointKind) {
        self.state = WatchpointState::Watching;
        self.kind = Some(kind);
    }

    pub fn invalidate(&mut self, _reason: &'static str) {
        // Future JIT/cache integration will notify dependents here.
        self.state = WatchpointState::Invalidated;
        self.generation = self.generation.saturating_add(1);
    }
}
