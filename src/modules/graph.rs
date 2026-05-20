use crate::modules::key::ModuleKey;
use crate::modules::registry::{ModulePromiseSlot, ScriptFetcherSlot};
use crate::modules::{
    ImportMapResolution, ModuleRecordId, ModuleRequest, ModuleRequestFailureKind,
    ModuleRequestPhase, ModuleRequestResolution,
};
use std::collections::HashSet;

/// Iterative graph-loading phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphLoadPhase {
    Resolve,
    Fetch,
    Instantiate,
    Link,
    Evaluate,
    Complete,
    Error,
}

/// Host-defined payload threaded through graph loading callbacks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphLoadPayloadId(u64);

impl GraphLoadPayloadId {
    pub const fn from_host_token(token: u64) -> Self {
        Self(token)
    }

    pub const fn host_token(self) -> u64 {
        self.0
    }
}

/// Kind of payload supplied to `FinishLoadingImportedModule`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphLoadPayloadKind {
    GraphLoadingState,
    DynamicImport,
}

/// Completion passed from host load back into the graph state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleLoadCompletion {
    Normal(ModuleRecordId),
    Abrupt(GraphLoadErrorKind),
}

/// Graph-load failure class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphLoadErrorKind {
    Resolution,
    Fetch,
    Instantiation,
    Evaluation,
    Cancelled,
}

/// A visited module in graph loading.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VisitedModule {
    record: ModuleRecordId,
}

impl VisitedModule {
    pub const fn new(record: ModuleRecordId) -> Self {
        Self { record }
    }

    pub const fn record(self) -> ModuleRecordId {
        self.record
    }
}

/// State for an iterative module graph load.
///
/// This avoids recursive graph traversal and gives host callbacks a place to
/// suspend and later resume without baking in an event loop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleGraphLoad {
    root: ModuleKey,
    phase: GraphLoadPhase,
    payload: Option<GraphLoadPayloadId>,
    promise: Option<ModulePromiseSlot>,
    script_fetcher: Option<ScriptFetcherSlot>,
    visited_owner: GraphVisitedOwner,
    join_state: GraphLoadJoinState,
    pending_modules: u32,
    is_loading: bool,
}

impl ModuleGraphLoad {
    pub const fn new(root: ModuleKey) -> Self {
        Self {
            root,
            phase: GraphLoadPhase::Resolve,
            payload: None,
            promise: None,
            script_fetcher: None,
            visited_owner: GraphVisitedOwner::GraphLoadingState,
            join_state: GraphLoadJoinState::new(),
            pending_modules: 1,
            is_loading: true,
        }
    }

    pub const fn with_payload(
        root: ModuleKey,
        payload: GraphLoadPayloadId,
        phase: GraphLoadPhase,
    ) -> Self {
        Self {
            root,
            phase,
            payload: Some(payload),
            promise: None,
            script_fetcher: None,
            visited_owner: GraphVisitedOwner::GraphLoadingState,
            join_state: GraphLoadJoinState::new(),
            pending_modules: 1,
            is_loading: true,
        }
    }

    pub const fn phase(&self) -> GraphLoadPhase {
        self.phase
    }

    pub const fn root(&self) -> &ModuleKey {
        &self.root
    }

    pub const fn payload(&self) -> Option<GraphLoadPayloadId> {
        self.payload
    }

    pub const fn promise(&self) -> Option<ModulePromiseSlot> {
        self.promise
    }

    pub const fn script_fetcher(&self) -> Option<ScriptFetcherSlot> {
        self.script_fetcher
    }

    pub const fn visited_owner(&self) -> GraphVisitedOwner {
        self.visited_owner
    }

    pub const fn join_state(&self) -> GraphLoadJoinState {
        self.join_state
    }

    pub const fn pending_modules(&self) -> u32 {
        self.pending_modules
    }

    pub const fn is_loading(&self) -> bool {
        self.is_loading
    }
}

/// Owner of immutable module graph descriptor metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleGraphDescriptorOwner {
    ModuleAnalysis,
    RealmModuleLoader,
    HostLoader,
    GeneratedStaticData,
}

/// Provenance for module graph descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleGraphDescriptorProvenance {
    ParsedSourceText,
    HostSyntheticModule,
    GeneratedFromModuleAnalysis,
    GeneratedFromEngineMetadata,
}

