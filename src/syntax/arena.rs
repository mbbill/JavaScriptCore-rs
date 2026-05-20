use std::collections::HashMap;
use std::marker::PhantomData;

/// Parser-product arena owner.
///
/// The arena owns AST nodes and parser-local identifier handles. Future
/// implementations may use typed bump allocation internally, but all public
/// handles must prevent use after the arena is dropped. Unlike C++ JSC, Rust
/// code should not expose placement-new objects or raw parser pointers; typed
/// IDs are the stable boundary and any unsafe bump allocation remains private.
#[derive(Debug, Default)]
pub struct ParserArena {
    identifiers: IdentifierArena,
    nodes: NodeArena,
    scopes: Vec<crate::syntax::ast::ScopeNode>,
    statements: Vec<crate::syntax::ast::Stmt>,
    expressions: Vec<crate::syntax::ast::Expr>,
    patterns: Vec<crate::syntax::ast::Pattern>,
    functions: Vec<crate::syntax::ast::FunctionMetadata>,
    generation: ArenaGeneration,
}

impl ParserArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn identifiers(&self) -> &IdentifierArena {
        &self.identifiers
    }

    pub fn identifiers_mut(&mut self) -> &mut IdentifierArena {
        &mut self.identifiers
    }

    pub fn generation(&self) -> ArenaGeneration {
        self.generation
    }

    pub fn reserve_node<T>(&mut self, kind: NodeArenaKind) -> AstRef<T> {
        self.nodes.reserve(kind, self.generation)
    }

    pub fn alloc_scope_node(
        &mut self,
        node: crate::syntax::ast::ScopeNode,
    ) -> AstRef<crate::syntax::ast::ScopeNode> {
        let slot = self.scopes.len().try_into().unwrap_or(u32::MAX);
        let reference = self
            .nodes
            .reserve_stored(NodeArenaKind::Scope, slot, self.generation);
        self.scopes.push(node);
        reference
    }

    pub fn alloc_statement(
        &mut self,
        node: crate::syntax::ast::Stmt,
    ) -> AstRef<crate::syntax::ast::Stmt> {
        let slot = self.statements.len().try_into().unwrap_or(u32::MAX);
        let reference = self
            .nodes
            .reserve_stored(NodeArenaKind::Statement, slot, self.generation);
        self.statements.push(node);
        reference
    }

    pub fn alloc_expression(
        &mut self,
        node: crate::syntax::ast::Expr,
    ) -> AstRef<crate::syntax::ast::Expr> {
        let slot = self.expressions.len().try_into().unwrap_or(u32::MAX);
        let reference = self
            .nodes
            .reserve_stored(NodeArenaKind::Expression, slot, self.generation);
        self.expressions.push(node);
        reference
    }

    pub fn alloc_pattern(
        &mut self,
        node: crate::syntax::ast::Pattern,
    ) -> AstRef<crate::syntax::ast::Pattern> {
        let slot = self.patterns.len().try_into().unwrap_or(u32::MAX);
        let reference = self
            .nodes
            .reserve_stored(NodeArenaKind::Pattern, slot, self.generation);
        self.patterns.push(node);
        reference
    }

    pub fn alloc_function_metadata(
        &mut self,
        node: crate::syntax::ast::FunctionMetadata,
    ) -> AstRef<crate::syntax::ast::FunctionMetadata> {
        let slot = self.functions.len().try_into().unwrap_or(u32::MAX);
        let reference =
            self.nodes
                .reserve_stored(NodeArenaKind::FunctionMetadata, slot, self.generation);
        self.functions.push(node);
        reference
    }

    pub fn reserve_scope_node(&mut self) -> AstRef<crate::syntax::ast::ScopeNode> {
        self.reserve_node(NodeArenaKind::Scope)
    }

    pub fn reserve_statement(&mut self) -> AstRef<crate::syntax::ast::Stmt> {
        self.reserve_node(NodeArenaKind::Statement)
    }

    pub fn reserve_expression(&mut self) -> AstRef<crate::syntax::ast::Expr> {
        self.reserve_node(NodeArenaKind::Expression)
    }

    pub fn reserve_pattern(&mut self) -> AstRef<crate::syntax::ast::Pattern> {
        self.reserve_node(NodeArenaKind::Pattern)
    }

    pub fn reserve_function_metadata(&mut self) -> AstRef<crate::syntax::ast::FunctionMetadata> {
        self.reserve_node(NodeArenaKind::FunctionMetadata)
    }

    pub fn node_count(&self) -> u32 {
        self.nodes.len()
    }

    pub fn node_descriptor(&self, id: NodeId) -> Option<&NodeDescriptor> {
        self.nodes.descriptor(id)
    }

    pub fn scope_node(
        &self,
        reference: AstRef<crate::syntax::ast::ScopeNode>,
    ) -> Option<&crate::syntax::ast::ScopeNode> {
        self.stored_node(reference, NodeArenaKind::Scope, &self.scopes)
    }

    pub fn statement(
        &self,
        reference: AstRef<crate::syntax::ast::Stmt>,
    ) -> Option<&crate::syntax::ast::Stmt> {
        self.stored_node(reference, NodeArenaKind::Statement, &self.statements)
    }

    pub fn expression(
        &self,
        reference: AstRef<crate::syntax::ast::Expr>,
    ) -> Option<&crate::syntax::ast::Expr> {
        self.stored_node(reference, NodeArenaKind::Expression, &self.expressions)
    }

    pub fn pattern(
        &self,
        reference: AstRef<crate::syntax::ast::Pattern>,
    ) -> Option<&crate::syntax::ast::Pattern> {
        self.stored_node(reference, NodeArenaKind::Pattern, &self.patterns)
    }

    pub fn function_metadata(
        &self,
        reference: AstRef<crate::syntax::ast::FunctionMetadata>,
    ) -> Option<&crate::syntax::ast::FunctionMetadata> {
        self.stored_node(reference, NodeArenaKind::FunctionMetadata, &self.functions)
    }

    fn stored_node<'a, T>(
        &'a self,
        reference: AstRef<T>,
        expected: NodeArenaKind,
        storage: &'a [T],
    ) -> Option<&'a T> {
        let descriptor = self.node_descriptor(reference.id())?;
        if descriptor.kind != expected {
            return None;
        }
        let slot = usize::try_from(descriptor.storage_slot?).ok()?;
        storage.get(slot)
    }

    pub fn validate_ref<T>(
        &self,
        reference: AstRef<T>,
        expected: NodeArenaKind,
    ) -> AstRefValidationReport {
        let mut findings = Vec::new();
        let id = reference.id();
        if id.generation != self.generation {
            findings.push(AstRefValidationFinding::GenerationMismatch {
                expected: self.generation,
                actual: id.generation,
            });
        }

        match self.node_descriptor(id) {
            Some(descriptor) if descriptor.kind != expected => {
                findings.push(AstRefValidationFinding::KindMismatch {
                    id,
                    expected,
                    actual: descriptor.kind,
                });
            }
            Some(_) => {}
            None => findings.push(AstRefValidationFinding::MissingNode { id, expected }),
        }

        AstRefValidationReport { findings }
    }
}

