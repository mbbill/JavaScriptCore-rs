//! OfflineASM contracts.
//!
//! OfflineASM describes LLInt and thunk assembly in a portable DSL. This module
//! names parser products, lowering targets, and generated labels without
//! interpreting the DSL.

use crate::assembler::{AssemblerArchitecture, AssemblerLabel};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OfflineAsmProgramId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OfflineAsmSourceFileId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OfflineAsmTokenId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OfflineAsmNodeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OfflineAsmCodeOrigin {
    pub file: OfflineAsmSourceFileId,
    pub line_number: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmSourceFile {
    pub id: OfflineAsmSourceFileId,
    pub path_ordinal: u32,
    pub basename_ordinal: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfflineAsmAnnotationKind {
    Global,
    Local,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmAnnotation {
    pub origin: OfflineAsmCodeOrigin,
    pub kind: OfflineAsmAnnotationKind,
    pub text_ordinal: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmToken {
    pub id: OfflineAsmTokenId,
    pub origin: OfflineAsmCodeOrigin,
    pub text_ordinal: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfflineAsmNodeKind {
    Sequence,
    Label,
    LocalLabel,
    LabelReference,
    Immediate,
    Register,
    Memory,
    Instruction,
    Macro,
    IfThenElse,
    Setting,
    StructOffset,
    Sizeof,
    Skip,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmAstNode {
    pub id: OfflineAsmNodeId,
    pub kind: OfflineAsmNodeKind,
    pub origin: OfflineAsmCodeOrigin,
    pub children: Vec<OfflineAsmNodeId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfflineAsmOpcodeKind {
    Macro,
    Instruction,
    SlowPath,
    CommonThunk,
    PlatformGuard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfflineAsmBackend {
    X86_64,
    Armv7,
    Arm64,
    Arm64e,
    Riscv64,
    CLoop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfflineAsmBackendStatus {
    Reserved,
    Working,
    ExcludedByConfiguration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmBackendSelection {
    pub backend: OfflineAsmBackend,
    pub status: OfflineAsmBackendStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmProgramDescriptor {
    pub id: OfflineAsmProgramId,
    pub sources: Vec<OfflineAsmSourceFile>,
    pub tokens: Vec<OfflineAsmToken>,
    pub annotations: Vec<OfflineAsmAnnotation>,
    pub root: Option<OfflineAsmNodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OfflineAsmValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicateOpcodeName(&'static str),
    DuplicateBackend(OfflineAsmBackend),
    EmptyNodeKinds(&'static str),
    MissingSource(OfflineAsmSourceFileId),
    MissingRoot(OfflineAsmNodeId),
    DuplicateNodeId(OfflineAsmNodeId),
    NodeChildMissing(OfflineAsmNodeId),
    InvalidOriginLine(OfflineAsmNodeId),
    BackendSchemaMissing(OfflineAsmBackend),
    BackendArchitectureMismatch(OfflineAsmBackend),
    BackendStatusMismatch(OfflineAsmBackend),
    CfiUnsupported(OfflineAsmBackend),
    MetadataUnsupported(OfflineAsmBackend),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmProgramBuilder {
    program: OfflineAsmProgramDescriptor,
}

impl OfflineAsmProgramBuilder {
    pub fn new(id: OfflineAsmProgramId) -> Self {
        Self {
            program: OfflineAsmProgramDescriptor {
                id,
                sources: Vec::new(),
                tokens: Vec::new(),
                annotations: Vec::new(),
                root: None,
            },
        }
    }

    pub fn source(mut self, source: OfflineAsmSourceFile) -> Self {
        self.program.sources.push(source);
        self
    }

    pub fn token(mut self, token: OfflineAsmToken) -> Self {
        self.program.tokens.push(token);
        self
    }

    pub fn annotation(mut self, annotation: OfflineAsmAnnotation) -> Self {
        self.program.annotations.push(annotation);
        self
    }

    pub fn root(mut self, root: OfflineAsmNodeId) -> Self {
        self.program.root = Some(root);
        self
    }

    pub fn build(
        self,
        nodes: &[OfflineAsmAstNode],
    ) -> Result<OfflineAsmProgramDescriptor, OfflineAsmValidationError> {
        self.program.validate(nodes)?;
        Ok(self.program)
    }
}

impl OfflineAsmProgramDescriptor {
    pub fn builder(id: OfflineAsmProgramId) -> OfflineAsmProgramBuilder {
        OfflineAsmProgramBuilder::new(id)
    }

    pub fn validate(&self, nodes: &[OfflineAsmAstNode]) -> Result<(), OfflineAsmValidationError> {
        for (index, node) in nodes.iter().enumerate() {
            if nodes[index + 1..].iter().any(|other| other.id == node.id) {
                return Err(OfflineAsmValidationError::DuplicateNodeId(node.id));
            }
            node.validate(self, nodes)?;
        }
        if let Some(root) = self.root {
            if !nodes.iter().any(|node| node.id == root) {
                return Err(OfflineAsmValidationError::MissingRoot(root));
            }
        }
        for token in &self.tokens {
            if !self
                .sources
                .iter()
                .any(|source| source.id == token.origin.file)
            {
                return Err(OfflineAsmValidationError::MissingSource(token.origin.file));
            }
        }

        Ok(())
    }
}

impl OfflineAsmAstNode {
    pub fn validate(
        &self,
        program: &OfflineAsmProgramDescriptor,
        nodes: &[OfflineAsmAstNode],
    ) -> Result<(), OfflineAsmValidationError> {
        if self.origin.line_number == 0 {
            return Err(OfflineAsmValidationError::InvalidOriginLine(self.id));
        }
        if !program
            .sources
            .iter()
            .any(|source| source.id == self.origin.file)
        {
            return Err(OfflineAsmValidationError::MissingSource(self.origin.file));
        }
        for child in &self.children {
            if !nodes.iter().any(|node| node.id == *child) {
                return Err(OfflineAsmValidationError::NodeChildMissing(*child));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmLoweringPlan {
    pub program: OfflineAsmProgramId,
    pub target: AssemblerArchitecture,
    pub backend: OfflineAsmBackend,
    pub entry_label: Option<AssemblerLabel>,
    pub emits_cfi: bool,
    pub emits_metadata_table: bool,
    /// Parser and transform phases may replace AST nodes; backend lowering owns
    /// label materialization after this plan is created.
    pub selected_backends: Vec<OfflineAsmBackendSelection>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum OfflineAsmSchemaOwner {
    #[default]
    OfflineAsmGenerator,
    LLIntGeneratedData,
    BackendRegistry,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum OfflineAsmRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticOfflineAsmOpcodeSchema {
    pub name: &'static str,
    pub kind: OfflineAsmOpcodeKind,
    pub node_kinds: &'static [OfflineAsmNodeKind],
    pub owner: OfflineAsmSchemaOwner,
    pub mutation_authority: OfflineAsmRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticOfflineAsmBackendSchema {
    pub backend: OfflineAsmBackend,
    pub architecture: AssemblerArchitecture,
    pub status: OfflineAsmBackendStatus,
    pub supports_cfi: bool,
    pub supports_metadata_table: bool,
    pub owner: OfflineAsmSchemaOwner,
    pub mutation_authority: OfflineAsmRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OfflineAsmSchemaRegistry {
    pub opcodes: &'static [StaticOfflineAsmOpcodeSchema],
    pub backends: &'static [StaticOfflineAsmBackendSchema],
}

impl OfflineAsmSchemaRegistry {
    pub const fn new(
        opcodes: &'static [StaticOfflineAsmOpcodeSchema],
        backends: &'static [StaticOfflineAsmBackendSchema],
    ) -> Self {
        Self { opcodes, backends }
    }

    pub const fn opcodes(self) -> &'static [StaticOfflineAsmOpcodeSchema] {
        self.opcodes
    }

    pub const fn backends(self) -> &'static [StaticOfflineAsmBackendSchema] {
        self.backends
    }

    pub fn backend_schema(
        self,
        backend: OfflineAsmBackend,
    ) -> Option<&'static StaticOfflineAsmBackendSchema> {
        self.backends
            .iter()
            .find(|schema| schema.backend == backend)
    }

    pub fn validate(self) -> Result<(), OfflineAsmValidationError> {
        for (index, opcode) in self.opcodes.iter().enumerate() {
            opcode.validate()?;
            if self.opcodes[index + 1..]
                .iter()
                .any(|other| other.name == opcode.name)
            {
                return Err(OfflineAsmValidationError::DuplicateOpcodeName(opcode.name));
            }
        }
        for (index, backend) in self.backends.iter().enumerate() {
            backend.validate()?;
            if self.backends[index + 1..]
                .iter()
                .any(|other| other.backend == backend.backend)
            {
                return Err(OfflineAsmValidationError::DuplicateBackend(backend.backend));
            }
        }

        Ok(())
    }
}

impl StaticOfflineAsmOpcodeSchema {
    pub fn validate(&self) -> Result<(), OfflineAsmValidationError> {
        if self.name.is_empty() {
            return Err(OfflineAsmValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(OfflineAsmValidationError::EmptyProvenance(self.name));
        }
        if self.node_kinds.is_empty() {
            return Err(OfflineAsmValidationError::EmptyNodeKinds(self.name));
        }

        Ok(())
    }
}

impl StaticOfflineAsmBackendSchema {
    pub fn validate(&self) -> Result<(), OfflineAsmValidationError> {
        if self.provenance.is_empty() {
            return Err(OfflineAsmValidationError::EmptyProvenance("backend"));
        }

        Ok(())
    }
}

impl OfflineAsmLoweringPlan {
    pub fn validate_against(
        &self,
        registry: OfflineAsmSchemaRegistry,
    ) -> Result<(), OfflineAsmValidationError> {
        let schema = registry.backend_schema(self.backend).ok_or(
            OfflineAsmValidationError::BackendSchemaMissing(self.backend),
        )?;
        if schema.architecture != self.target {
            return Err(OfflineAsmValidationError::BackendArchitectureMismatch(
                self.backend,
            ));
        }
        if schema.status != OfflineAsmBackendStatus::Working
            && self.selected_backends.iter().any(|selection| {
                selection.backend == self.backend
                    && selection.status == OfflineAsmBackendStatus::Working
            })
        {
            return Err(OfflineAsmValidationError::BackendStatusMismatch(
                self.backend,
            ));
        }
        if self.emits_cfi && !schema.supports_cfi {
            return Err(OfflineAsmValidationError::CfiUnsupported(self.backend));
        }
        if self.emits_metadata_table && !schema.supports_metadata_table {
            return Err(OfflineAsmValidationError::MetadataUnsupported(self.backend));
        }

        Ok(())
    }
}

pub fn plan_offlineasm_symbolic_lowering(
    program: OfflineAsmProgramId,
    backend: OfflineAsmBackend,
    registry: OfflineAsmSchemaRegistry,
) -> Result<OfflineAsmLoweringPlan, OfflineAsmValidationError> {
    registry.validate()?;
    let schema = registry
        .backend_schema(backend)
        .ok_or(OfflineAsmValidationError::BackendSchemaMissing(backend))?;
    let plan = OfflineAsmLoweringPlan {
        program,
        target: schema.architecture,
        backend,
        entry_label: None,
        emits_cfi: schema.supports_cfi,
        emits_metadata_table: schema.supports_metadata_table,
        selected_backends: registry
            .backends()
            .iter()
            .map(|backend_schema| OfflineAsmBackendSelection {
                backend: backend_schema.backend,
                status: backend_schema.status,
            })
            .collect(),
    };
    plan.validate_against(registry)?;
    Ok(plan)
}

const INSTRUCTION_NODE_KINDS: &[OfflineAsmNodeKind] = &[
    OfflineAsmNodeKind::Instruction,
    OfflineAsmNodeKind::Immediate,
    OfflineAsmNodeKind::Register,
    OfflineAsmNodeKind::Memory,
    OfflineAsmNodeKind::LabelReference,
];
const MACRO_NODE_KINDS: &[OfflineAsmNodeKind] = &[
    OfflineAsmNodeKind::Macro,
    OfflineAsmNodeKind::Setting,
    OfflineAsmNodeKind::StructOffset,
    OfflineAsmNodeKind::Sizeof,
];
const SLOW_PATH_NODE_KINDS: &[OfflineAsmNodeKind] = &[
    OfflineAsmNodeKind::Label,
    OfflineAsmNodeKind::Instruction,
    OfflineAsmNodeKind::Macro,
    OfflineAsmNodeKind::Skip,
];

pub const STATIC_OFFLINEASM_OPCODE_SCHEMAS: &[StaticOfflineAsmOpcodeSchema] = &[
    StaticOfflineAsmOpcodeSchema {
        name: "instruction",
        kind: OfflineAsmOpcodeKind::Instruction,
        node_kinds: INSTRUCTION_NODE_KINDS,
        owner: OfflineAsmSchemaOwner::OfflineAsmGenerator,
        mutation_authority: OfflineAsmRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust OfflineASM opcode schema",
    },
    StaticOfflineAsmOpcodeSchema {
        name: "macro",
        kind: OfflineAsmOpcodeKind::Macro,
        node_kinds: MACRO_NODE_KINDS,
        owner: OfflineAsmSchemaOwner::OfflineAsmGenerator,
        mutation_authority: OfflineAsmRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust OfflineASM opcode schema",
    },
    StaticOfflineAsmOpcodeSchema {
        name: "slow-path",
        kind: OfflineAsmOpcodeKind::SlowPath,
        node_kinds: SLOW_PATH_NODE_KINDS,
        owner: OfflineAsmSchemaOwner::LLIntGeneratedData,
        mutation_authority: OfflineAsmRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust OfflineASM opcode schema",
    },
];

pub const STATIC_OFFLINEASM_BACKEND_SCHEMAS: &[StaticOfflineAsmBackendSchema] = &[
    StaticOfflineAsmBackendSchema {
        backend: OfflineAsmBackend::X86_64,
        architecture: AssemblerArchitecture::X86_64,
        status: OfflineAsmBackendStatus::Reserved,
        supports_cfi: true,
        supports_metadata_table: true,
        owner: OfflineAsmSchemaOwner::BackendRegistry,
        mutation_authority: OfflineAsmRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust OfflineASM backend schema",
    },
    StaticOfflineAsmBackendSchema {
        backend: OfflineAsmBackend::Arm64,
        architecture: AssemblerArchitecture::Arm64,
        status: OfflineAsmBackendStatus::Reserved,
        supports_cfi: true,
        supports_metadata_table: true,
        owner: OfflineAsmSchemaOwner::BackendRegistry,
        mutation_authority: OfflineAsmRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust OfflineASM backend schema",
    },
    StaticOfflineAsmBackendSchema {
        backend: OfflineAsmBackend::CLoop,
        architecture: AssemblerArchitecture::Unknown,
        status: OfflineAsmBackendStatus::Reserved,
        supports_cfi: false,
        supports_metadata_table: true,
        owner: OfflineAsmSchemaOwner::BackendRegistry,
        mutation_authority: OfflineAsmRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust OfflineASM backend schema",
    },
];

pub const OFFLINEASM_SCHEMA_REGISTRY: OfflineAsmSchemaRegistry = OfflineAsmSchemaRegistry::new(
    STATIC_OFFLINEASM_OPCODE_SCHEMAS,
    STATIC_OFFLINEASM_BACKEND_SCHEMAS,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_offlineasm_registry_validates() {
        assert_eq!(OFFLINEASM_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn program_builder_rejects_missing_root() {
        let source = OfflineAsmSourceFile {
            id: OfflineAsmSourceFileId(1),
            path_ordinal: 1,
            basename_ordinal: 1,
        };
        let program = OfflineAsmProgramDescriptor::builder(OfflineAsmProgramId(1))
            .source(source)
            .root(OfflineAsmNodeId(42))
            .build(&[]);

        assert_eq!(
            program,
            Err(OfflineAsmValidationError::MissingRoot(OfflineAsmNodeId(42)))
        );
    }

    #[test]
    fn symbolic_lowering_plan_uses_backend_schema_capabilities() {
        let plan = plan_offlineasm_symbolic_lowering(
            OfflineAsmProgramId(9),
            OfflineAsmBackend::CLoop,
            OFFLINEASM_SCHEMA_REGISTRY,
        )
        .unwrap();

        assert_eq!(plan.target, AssemblerArchitecture::Unknown);
        assert!(!plan.emits_cfi);
        assert!(plan.emits_metadata_table);
        assert_eq!(
            plan.selected_backends.len(),
            STATIC_OFFLINEASM_BACKEND_SCHEMAS.len()
        );
    }
}