/// Static module graph node descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleGraphNodeDescriptor {
    pub record: ModuleRecordId,
    pub key: Option<&'static ModuleKey>,
    pub phase: GraphLoadPhase,
}

/// Static request edge between module records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleGraphEdgeDescriptor {
    pub from: ModuleRecordId,
    pub request: &'static ModuleRequest,
    pub to: Option<ModuleRecordId>,
    pub phase: ModuleRequestPhase,
}

/// Immutable module graph descriptor.
///
/// The descriptor does not resolve, fetch, instantiate, link, or evaluate. It
/// only names graph metadata produced by parser, host, or generated tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleGraphDescriptor {
    pub name: &'static str,
    pub owner: ModuleGraphDescriptorOwner,
    pub provenance: ModuleGraphDescriptorProvenance,
    nodes: &'static [ModuleGraphNodeDescriptor],
    edges: &'static [ModuleGraphEdgeDescriptor],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleGraphValidationError {
    EmptyName,
    DuplicateNode(ModuleRecordId),
    EdgeFromMissing(ModuleRecordId),
    EdgeToMissing(ModuleRecordId),
    EdgePhaseMismatch {
        from: ModuleRecordId,
        edge: ModuleRequestPhase,
        request: ModuleRequestPhase,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleGraphDescriptorBuilder {
    name: &'static str,
    owner: ModuleGraphDescriptorOwner,
    provenance: ModuleGraphDescriptorProvenance,
    nodes: &'static [ModuleGraphNodeDescriptor],
    edges: &'static [ModuleGraphEdgeDescriptor],
}

impl ModuleGraphDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        owner: ModuleGraphDescriptorOwner,
        provenance: ModuleGraphDescriptorProvenance,
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            nodes: &[],
            edges: &[],
        }
    }

    pub const fn nodes(mut self, nodes: &'static [ModuleGraphNodeDescriptor]) -> Self {
        self.nodes = nodes;
        self
    }

    pub const fn edges(mut self, edges: &'static [ModuleGraphEdgeDescriptor]) -> Self {
        self.edges = edges;
        self
    }

    pub fn build(self) -> Result<ModuleGraphDescriptor, ModuleGraphValidationError> {
        let descriptor = ModuleGraphDescriptor::new(
            self.name,
            self.owner,
            self.provenance,
            self.nodes,
            self.edges,
        );
        descriptor.validate()?;
        Ok(descriptor)
    }
}

impl ModuleGraphDescriptor {
    pub const fn new(
        name: &'static str,
        owner: ModuleGraphDescriptorOwner,
        provenance: ModuleGraphDescriptorProvenance,
        nodes: &'static [ModuleGraphNodeDescriptor],
        edges: &'static [ModuleGraphEdgeDescriptor],
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            nodes,
            edges,
        }
    }

    /// Returns immutable module graph nodes.
    pub const fn nodes(&self) -> &'static [ModuleGraphNodeDescriptor] {
        self.nodes
    }

    /// Returns immutable module request edges.
    pub const fn edges(&self) -> &'static [ModuleGraphEdgeDescriptor] {
        self.edges
    }

    /// Returns one existing graph node by table index.
    pub const fn node_at(&self, index: usize) -> Option<&'static ModuleGraphNodeDescriptor> {
        if index < self.nodes.len() {
            Some(&self.nodes[index])
        } else {
            None
        }
    }

    pub fn validate(&self) -> Result<(), ModuleGraphValidationError> {
        validate_module_graph_descriptor(self)
    }
}

pub fn validate_module_graph_descriptor(
    descriptor: &ModuleGraphDescriptor,
) -> Result<(), ModuleGraphValidationError> {
    if descriptor.name.is_empty() {
        return Err(ModuleGraphValidationError::EmptyName);
    }

    let mut records = HashSet::new();
    for node in descriptor.nodes {
        if !records.insert(node.record) {
            return Err(ModuleGraphValidationError::DuplicateNode(node.record));
        }
    }

    for edge in descriptor.edges {
        if !records.contains(&edge.from) {
            return Err(ModuleGraphValidationError::EdgeFromMissing(edge.from));
        }
        if let Some(to) = edge.to {
            if !records.contains(&to) {
                return Err(ModuleGraphValidationError::EdgeToMissing(to));
            }
        }
        if edge.phase != edge.request.phase() {
            return Err(ModuleGraphValidationError::EdgePhaseMismatch {
                from: edge.from,
                edge: edge.phase,
                request: edge.request.phase(),
            });
        }
    }

    Ok(())
}

