//! DOMJIT contracts.
//!
//! DOMJIT gives embedders a way to expose host-side structure and call
//! knowledge to optimizing tiers. This module records that trust boundary
//! without embedding WebCore or generating host stubs.

use crate::dfg::SpeculatedType;
use crate::jit::{CallBoundaryId, EffectSummary};
use crate::runtime::{HostHookId, ObjectId};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DomJitSignatureId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DomJitAbstractHeapId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DomJitGetterSetterId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DomJitSnippetId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomJitHeapRange {
    Top,
    None,
    Range { begin: u16, end: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomJitAbstractHeap {
    pub id: DomJitAbstractHeapId,
    pub name_ordinal: u32,
    pub parent: Option<DomJitAbstractHeapId>,
    pub children: Vec<DomJitAbstractHeapId>,
    pub range: DomJitHeapRange,
    /// WebCore/embedder registration owns heap hierarchy mutation. Optimizing
    /// tiers may only consume the computed range contract.
    pub computed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomJitValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicateEffectName(&'static str),
    DuplicateSignatureName(&'static str),
    InvalidHeapRange,
    DuplicateHeapId(DomJitAbstractHeapId),
    MissingParentHeap(DomJitAbstractHeapId),
    MissingChildHeap(DomJitAbstractHeapId),
    HeapParentChildMismatch(DomJitAbstractHeapId),
    SignatureArgumentCountMismatch(DomJitSignatureId),
    SignatureEffectMismatch,
    HostHookMissing(DomJitSignatureId),
    GetterSetterSnippetMismatch,
    StructurePlanEmpty,
    EffectSummaryMismatch(DomJitEffect),
}

impl DomJitHeapRange {
    pub const fn validate(self) -> Result<(), DomJitValidationError> {
        match self {
            DomJitHeapRange::Range { begin, end } if begin > end => {
                Err(DomJitValidationError::InvalidHeapRange)
            }
            _ => Ok(()),
        }
    }
}

impl DomJitAbstractHeap {
    pub fn validate_all(heaps: &[Self]) -> Result<(), DomJitValidationError> {
        for (index, heap) in heaps.iter().enumerate() {
            heap.range.validate()?;
            if heaps[index + 1..].iter().any(|other| other.id == heap.id) {
                return Err(DomJitValidationError::DuplicateHeapId(heap.id));
            }
            if let Some(parent) = heap.parent {
                let parent_heap = heaps
                    .iter()
                    .find(|candidate| candidate.id == parent)
                    .ok_or(DomJitValidationError::MissingParentHeap(parent))?;
                if !parent_heap.children.contains(&heap.id) {
                    return Err(DomJitValidationError::HeapParentChildMismatch(heap.id));
                }
            }
            for child in &heap.children {
                let child_heap = heaps
                    .iter()
                    .find(|candidate| candidate.id == *child)
                    .ok_or(DomJitValidationError::MissingChildHeap(*child))?;
                if child_heap.parent != Some(heap.id) {
                    return Err(DomJitValidationError::HeapParentChildMismatch(*child));
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomJitEffect {
    Pure,
    ReadsWorld,
    WritesWorld,
    MayCallScript,
    MayThrow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomJitEffectSet {
    pub reads: DomJitHeapRange,
    pub writes: DomJitHeapRange,
    pub def: DomJitHeapRange,
    pub summary: DomJitEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomJitSemanticEffectSummary {
    pub effects: EffectSummary,
    pub reads: DomJitHeapRange,
    pub writes: DomJitHeapRange,
    pub def: DomJitHeapRange,
    pub must_generate: bool,
}

impl DomJitHeapRange {
    pub const fn is_empty(self) -> bool {
        matches!(self, DomJitHeapRange::None)
    }

    pub const fn may_overlap(self, other: Self) -> bool {
        match (self, other) {
            (DomJitHeapRange::None, _) | (_, DomJitHeapRange::None) => false,
            (DomJitHeapRange::Top, _) | (_, DomJitHeapRange::Top) => true,
            (
                DomJitHeapRange::Range {
                    begin: left_begin,
                    end: left_end,
                },
                DomJitHeapRange::Range {
                    begin: right_begin,
                    end: right_end,
                },
            ) => left_begin <= right_end && right_begin <= left_end,
        }
    }
}

impl DomJitEffectSet {
    pub const fn semantic_summary(self) -> DomJitSemanticEffectSummary {
        let base = match self.summary {
            DomJitEffect::Pure => EffectSummary::pure(),
            DomJitEffect::ReadsWorld => EffectSummary {
                reads_heap: true,
                ..EffectSummary::pure()
            },
            DomJitEffect::WritesWorld => EffectSummary {
                reads_heap: true,
                writes_heap: true,
                ..EffectSummary::pure()
            },
            DomJitEffect::MayCallScript => EffectSummary::for_call(),
            DomJitEffect::MayThrow => EffectSummary {
                may_throw: true,
                may_exit: true,
                ..EffectSummary::pure()
            },
        };

        DomJitSemanticEffectSummary {
            effects: EffectSummary {
                reads_heap: base.reads_heap || !self.reads.is_empty() || !self.def.is_empty(),
                writes_heap: base.writes_heap || !self.writes.is_empty(),
                ..base
            },
            reads: self.reads,
            writes: self.writes,
            def: self.def,
            must_generate: !self.writes.is_empty(),
        }
    }

    pub const fn validate_semantics(self) -> Result<(), DomJitValidationError> {
        match self.summary {
            DomJitEffect::Pure
                if self.reads.is_empty() && self.writes.is_empty() && self.def.is_empty() =>
            {
                Ok(())
            }
            DomJitEffect::ReadsWorld if !self.reads.is_empty() && self.writes.is_empty() => Ok(()),
            DomJitEffect::WritesWorld if !self.writes.is_empty() => Ok(()),
            DomJitEffect::MayCallScript | DomJitEffect::MayThrow => Ok(()),
            effect => Err(DomJitValidationError::EffectSummaryMismatch(effect)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomJitCallSignature {
    pub id: DomJitSignatureId,
    pub hook: HostHookId,
    pub receiver: Option<ObjectId>,
    pub class_info_ordinal: Option<u32>,
    pub result: SpeculatedType,
    pub arguments: Vec<SpeculatedType>,
    pub effect: DomJitEffectSet,
    pub argument_count: u16,
    pub function_without_type_check: Option<CallBoundaryId>,
}

impl DomJitCallSignature {
    pub fn validate(&self) -> Result<(), DomJitValidationError> {
        self.effect.reads.validate()?;
        self.effect.writes.validate()?;
        self.effect.def.validate()?;
        self.effect.validate_semantics()?;
        if self.argument_count as usize != self.arguments.len() {
            return Err(DomJitValidationError::SignatureArgumentCountMismatch(
                self.id,
            ));
        }
        if self.hook == HostHookId::default() && self.function_without_type_check.is_none() {
            return Err(DomJitValidationError::HostHookMissing(self.id));
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomJitGetterSetter {
    pub id: DomJitGetterSetterId,
    pub getter: HostHookId,
    pub setter: Option<HostHookId>,
    pub result: SpeculatedType,
    pub snippet: Option<DomJitSnippetId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomJitCallDomGetterSnippet {
    pub id: DomJitSnippetId,
    pub getter_setter: DomJitGetterSetterId,
    pub require_global_object: bool,
    pub effect: DomJitEffectSet,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DomJitStructurePlan {
    pub signature: Option<DomJitSignatureId>,
    pub getter_setter: Option<DomJitGetterSetterId>,
    pub snippet: Option<DomJitSnippetId>,
    pub requires_watchpoint: bool,
    pub can_inline_load: bool,
    pub can_inline_call: bool,
    /// DOMJIT is an embedder trust boundary: host signatures may describe
    /// effects and types, while DFG/FTL retain authority over inlining.
    pub boundary: Option<CallBoundaryId>,
}

impl DomJitStructurePlan {
    pub fn validate(&self) -> Result<(), DomJitValidationError> {
        if self.signature.is_none() && self.getter_setter.is_none() && self.snippet.is_none() {
            return Err(DomJitValidationError::StructurePlanEmpty);
        }
        if self.snippet.is_some() && self.getter_setter.is_none() {
            return Err(DomJitValidationError::GetterSetterSnippetMismatch);
        }
        if self.can_inline_call && self.boundary.is_none() {
            return Err(DomJitValidationError::HostHookMissing(
                self.signature.unwrap_or_default(),
            ));
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DomJitSchemaOwner {
    #[default]
    EmbedderRegistry,
    DomJitEffectRegistry,
    OptimizingTierConsumer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DomJitRegistryMutationAuthority {
    #[default]
    EmbedderRegistration,
    GeneratedStaticDataRefresh,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticDomJitEffectSchema {
    pub name: &'static str,
    pub effect: DomJitEffect,
    pub reads: DomJitHeapRange,
    pub writes: DomJitHeapRange,
    pub owner: DomJitSchemaOwner,
    pub mutation_authority: DomJitRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticDomJitSignatureSchema {
    pub name: &'static str,
    pub result: SpeculatedType,
    pub arguments: &'static [SpeculatedType],
    pub effect: DomJitEffect,
    pub may_use_host_hook: bool,
    pub owner: DomJitSchemaOwner,
    pub mutation_authority: DomJitRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomJitSchemaRegistry {
    pub effects: &'static [StaticDomJitEffectSchema],
    pub signatures: &'static [StaticDomJitSignatureSchema],
}

impl DomJitSchemaRegistry {
    pub const fn new(
        effects: &'static [StaticDomJitEffectSchema],
        signatures: &'static [StaticDomJitSignatureSchema],
    ) -> Self {
        Self {
            effects,
            signatures,
        }
    }

    pub const fn effects(self) -> &'static [StaticDomJitEffectSchema] {
        self.effects
    }

    pub const fn signatures(self) -> &'static [StaticDomJitSignatureSchema] {
        self.signatures
    }

    pub fn signature_for_name(self, name: &str) -> Option<&'static StaticDomJitSignatureSchema> {
        self.signatures
            .iter()
            .find(|signature| signature.name == name)
    }

    pub fn validate(self) -> Result<(), DomJitValidationError> {
        for (index, effect) in self.effects.iter().enumerate() {
            effect.validate()?;
            if self.effects[index + 1..]
                .iter()
                .any(|other| other.name == effect.name)
            {
                return Err(DomJitValidationError::DuplicateEffectName(effect.name));
            }
        }
        for (index, signature) in self.signatures.iter().enumerate() {
            signature.validate()?;
            if self.signatures[index + 1..]
                .iter()
                .any(|other| other.name == signature.name)
            {
                return Err(DomJitValidationError::DuplicateSignatureName(
                    signature.name,
                ));
            }
            if !self
                .effects
                .iter()
                .any(|effect| effect.effect == signature.effect)
            {
                return Err(DomJitValidationError::SignatureEffectMismatch);
            }
        }

        Ok(())
    }
}

impl StaticDomJitEffectSchema {
    pub fn validate(&self) -> Result<(), DomJitValidationError> {
        if self.name.is_empty() {
            return Err(DomJitValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(DomJitValidationError::EmptyProvenance(self.name));
        }
        self.reads.validate()?;
        self.writes.validate()
    }
}

impl StaticDomJitSignatureSchema {
    pub fn validate(&self) -> Result<(), DomJitValidationError> {
        if self.name.is_empty() {
            return Err(DomJitValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(DomJitValidationError::EmptyProvenance(self.name));
        }

        Ok(())
    }
}

const DOMJIT_NO_ARGUMENTS: &[SpeculatedType] = &[];
const DOMJIT_VALUE_ARGUMENTS: &[SpeculatedType] = &[SpeculatedType::Unknown];

pub const STATIC_DOMJIT_EFFECT_SCHEMAS: &[StaticDomJitEffectSchema] = &[
    StaticDomJitEffectSchema {
        name: "pure",
        effect: DomJitEffect::Pure,
        reads: DomJitHeapRange::None,
        writes: DomJitHeapRange::None,
        owner: DomJitSchemaOwner::DomJitEffectRegistry,
        mutation_authority: DomJitRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust DOMJIT effect schema",
    },
    StaticDomJitEffectSchema {
        name: "reads-world",
        effect: DomJitEffect::ReadsWorld,
        reads: DomJitHeapRange::Top,
        writes: DomJitHeapRange::None,
        owner: DomJitSchemaOwner::DomJitEffectRegistry,
        mutation_authority: DomJitRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust DOMJIT effect schema",
    },
    StaticDomJitEffectSchema {
        name: "writes-world",
        effect: DomJitEffect::WritesWorld,
        reads: DomJitHeapRange::Top,
        writes: DomJitHeapRange::Top,
        owner: DomJitSchemaOwner::DomJitEffectRegistry,
        mutation_authority: DomJitRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust DOMJIT effect schema",
    },
];

pub const STATIC_DOMJIT_SIGNATURE_SCHEMAS: &[StaticDomJitSignatureSchema] = &[
    StaticDomJitSignatureSchema {
        name: "dom-getter",
        result: SpeculatedType::Unknown,
        arguments: DOMJIT_NO_ARGUMENTS,
        effect: DomJitEffect::ReadsWorld,
        may_use_host_hook: true,
        owner: DomJitSchemaOwner::EmbedderRegistry,
        mutation_authority: DomJitRegistryMutationAuthority::EmbedderRegistration,
        provenance: "static Rust DOMJIT signature schema",
    },
    StaticDomJitSignatureSchema {
        name: "dom-setter",
        result: SpeculatedType::Unknown,
        arguments: DOMJIT_VALUE_ARGUMENTS,
        effect: DomJitEffect::WritesWorld,
        may_use_host_hook: true,
        owner: DomJitSchemaOwner::EmbedderRegistry,
        mutation_authority: DomJitRegistryMutationAuthority::EmbedderRegistration,
        provenance: "static Rust DOMJIT signature schema",
    },
];

pub const DOMJIT_SCHEMA_REGISTRY: DomJitSchemaRegistry = DomJitSchemaRegistry::new(
    STATIC_DOMJIT_EFFECT_SCHEMAS,
    STATIC_DOMJIT_SIGNATURE_SCHEMAS,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_domjit_registry_validates() {
        assert_eq!(DOMJIT_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn domjit_signature_rejects_argument_count_mismatch() {
        let signature = DomJitCallSignature {
            id: DomJitSignatureId(1),
            hook: HostHookId(1),
            receiver: None,
            class_info_ordinal: None,
            result: SpeculatedType::Unknown,
            arguments: vec![SpeculatedType::Int32],
            effect: DomJitEffectSet {
                reads: DomJitHeapRange::None,
                writes: DomJitHeapRange::None,
                def: DomJitHeapRange::None,
                summary: DomJitEffect::Pure,
            },
            argument_count: 2,
            function_without_type_check: None,
        };

        assert_eq!(
            signature.validate(),
            Err(DomJitValidationError::SignatureArgumentCountMismatch(
                DomJitSignatureId(1)
            ))
        );
    }

    #[test]
    fn domjit_pure_effect_rejects_hidden_write_range() {
        let effect = DomJitEffectSet {
            reads: DomJitHeapRange::None,
            writes: DomJitHeapRange::Top,
            def: DomJitHeapRange::None,
            summary: DomJitEffect::Pure,
        };

        assert_eq!(
            effect.validate_semantics(),
            Err(DomJitValidationError::EffectSummaryMismatch(
                DomJitEffect::Pure
            ))
        );
    }

    #[test]
    fn domjit_write_effect_must_generate() {
        let effect = DomJitEffectSet {
            reads: DomJitHeapRange::Top,
            writes: DomJitHeapRange::Range { begin: 2, end: 4 },
            def: DomJitHeapRange::None,
            summary: DomJitEffect::WritesWorld,
        };

        assert!(effect.semantic_summary().must_generate);
        assert!(effect
            .writes
            .may_overlap(DomJitHeapRange::Range { begin: 4, end: 6 }));
    }
}
