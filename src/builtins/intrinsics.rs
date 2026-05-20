/// Arity contract for a privileged intrinsic operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntrinsicArity {
    Fixed(u8),
    Variadic,
}

/// Whether an intrinsic may observe or mutate VM state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntrinsicSafety {
    PureMetadata,
    MayAllocate,
    MayCallHost,
    MayThrow,
}

/// Runtime component that owns a host intrinsic implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntrinsicHostOwner {
    Vm,
    GlobalObject,
    PromiseRuntime,
    ModuleLoader,
    IteratorRuntime,
    RegExpRuntime,
    TypedArrayRuntime,
    ApiBridge,
}

/// Binding phase for an intrinsic hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntrinsicBindingPhase {
    /// The generated name is known, but no callable runtime hook is attached.
    Declared,
    /// The VM has installed identifiers and common builtin names.
    NamesReady,
    /// The runtime owner has attached the callable hook.
    Bound,
}

/// Privileged operation exposed to builtin code.
///
/// Implementations must be attached through VM/runtime entry points so that
/// allocation, exceptions, host reentry, and write barriers remain explicit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BuiltinIntrinsic {
    generated_index: u32,
}

impl BuiltinIntrinsic {
    pub const fn from_generated_index(generated_index: u32) -> Self {
        Self { generated_index }
    }

    pub const fn generated_index(self) -> u32 {
        self.generated_index
    }
}

/// Complete contract for one intrinsic referenced by generated builtin code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinIntrinsicDescriptor {
    intrinsic: BuiltinIntrinsic,
    arity: IntrinsicArity,
    safety: IntrinsicSafety,
    owner: IntrinsicHostOwner,
    phase: IntrinsicBindingPhase,
}

impl BuiltinIntrinsicDescriptor {
    pub const fn new(
        intrinsic: BuiltinIntrinsic,
        arity: IntrinsicArity,
        safety: IntrinsicSafety,
        owner: IntrinsicHostOwner,
        phase: IntrinsicBindingPhase,
    ) -> Self {
        Self {
            intrinsic,
            arity,
            safety,
            owner,
            phase,
        }
    }

    pub const fn intrinsic(self) -> BuiltinIntrinsic {
        self.intrinsic
    }

    pub const fn arity(self) -> IntrinsicArity {
        self.arity
    }

    pub const fn safety(self) -> IntrinsicSafety {
        self.safety
    }

    pub const fn owner(self) -> IntrinsicHostOwner {
        self.owner
    }

    pub const fn phase(self) -> IntrinsicBindingPhase {
        self.phase
    }
}

/// Component that owns immutable intrinsic descriptor metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IntrinsicRegistryOwner {
    #[default]
    BuiltinGenerator,
    RuntimeSubsystems,
    TestFixture,
}

/// Authority allowed to bind host hooks for intrinsic descriptors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IntrinsicRegistryMutationAuthority {
    #[default]
    RuntimeOwnerBinding,
    GeneratedDataRefresh,
}

/// Immutable registry of generated intrinsic descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticIntrinsicRegistry {
    pub owner: IntrinsicRegistryOwner,
    pub mutation_authority: IntrinsicRegistryMutationAuthority,
    pub descriptors: &'static [BuiltinIntrinsicDescriptor],
}

impl StaticIntrinsicRegistry {
    pub const fn descriptors(self) -> &'static [BuiltinIntrinsicDescriptor] {
        self.descriptors
    }

    pub fn descriptor_for_intrinsic(
        self,
        intrinsic: BuiltinIntrinsic,
    ) -> Option<&'static BuiltinIntrinsicDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.intrinsic() == intrinsic)
    }

    pub fn validate(self) -> IntrinsicValidationReport {
        let mut findings = Vec::new();
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            if self.descriptors[..index]
                .iter()
                .any(|candidate| candidate.intrinsic() == descriptor.intrinsic())
            {
                findings.push(IntrinsicValidationFinding::DuplicateIntrinsic {
                    intrinsic: descriptor.intrinsic(),
                });
            }
            if descriptor.phase() == IntrinsicBindingPhase::Bound
                && descriptor.safety() == IntrinsicSafety::PureMetadata
                && descriptor.owner() != IntrinsicHostOwner::Vm
            {
                findings.push(IntrinsicValidationFinding::PureMetadataBoundOutsideVm {
                    intrinsic: descriptor.intrinsic(),
                    owner: descriptor.owner(),
                });
            }
        }
        IntrinsicValidationReport { findings }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntrinsicValidationReport {
    pub findings: Vec<IntrinsicValidationFinding>,
}

impl IntrinsicValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntrinsicValidationFinding {
    DuplicateIntrinsic {
        intrinsic: BuiltinIntrinsic,
    },
    PureMetadataBoundOutsideVm {
        intrinsic: BuiltinIntrinsic,
        owner: IntrinsicHostOwner,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTRINSIC: BuiltinIntrinsic = BuiltinIntrinsic::from_generated_index(1);
    const DESCRIPTOR: BuiltinIntrinsicDescriptor = BuiltinIntrinsicDescriptor::new(
        INTRINSIC,
        IntrinsicArity::Fixed(0),
        IntrinsicSafety::PureMetadata,
        IntrinsicHostOwner::Vm,
        IntrinsicBindingPhase::Declared,
    );

    #[test]
    fn intrinsic_registry_validation_accepts_unique_descriptors() {
        let registry = StaticIntrinsicRegistry {
            owner: IntrinsicRegistryOwner::TestFixture,
            mutation_authority: IntrinsicRegistryMutationAuthority::GeneratedDataRefresh,
            descriptors: &[DESCRIPTOR],
        };

        assert!(registry.validate().is_valid());
    }

    #[test]
    fn intrinsic_registry_validation_reports_duplicates() {
        let registry = StaticIntrinsicRegistry {
            owner: IntrinsicRegistryOwner::TestFixture,
            mutation_authority: IntrinsicRegistryMutationAuthority::GeneratedDataRefresh,
            descriptors: &[DESCRIPTOR, DESCRIPTOR],
        };

        assert_eq!(
            registry.validate().findings,
            vec![IntrinsicValidationFinding::DuplicateIntrinsic {
                intrinsic: INTRINSIC,
            }]
        );
    }
}