/// Resolved edge produced from static graph and import-map descriptors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedModuleGraphEdge {
    pub from: ModuleRecordId,
    pub request: &'static ModuleRequest,
    pub key: ModuleKey,
    pub to: ModuleRecordId,
    pub phase: ModuleRequestPhase,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleGraphResolutionError {
    InvalidGraph(ModuleGraphValidationError),
    RequestFailed {
        from: ModuleRecordId,
        kind: ModuleRequestFailureKind,
    },
    TargetMissing {
        from: ModuleRecordId,
        key: ModuleKey,
    },
}

/// Resolves graph request edges without fetching, linking, or evaluating modules.
pub fn resolve_module_graph_descriptor(
    descriptor: &ModuleGraphDescriptor,
    import_map_resolutions: &[ImportMapResolution],
) -> Result<Vec<ResolvedModuleGraphEdge>, ModuleGraphResolutionError> {
    descriptor
        .validate()
        .map_err(ModuleGraphResolutionError::InvalidGraph)?;

    let mut resolved_edges = Vec::with_capacity(descriptor.edges.len());
    for edge in descriptor.edges {
        let resolution =
            crate::modules::resolve_module_request_descriptor(edge.request, import_map_resolutions);
        let ModuleRequestResolution::Resolved(key) = resolution else {
            let kind = match resolution {
                ModuleRequestResolution::Failed(failure) => failure.kind(),
                ModuleRequestResolution::Unresolved | ModuleRequestResolution::Resolved(_) => {
                    ModuleRequestFailureKind::Resolution
                }
            };
            return Err(ModuleGraphResolutionError::RequestFailed {
                from: edge.from,
                kind,
            });
        };

        let to = edge.to.or_else(|| {
            descriptor
                .nodes
                .iter()
                .find(|node| node.key.is_some_and(|node_key| node_key == &key))
                .map(|node| node.record)
        });

        let Some(to) = to else {
            return Err(ModuleGraphResolutionError::TargetMissing {
                from: edge.from,
                key,
            });
        };

        resolved_edges.push(ResolvedModuleGraphEdge {
            from: edge.from,
            request: edge.request,
            key,
            to,
            phase: edge.phase,
        });
    }

    Ok(resolved_edges)
}

/// Owner of the visited-module set used during graph loading.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GraphVisitedOwner {
    #[default]
    GraphLoadingState,
    LoaderPayload,
}

/// Combined-promise join state used by top-level module loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphLoadJoinState {
    pub remaining_fulfillments: u8,
    pub fulfillment: Option<ModuleRecordId>,
}

