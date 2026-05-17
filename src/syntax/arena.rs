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

    pub fn node_count(&self) -> u32 {
        self.nodes.len()
    }

    pub fn node_descriptor(&self, id: NodeId) -> Option<&NodeDescriptor> {
        self.nodes.descriptor(id)
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
        let index = self.descriptors.len().try_into().unwrap_or(u32::MAX);
        self.descriptors.push(NodeDescriptor { kind });
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
}

/// Parser-local identifier cache backed later by engine string interning.
#[derive(Debug, Default)]
pub struct IdentifierArena {
    next: u32,
    descriptors: Vec<IdentifierDescriptor>,
}

impl IdentifierArena {
    pub fn reserve_identifier_slot(&mut self) -> ParserIdentifier {
        let id = ParserIdentifier(self.next);
        self.next = self.next.saturating_add(1);
        self.descriptors.push(IdentifierDescriptor {
            source: IdentifierSource::Unknown,
        });
        id
    }

    pub fn reserve_identifier(&mut self, source: IdentifierSource) -> ParserIdentifier {
        let id = ParserIdentifier(self.next);
        self.next = self.next.saturating_add(1);
        self.descriptors.push(IdentifierDescriptor { source });
        id
    }

    pub fn descriptor(&self, identifier: ParserIdentifier) -> Option<&IdentifierDescriptor> {
        self.descriptors.get(usize::try_from(identifier.0).ok()?)
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