/// Typed non-owning handle into `ParserArena`.
///
/// This is an index-style placeholder. It deliberately does not expose raw node
/// pointers or downcasts as an API contract. `generation` lets future parser
/// roots reject handles that came from a cleared or swapped arena.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AstRef<T> {
    id: NodeId,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for AstRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for AstRef<T> {}

impl<T> AstRef<T> {
    pub fn from_raw_index(index: u32) -> Self {
        Self {
            id: NodeId {
                index,
                generation: ArenaGeneration::default(),
            },
            _marker: PhantomData,
        }
    }

    pub fn raw_index(self) -> u32 {
        self.id.index
    }

    pub fn id(self) -> NodeId {
        self.id
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ArenaGeneration(pub u32);

/// Parser-arena node identity.
///
/// `ParserArena` owns the allocation table and is the only mutation authority.
/// `NodeId` is a non-owning handle scoped by `ArenaGeneration`; it is not a GC
/// cell, runtime object, or semantic-scope identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct NodeId {
    pub index: u32,
    pub generation: ArenaGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NodeArenaKind {
    Root,
    Scope,
    Statement,
    Expression,
    Pattern,
    FunctionMetadata,
    ModuleRecord,
}

#[derive(Debug, Default)]
pub struct NodeArena {
    descriptors: Vec<NodeDescriptor>,
}

impl NodeArena {
    fn reserve<T>(&mut self, kind: NodeArenaKind, generation: ArenaGeneration) -> AstRef<T> {
        self.reserve_with_slot(kind, None, generation)
    }

    fn reserve_stored<T>(
        &mut self,
        kind: NodeArenaKind,
        storage_slot: u32,
        generation: ArenaGeneration,
    ) -> AstRef<T> {
        self.reserve_with_slot(kind, Some(storage_slot), generation)
    }

    fn reserve_with_slot<T>(
        &mut self,
        kind: NodeArenaKind,
        storage_slot: Option<u32>,
        generation: ArenaGeneration,
    ) -> AstRef<T> {
        let index = self.descriptors.len().try_into().unwrap_or(u32::MAX);
        self.descriptors.push(NodeDescriptor { kind, storage_slot });
        AstRef {
            id: NodeId { index, generation },
            _marker: PhantomData,
        }
    }

    fn len(&self) -> u32 {
        self.descriptors.len().try_into().unwrap_or(u32::MAX)
    }

    pub fn descriptor(&self, id: NodeId) -> Option<&NodeDescriptor> {
        self.descriptors.get(usize::try_from(id.index).ok()?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NodeDescriptor {
    pub kind: NodeArenaKind,
    pub storage_slot: Option<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AstRefValidationReport {
    pub findings: Vec<AstRefValidationFinding>,
}

impl AstRefValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstRefValidationFinding {
    MissingNode {
        id: NodeId,
        expected: NodeArenaKind,
    },
    GenerationMismatch {
        expected: ArenaGeneration,
        actual: ArenaGeneration,
    },
    KindMismatch {
        id: NodeId,
        expected: NodeArenaKind,
        actual: NodeArenaKind,
    },
}

/// Parser-local identifier cache backed later by engine string interning.
///
/// The parser owns mutation while tokenizing and building AST nodes. Borrowers
/// receive `ParserIdentifier` handles that stay meaningful only with the parse
/// product that produced them.
#[derive(Debug, Default)]
pub struct IdentifierArena {
    next: u32,
    descriptors: Vec<IdentifierDescriptor>,
    texts: Vec<Option<String>>,
    by_text: HashMap<String, ParserIdentifier>,
}

impl IdentifierArena {
    pub fn reserve_identifier_slot(&mut self) -> ParserIdentifier {
        let id = ParserIdentifier(self.next);
        self.next = self.next.saturating_add(1);
        self.descriptors.push(IdentifierDescriptor {
            source: IdentifierSource::Unknown,
        });
        self.texts.push(None);
        id
    }

    pub fn reserve_identifier(&mut self, source: IdentifierSource) -> ParserIdentifier {
        let id = ParserIdentifier(self.next);
        self.next = self.next.saturating_add(1);
        self.descriptors.push(IdentifierDescriptor { source });
        self.texts.push(None);
        id
    }

    pub fn reserve_identifier_text(
        &mut self,
        source: IdentifierSource,
        text: String,
    ) -> ParserIdentifier {
        if let Some(identifier) = self.by_text.get(&text) {
            return *identifier;
        }
        let id = ParserIdentifier(self.next);
        self.next = self.next.saturating_add(1);
        self.descriptors.push(IdentifierDescriptor { source });
        self.texts.push(Some(text.clone()));
        self.by_text.insert(text, id);
        id
    }

    pub fn descriptor(&self, identifier: ParserIdentifier) -> Option<&IdentifierDescriptor> {
        self.descriptors.get(usize::try_from(identifier.0).ok()?)
    }

    pub fn identifier_text(&self, identifier: ParserIdentifier) -> Option<&str> {
        self.texts
            .get(usize::try_from(identifier.0).ok()?)?
            .as_deref()
    }

    pub fn identifier_texts(&self) -> Vec<(ParserIdentifier, String)> {
        self.texts
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                Some((
                    ParserIdentifier(index.try_into().ok()?),
                    text.as_ref()?.clone(),
                ))
            })
            .collect()
    }

    pub fn identifier_for_text(&self, text: &str) -> Option<ParserIdentifier> {
        self.by_text.get(text).copied()
    }
}

/// Parser-local name identity.
///
/// This is not a runtime identifier or property key. Converting it to an
/// `AstPropertyKey` or interned runtime key belongs at the appropriate
/// parser/runtime string boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ParserIdentifier(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct IdentifierDescriptor {
    pub source: IdentifierSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum IdentifierSource {
    Unknown,
    SourceSlice,
    CookedString,
    RawString,
    NumericLiteral,
    PrivateName,
    WellKnown(WellKnownIdentifier),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WellKnownIdentifier {
    Empty,
    Arguments,
    Eval,
    Constructor,
    Prototype,
    Async,
    Await,
    Yield,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ast::{Expr, Stmt};

    #[test]
    fn arena_ref_validation_accepts_matching_kind() {
        let mut arena = ParserArena::new();
        let expr = arena.reserve_expression();

        assert!(arena
            .validate_ref::<Expr>(expr, NodeArenaKind::Expression)
            .is_valid());
    }

    #[test]
    fn arena_ref_validation_rejects_missing_and_wrong_kind() {
        let mut arena = ParserArena::new();
        let expr = arena.reserve_expression();
        let missing = AstRef::<Stmt>::from_raw_index(7);

        assert_eq!(
            arena
                .validate_ref::<Expr>(expr, NodeArenaKind::Statement)
                .findings,
            vec![AstRefValidationFinding::KindMismatch {
                id: expr.id(),
                expected: NodeArenaKind::Statement,
                actual: NodeArenaKind::Expression,
            }]
        );
        assert_eq!(
            arena
                .validate_ref::<Stmt>(missing, NodeArenaKind::Statement)
                .findings,
            vec![AstRefValidationFinding::MissingNode {
                id: missing.id(),
                expected: NodeArenaKind::Statement,
            }]
        );
    }

    #[test]
    fn arena_stores_and_returns_typed_nodes() {
        let mut arena = ParserArena::new();
        let span = crate::syntax::source::SourceSpan::default();
        let expr = arena.alloc_expression(Expr::Literal(crate::syntax::ast::LiteralExpr {
            span,
            kind: crate::syntax::ast::LiteralKind::Number {
                value: crate::syntax::ast::NumberLiteralValue::Int32(1),
            },
        }));
        let stmt = arena.alloc_statement(Stmt::Expression(expr));

        assert_eq!(
            arena.expression(expr),
            Some(&Expr::Literal(crate::syntax::ast::LiteralExpr {
                span,
                kind: crate::syntax::ast::LiteralKind::Number {
                    value: crate::syntax::ast::NumberLiteralValue::Int32(1),
                },
            }))
        );
        assert_eq!(arena.statement(stmt), Some(&Stmt::Expression(expr)));
        assert!(arena
            .validate_ref::<Stmt>(stmt, NodeArenaKind::Statement)
            .is_valid());
    }

    #[test]
    fn identifier_arena_reuses_source_text_names() {
        let mut identifiers = IdentifierArena::default();
        let first = identifiers.reserve_identifier_text(IdentifierSource::SourceSlice, "x".into());
        let second = identifiers.reserve_identifier_text(IdentifierSource::SourceSlice, "x".into());
        let other = identifiers.reserve_identifier_text(IdentifierSource::SourceSlice, "y".into());

        assert_eq!(first, second);
        assert_ne!(first, other);
        assert_eq!(identifiers.identifier_text(first), Some("x"));
    }
}