impl Default for GraphLoadJoinState {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphLoadJoinState {
    pub const fn new() -> Self {
        Self {
            remaining_fulfillments: 2,
            fulfillment: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{
        ImportAttributes, ImportMapResolution, ModuleKey, ModuleRequestKind, ModuleType,
        ResolvedSpecifier, ResolvedSpecifierKind,
    };
    use crate::strings::{AtomId, Identifier};

    const REQUEST: ModuleRequest = ModuleRequest::with_phase(
        ResolvedSpecifier::from_identifier(Identifier::from_atom(AtomId::from_table_slot(7))),
        ModuleType::JavaScript,
        ImportAttributes::empty(),
        ModuleRequestKind::StaticImport,
        ModuleRequestPhase::Fetch,
    );

    #[test]
    fn graph_descriptor_accepts_edges_between_known_nodes() {
        static NODES: &[ModuleGraphNodeDescriptor] = &[
            ModuleGraphNodeDescriptor {
                record: ModuleRecordId::from_loader_slot(1),
                key: None,
                phase: GraphLoadPhase::Fetch,
            },
            ModuleGraphNodeDescriptor {
                record: ModuleRecordId::from_loader_slot(2),
                key: None,
                phase: GraphLoadPhase::Fetch,
            },
        ];
        static EDGES: &[ModuleGraphEdgeDescriptor] = &[ModuleGraphEdgeDescriptor {
            from: ModuleRecordId::from_loader_slot(1),
            request: &REQUEST,
            to: Some(ModuleRecordId::from_loader_slot(2)),
            phase: ModuleRequestPhase::Fetch,
        }];

        let graph = ModuleGraphDescriptorBuilder::new(
            "graph",
            ModuleGraphDescriptorOwner::ModuleAnalysis,
            ModuleGraphDescriptorProvenance::ParsedSourceText,
        )
        .nodes(NODES)
        .edges(EDGES)
        .build()
        .unwrap();

        assert_eq!(graph.edges().len(), 1);
    }

    #[test]
    fn graph_descriptor_rejects_missing_target_node() {
        static NODES: &[ModuleGraphNodeDescriptor] = &[ModuleGraphNodeDescriptor {
            record: ModuleRecordId::from_loader_slot(1),
            key: None,
            phase: GraphLoadPhase::Fetch,
        }];
        static EDGES: &[ModuleGraphEdgeDescriptor] = &[ModuleGraphEdgeDescriptor {
            from: ModuleRecordId::from_loader_slot(1),
            request: &REQUEST,
            to: Some(ModuleRecordId::from_loader_slot(2)),
            phase: ModuleRequestPhase::Fetch,
        }];

        let error = ModuleGraphDescriptor::new(
            "graph",
            ModuleGraphDescriptorOwner::ModuleAnalysis,
            ModuleGraphDescriptorProvenance::ParsedSourceText,
            NODES,
            EDGES,
        )
        .validate()
        .unwrap_err();

        assert_eq!(
            error,
            ModuleGraphValidationError::EdgeToMissing(ModuleRecordId::from_loader_slot(2))
        );
    }

    const REQUESTED: ResolvedSpecifier =
        ResolvedSpecifier::from_identifier(Identifier::from_atom(AtomId::from_table_slot(10)));
    const RESOLVED: ResolvedSpecifier =
        ResolvedSpecifier::from_identifier(Identifier::from_atom(AtomId::from_table_slot(11)));
    static RESOLVED_KEY: ModuleKey =
        ModuleKey::new(RESOLVED, ModuleType::JavaScript, ImportAttributes::empty());
    static MAPPED_REQUEST: ModuleRequest = ModuleRequest::with_phase(
        REQUESTED,
        ModuleType::JavaScript,
        ImportAttributes::empty(),
        ModuleRequestKind::StaticImport,
        ModuleRequestPhase::Fetch,
    );
    static IMPORT_MAPS: &[ImportMapResolution] = &[ImportMapResolution {
        import_map: crate::modules::ImportMapId::from_realm_slot(1),
        base_url: Identifier::from_atom(AtomId::from_table_slot(20)),
        requested_specifier: Identifier::from_atom(AtomId::from_table_slot(10)),
        resolved_specifier: RESOLVED,
        integrity_metadata: None,
        kind: ResolvedSpecifierKind::ImportMapResolved,
    }];

    #[test]
    fn graph_resolution_uses_import_map_to_find_target_node() {
        static NODES: &[ModuleGraphNodeDescriptor] = &[
            ModuleGraphNodeDescriptor {
                record: ModuleRecordId::from_loader_slot(1),
                key: None,
                phase: GraphLoadPhase::Fetch,
            },
            ModuleGraphNodeDescriptor {
                record: ModuleRecordId::from_loader_slot(2),
                key: Some(&RESOLVED_KEY),
                phase: GraphLoadPhase::Fetch,
            },
        ];
        static EDGES: &[ModuleGraphEdgeDescriptor] = &[ModuleGraphEdgeDescriptor {
            from: ModuleRecordId::from_loader_slot(1),
            request: &MAPPED_REQUEST,
            to: None,
            phase: ModuleRequestPhase::Fetch,
        }];
        let graph = ModuleGraphDescriptor::new(
            "graph",
            ModuleGraphDescriptorOwner::ModuleAnalysis,
            ModuleGraphDescriptorProvenance::ParsedSourceText,
            NODES,
            EDGES,
        );

        let resolved = resolve_module_graph_descriptor(&graph, IMPORT_MAPS).unwrap();

        assert_eq!(resolved[0].to, ModuleRecordId::from_loader_slot(2));
        assert_eq!(resolved[0].key, RESOLVED_KEY);
    }
}
