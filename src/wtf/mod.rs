//! WTF dependency contracts.
//!
//! JSC assumes WTF containers, strings, threading primitives, reference
//! counting, hashing, and platform abstractions. This module records those
//! assumptions so Rust replacements are explicit.
//!
//! WTF contracts describe infrastructure ownership only. They must not mint or
//! reinterpret JavaScript heap-cell identity; that remains `gc::CellId`.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WtfDependencyKind {
    Vector,
    Deque,
    HashMap,
    HashSet,
    HashCountedSet,
    SentinelLinkedList,
    SinglyLinkedList,
    DoublyLinkedList,
    BitVector,
    BitSet,
    StringImpl,
    CString,
    ASCIILiteral,
    RefCounted,
    RefPtr,
    SharedTask,
    Threading,
    AutomaticThread,
    ParallelHelperPool,
    Locking,
    CountingLock,
    Atomics,
    PlatformMemory,
    PageBlock,
    TZoneMalloc,
    FastMalloc,
    Assertions,
    PrintStream,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WtfRegistryAuthority {
    /// Replacement policy data is compiled static data.
    #[default]
    StaticReadOnly,
    /// A generated dependency scan may replace the compiled registry.
    GeneratedDependencyScan,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WtfRegistryOwner {
    /// `wtf` owns the Rust replacement policy schema.
    #[default]
    WtfReplacementSchema,
    /// A future generated scan of C++ WTF usage owns the row data.
    GeneratedWtfUsageScan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustReplacementPolicy {
    StandardLibrary,
    CustomEngineType,
    HostPlatformAdapter,
    UnsafeBoundaryRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WtfMutationAuthority {
    /// A single owning Rust object may mutate its private data.
    SingleThreadedOwner,
    /// A unique lock guard owns mutation for protected data.
    LockGuard,
    /// A shared-mode guard may read and only mutate data documented as shared.
    SharedLockGuard,
    /// Atomic operations own mutation for one atomic location.
    AtomicOperation,
    /// VM or heap lifecycle phases own mutation of engine-wide infrastructure.
    VMOrHeapLifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WtfAllocationDomain {
    RustGlobal,
    FastMalloc,
    TZone,
    PageAligned,
    HostPlatform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfReplacementContract {
    pub dependency: WtfDependencyKind,
    pub policy: RustReplacementPolicy,
    pub must_match_cpp_layout: bool,
    pub may_allocate: bool,
    /// Authority required to mutate the replacement object or its storage.
    pub mutation_authority: WtfMutationAuthority,
    /// Allocator domain that owns memory returned by the replacement.
    pub allocation_domain: WtfAllocationDomain,
}

/// Static replacement-policy row for one WTF dependency family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfReplacementPolicyDescriptor {
    pub name: &'static str,
    pub contract: WtfReplacementContract,
    pub owner: WtfRegistryOwner,
}

/// Immutable registry for WTF replacement policies and support contracts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfReplacementPolicyRegistry {
    pub name: &'static str,
    pub authority: WtfRegistryAuthority,
    pub policies: &'static [WtfReplacementPolicyDescriptor],
    pub locks: &'static [WtfLockContract],
    pub threading: &'static [WtfThreadingContract],
    pub containers: &'static [WtfContainerContract],
}

impl WtfReplacementPolicyRegistry {
    pub const fn policies(&self) -> &'static [WtfReplacementPolicyDescriptor] {
        self.policies
    }

    pub const fn locks(&self) -> &'static [WtfLockContract] {
        self.locks
    }

    pub const fn threading(&self) -> &'static [WtfThreadingContract] {
        self.threading
    }

    pub const fn containers(&self) -> &'static [WtfContainerContract] {
        self.containers
    }

    pub fn policy_for(
        &self,
        dependency: WtfDependencyKind,
    ) -> Option<&'static WtfReplacementPolicyDescriptor> {
        self.policies
            .iter()
            .find(|descriptor| descriptor.contract.dependency == dependency)
    }

    pub fn select_replacement(
        &self,
        request: WtfReplacementRequest,
    ) -> Result<WtfReplacementSelection, WtfReplacementSelectionError> {
        self.validate()
            .map_err(WtfReplacementSelectionError::InvalidRegistry)?;
        let policy = self.policy_for(request.dependency).ok_or(
            WtfReplacementSelectionError::MissingPolicy(request.dependency),
        )?;
        if request.requires_cpp_layout && !policy.contract.must_match_cpp_layout {
            return Err(WtfReplacementSelectionError::CppLayoutUnavailable(
                request.dependency,
            ));
        }
        if request.requires_allocation && !policy.contract.may_allocate {
            return Err(WtfReplacementSelectionError::AllocationUnavailable(
                request.dependency,
            ));
        }
        if let Some(domain) = request.allocation_domain {
            if policy.contract.allocation_domain != domain {
                return Err(WtfReplacementSelectionError::AllocationDomainMismatch {
                    dependency: request.dependency,
                    expected: domain,
                    actual: policy.contract.allocation_domain,
                });
            }
        }

        let container = self
            .containers
            .iter()
            .find(|container| container.dependency == request.dependency);

        Ok(WtfReplacementSelection {
            dependency: request.dependency,
            policy,
            container,
        })
    }

    pub fn evaluate_semantics(
        &self,
        request: WtfSemanticRequest,
    ) -> Result<WtfSemanticOutcome, WtfSemanticError> {
        let replacement_request = WtfReplacementRequest::new(request.dependency)
            .allocation(matches!(request.operation, WtfSemanticOperation::Allocate));
        let selection = self
            .select_replacement(replacement_request)
            .map_err(WtfSemanticError::Selection)?;
        let contract = selection.policy.contract;

        if matches!(
            request.operation,
            WtfSemanticOperation::Allocate | WtfSemanticOperation::Deallocate
        ) && !contract.may_allocate
        {
            return Err(WtfSemanticError::AllocationUnavailable(request.dependency));
        }

        let allocation_domain = request
            .allocation_domain
            .unwrap_or(contract.allocation_domain);
        if allocation_domain != contract.allocation_domain {
            return Err(WtfSemanticError::AllocationDomainMismatch {
                dependency: request.dependency,
                expected: contract.allocation_domain,
                actual: allocation_domain,
            });
        }

        let requires_lock = request.lock_protects.is_some()
            || matches!(
                contract.mutation_authority,
                WtfMutationAuthority::LockGuard | WtfMutationAuthority::SharedLockGuard
            );
        if requires_lock {
            let protects = request
                .lock_protects
                .ok_or(WtfSemanticError::MissingLockContract(""))?;
            let lock = self
                .locks
                .iter()
                .find(|lock| lock.protects == protects)
                .ok_or(WtfSemanticError::MissingLockContract(protects))?;
            if request.shared_lock_mode && !lock.supports_shared_mode {
                return Err(WtfSemanticError::SharedLockUnsupported(protects));
            }
            if request.authority != lock.authority
                && !(request.shared_lock_mode
                    && request.authority == WtfMutationAuthority::SharedLockGuard)
            {
                return Err(WtfSemanticError::WrongAuthority {
                    dependency: request.dependency,
                    expected: lock.authority,
                    actual: request.authority,
                });
            }
        } else if request.operation != WtfSemanticOperation::Read
            && request.authority != contract.mutation_authority
        {
            return Err(WtfSemanticError::WrongAuthority {
                dependency: request.dependency,
                expected: contract.mutation_authority,
                actual: request.authority,
            });
        }

        if request.operation == WtfSemanticOperation::AtomicReadModifyWrite
            && request.authority != WtfMutationAuthority::AtomicOperation
        {
            return Err(WtfSemanticError::WrongAuthority {
                dependency: request.dependency,
                expected: WtfMutationAuthority::AtomicOperation,
                actual: request.authority,
            });
        }

        let threading = self.threading.iter().find(|threading| {
            threading_dependency_name(request.dependency) == Some(threading.name)
        });
        if request.operation == WtfSemanticOperation::SpawnHelperThread && threading.is_none() {
            return Err(WtfSemanticError::MissingThreadingContract(
                request.dependency,
            ));
        }

        if request.operation == WtfSemanticOperation::BlockMutator {
            let threading = threading.ok_or(WtfSemanticError::MissingThreadingContract(
                request.dependency,
            ))?;
            if !threading.may_block_mutator {
                return Err(WtfSemanticError::MutatorBlockingUnavailable(
                    request.dependency,
                ));
            }
        }

        Ok(WtfSemanticOutcome {
            dependency: request.dependency,
            operation: request.operation,
            authority: request.authority,
            allocation_domain,
            requires_lock,
            may_block_mutator: threading
                .map(|threading| threading.may_block_mutator)
                .unwrap_or(false),
        })
    }

    pub fn validate(&self) -> Result<(), WtfReplacementValidationError> {
        if self.name.is_empty() {
            return Err(WtfReplacementValidationError::EmptyRegistryName);
        }

        for (index, policy) in self.policies.iter().enumerate() {
            policy.validate()?;
            if self.policies[..index]
                .iter()
                .any(|previous| previous.name == policy.name)
            {
                return Err(WtfReplacementValidationError::DuplicatePolicyName(
                    policy.name,
                ));
            }
            if self.policies[..index]
                .iter()
                .any(|previous| previous.contract.dependency == policy.contract.dependency)
            {
                return Err(WtfReplacementValidationError::DuplicateDependency(
                    policy.contract.dependency,
                ));
            }
        }

        for lock in self.locks {
            lock.validate()?;
        }

        for threading in self.threading {
            threading.validate()?;
        }

        for container in self.containers {
            container.validate()?;
            if !self
                .policies
                .iter()
                .any(|policy| policy.contract.dependency == container.dependency)
            {
                return Err(WtfReplacementValidationError::MissingPolicyForContainer(
                    container.dependency,
                ));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WtfReplacementValidationError {
    EmptyRegistryName,
    EmptyPolicyName,
    EmptyProtectedStateName,
    EmptyThreadingName,
    DuplicatePolicyName(&'static str),
    DuplicateDependency(WtfDependencyKind),
    MissingPolicyForContainer(WtfDependencyKind),
    AllocationDomainMismatch(WtfDependencyKind),
    LockAuthorityMismatch(&'static str),
    ThreadingTaskOwnershipMismatch(&'static str),
    ContainerContractMismatch(WtfDependencyKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfReplacementRequest {
    pub dependency: WtfDependencyKind,
    pub requires_cpp_layout: bool,
    pub requires_allocation: bool,
    pub allocation_domain: Option<WtfAllocationDomain>,
}

impl WtfReplacementRequest {
    pub const fn new(dependency: WtfDependencyKind) -> Self {
        Self {
            dependency,
            requires_cpp_layout: false,
            requires_allocation: false,
            allocation_domain: None,
        }
    }

    pub const fn cpp_layout(mut self, requires_cpp_layout: bool) -> Self {
        self.requires_cpp_layout = requires_cpp_layout;
        self
    }

    pub const fn allocation(mut self, requires_allocation: bool) -> Self {
        self.requires_allocation = requires_allocation;
        self
    }

    pub const fn allocation_domain(mut self, allocation_domain: WtfAllocationDomain) -> Self {
        self.allocation_domain = Some(allocation_domain);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfReplacementSelection {
    pub dependency: WtfDependencyKind,
    pub policy: &'static WtfReplacementPolicyDescriptor,
    pub container: Option<&'static WtfContainerContract>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WtfReplacementSelectionError {
    InvalidRegistry(WtfReplacementValidationError),
    MissingPolicy(WtfDependencyKind),
    CppLayoutUnavailable(WtfDependencyKind),
    AllocationUnavailable(WtfDependencyKind),
    AllocationDomainMismatch {
        dependency: WtfDependencyKind,
        expected: WtfAllocationDomain,
        actual: WtfAllocationDomain,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WtfSemanticOperation {
    Read,
    #[default]
    Mutate,
    Resize,
    Allocate,
    Deallocate,
    SpawnHelperThread,
    BlockMutator,
    AtomicReadModifyWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfSemanticRequest {
    pub dependency: WtfDependencyKind,
    pub operation: WtfSemanticOperation,
    pub authority: WtfMutationAuthority,
    pub allocation_domain: Option<WtfAllocationDomain>,
    pub lock_protects: Option<&'static str>,
    pub shared_lock_mode: bool,
}

impl WtfSemanticRequest {
    pub const fn new(dependency: WtfDependencyKind, operation: WtfSemanticOperation) -> Self {
        Self {
            dependency,
            operation,
            authority: WtfMutationAuthority::SingleThreadedOwner,
            allocation_domain: None,
            lock_protects: None,
            shared_lock_mode: false,
        }
    }

    pub const fn authority(mut self, authority: WtfMutationAuthority) -> Self {
        self.authority = authority;
        self
    }

    pub const fn allocation_domain(mut self, domain: WtfAllocationDomain) -> Self {
        self.allocation_domain = Some(domain);
        self
    }

    pub const fn lock_protects(mut self, protects: &'static str) -> Self {
        self.lock_protects = Some(protects);
        self
    }

    pub const fn shared_lock_mode(mut self, shared_lock_mode: bool) -> Self {
        self.shared_lock_mode = shared_lock_mode;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfSemanticOutcome {
    pub dependency: WtfDependencyKind,
    pub operation: WtfSemanticOperation,
    pub authority: WtfMutationAuthority,
    pub allocation_domain: WtfAllocationDomain,
    pub requires_lock: bool,
    pub may_block_mutator: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WtfSemanticError {
    Selection(WtfReplacementSelectionError),
    WrongAuthority {
        dependency: WtfDependencyKind,
        expected: WtfMutationAuthority,
        actual: WtfMutationAuthority,
    },
    MissingLockContract(&'static str),
    SharedLockUnsupported(&'static str),
    AllocationUnavailable(WtfDependencyKind),
    AllocationDomainMismatch {
        dependency: WtfDependencyKind,
        expected: WtfAllocationDomain,
        actual: WtfAllocationDomain,
    },
    MissingThreadingContract(WtfDependencyKind),
    MutatorBlockingUnavailable(WtfDependencyKind),
}

fn threading_dependency_name(dependency: WtfDependencyKind) -> Option<&'static str> {
    match dependency {
        WtfDependencyKind::ParallelHelperPool => Some("parallel-helper-pool"),
        WtfDependencyKind::AutomaticThread => Some("automatic-thread"),
        _ => None,
    }
}

impl WtfReplacementPolicyDescriptor {
    pub const fn new(
        name: &'static str,
        dependency: WtfDependencyKind,
        policy: RustReplacementPolicy,
    ) -> Self {
        Self {
            name,
            contract: WtfReplacementContract {
                dependency,
                policy,
                must_match_cpp_layout: false,
                may_allocate: false,
                mutation_authority: WtfMutationAuthority::SingleThreadedOwner,
                allocation_domain: WtfAllocationDomain::RustGlobal,
            },
            owner: WtfRegistryOwner::WtfReplacementSchema,
        }
    }

    pub fn validate(&self) -> Result<(), WtfReplacementValidationError> {
        if self.name.is_empty() {
            return Err(WtfReplacementValidationError::EmptyPolicyName);
        }
        self.contract.validate()
    }
}

impl WtfReplacementContract {
    pub fn validate(&self) -> Result<(), WtfReplacementValidationError> {
        if matches!(
            self.allocation_domain,
            WtfAllocationDomain::FastMalloc | WtfAllocationDomain::TZone
        ) && !self.may_allocate
        {
            return Err(WtfReplacementValidationError::AllocationDomainMismatch(
                self.dependency,
            ));
        }
        if self.dependency == WtfDependencyKind::Locking
            && self.mutation_authority != WtfMutationAuthority::LockGuard
        {
            return Err(WtfReplacementValidationError::LockAuthorityMismatch(
                "Locking",
            ));
        }
        if self.dependency == WtfDependencyKind::Atomics
            && self.mutation_authority != WtfMutationAuthority::AtomicOperation
        {
            return Err(WtfReplacementValidationError::LockAuthorityMismatch(
                "Atomics",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfReplacementPolicyDescriptorBuilder {
    descriptor: WtfReplacementPolicyDescriptor,
}

impl WtfReplacementPolicyDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        dependency: WtfDependencyKind,
        policy: RustReplacementPolicy,
    ) -> Self {
        Self {
            descriptor: WtfReplacementPolicyDescriptor::new(name, dependency, policy),
        }
    }

    pub const fn must_match_cpp_layout(mut self, must_match_cpp_layout: bool) -> Self {
        self.descriptor.contract.must_match_cpp_layout = must_match_cpp_layout;
        self
    }

    pub const fn may_allocate(mut self, may_allocate: bool) -> Self {
        self.descriptor.contract.may_allocate = may_allocate;
        self
    }

    pub const fn mutation_authority(mut self, authority: WtfMutationAuthority) -> Self {
        self.descriptor.contract.mutation_authority = authority;
        self
    }

    pub const fn allocation_domain(mut self, domain: WtfAllocationDomain) -> Self {
        self.descriptor.contract.allocation_domain = domain;
        self
    }

    pub const fn owner(mut self, owner: WtfRegistryOwner) -> Self {
        self.descriptor.owner = owner;
        self
    }

    pub fn build(self) -> Result<WtfReplacementPolicyDescriptor, WtfReplacementValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfLockContract {
    /// Borrowed name of the protected state; not a pointer or identity token.
    pub protects: &'static str,
    /// Mutation authority granted while the lock contract is satisfied.
    pub authority: WtfMutationAuthority,
    pub supports_shared_mode: bool,
    pub required_for_resize: bool,
}

impl WtfLockContract {
    pub fn validate(&self) -> Result<(), WtfReplacementValidationError> {
        if self.protects.is_empty() {
            return Err(WtfReplacementValidationError::EmptyProtectedStateName);
        }
        if self.required_for_resize
            && !matches!(
                self.authority,
                WtfMutationAuthority::LockGuard | WtfMutationAuthority::SharedLockGuard
            )
        {
            return Err(WtfReplacementValidationError::LockAuthorityMismatch(
                self.protects,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfThreadingContract {
    pub name: &'static str,
    pub may_spawn_helper_threads: bool,
    pub may_block_mutator: bool,
    /// Whether queued work is retained by reference-counted task ownership.
    pub task_ref_counted: bool,
}

impl WtfThreadingContract {
    pub fn validate(&self) -> Result<(), WtfReplacementValidationError> {
        if self.name.is_empty() {
            return Err(WtfReplacementValidationError::EmptyThreadingName);
        }
        if self.may_spawn_helper_threads && !self.task_ref_counted {
            return Err(WtfReplacementValidationError::ThreadingTaskOwnershipMismatch(self.name));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfContainerContract {
    pub dependency: WtfDependencyKind,
    /// Whether borrowers may keep element addresses across container mutation.
    pub preserves_pointer_stability: bool,
    /// Whether links are stored in borrower-owned elements.
    pub intrusive_links: bool,
    pub hash_deleted_value_required: bool,
    /// Memory owner for container backing storage.
    pub allocation_domain: WtfAllocationDomain,
}

impl WtfContainerContract {
    pub fn validate(&self) -> Result<(), WtfReplacementValidationError> {
        if self.intrusive_links && !self.preserves_pointer_stability {
            return Err(WtfReplacementValidationError::ContainerContractMismatch(
                self.dependency,
            ));
        }
        if self.hash_deleted_value_required
            && !matches!(
                self.dependency,
                WtfDependencyKind::HashMap
                    | WtfDependencyKind::HashSet
                    | WtfDependencyKind::HashCountedSet
            )
        {
            return Err(WtfReplacementValidationError::ContainerContractMismatch(
                self.dependency,
            ));
        }
        Ok(())
    }
}

pub const STATIC_WTF_REPLACEMENT_POLICIES: &[WtfReplacementPolicyDescriptor] = &[
    WtfReplacementPolicyDescriptor {
        name: "Vector",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::Vector,
            policy: RustReplacementPolicy::CustomEngineType,
            must_match_cpp_layout: false,
            may_allocate: true,
            mutation_authority: WtfMutationAuthority::SingleThreadedOwner,
            allocation_domain: WtfAllocationDomain::RustGlobal,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "HashMap",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::HashMap,
            policy: RustReplacementPolicy::CustomEngineType,
            must_match_cpp_layout: false,
            may_allocate: true,
            mutation_authority: WtfMutationAuthority::SingleThreadedOwner,
            allocation_domain: WtfAllocationDomain::RustGlobal,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "SentinelLinkedList",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::SentinelLinkedList,
            policy: RustReplacementPolicy::CustomEngineType,
            must_match_cpp_layout: false,
            may_allocate: false,
            mutation_authority: WtfMutationAuthority::SingleThreadedOwner,
            allocation_domain: WtfAllocationDomain::RustGlobal,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "BitVector",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::BitVector,
            policy: RustReplacementPolicy::CustomEngineType,
            must_match_cpp_layout: false,
            may_allocate: true,
            mutation_authority: WtfMutationAuthority::SingleThreadedOwner,
            allocation_domain: WtfAllocationDomain::RustGlobal,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "Locking",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::Locking,
            policy: RustReplacementPolicy::HostPlatformAdapter,
            must_match_cpp_layout: false,
            may_allocate: false,
            mutation_authority: WtfMutationAuthority::LockGuard,
            allocation_domain: WtfAllocationDomain::HostPlatform,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "Atomics",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::Atomics,
            policy: RustReplacementPolicy::StandardLibrary,
            must_match_cpp_layout: false,
            may_allocate: false,
            mutation_authority: WtfMutationAuthority::AtomicOperation,
            allocation_domain: WtfAllocationDomain::HostPlatform,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "ParallelHelperPool",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::ParallelHelperPool,
            policy: RustReplacementPolicy::HostPlatformAdapter,
            must_match_cpp_layout: false,
            may_allocate: true,
            mutation_authority: WtfMutationAuthority::LockGuard,
            allocation_domain: WtfAllocationDomain::HostPlatform,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "AutomaticThread",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::AutomaticThread,
            policy: RustReplacementPolicy::HostPlatformAdapter,
            must_match_cpp_layout: false,
            may_allocate: true,
            mutation_authority: WtfMutationAuthority::LockGuard,
            allocation_domain: WtfAllocationDomain::HostPlatform,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "FastMalloc",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::FastMalloc,
            policy: RustReplacementPolicy::UnsafeBoundaryRequired,
            must_match_cpp_layout: false,
            may_allocate: true,
            mutation_authority: WtfMutationAuthority::VMOrHeapLifecycle,
            allocation_domain: WtfAllocationDomain::FastMalloc,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
    WtfReplacementPolicyDescriptor {
        name: "TZoneMalloc",
        contract: WtfReplacementContract {
            dependency: WtfDependencyKind::TZoneMalloc,
            policy: RustReplacementPolicy::UnsafeBoundaryRequired,
            must_match_cpp_layout: true,
            may_allocate: true,
            mutation_authority: WtfMutationAuthority::VMOrHeapLifecycle,
            allocation_domain: WtfAllocationDomain::TZone,
        },
        owner: WtfRegistryOwner::WtfReplacementSchema,
    },
];

pub const STATIC_WTF_LOCK_CONTRACTS: &[WtfLockContract] = &[
    WtfLockContract {
        protects: "block-directory",
        authority: WtfMutationAuthority::LockGuard,
        supports_shared_mode: false,
        required_for_resize: true,
    },
    WtfLockContract {
        protects: "weak-registry",
        authority: WtfMutationAuthority::LockGuard,
        supports_shared_mode: false,
        required_for_resize: true,
    },
    WtfLockContract {
        protects: "parallel-helper-pool",
        authority: WtfMutationAuthority::LockGuard,
        supports_shared_mode: false,
        required_for_resize: false,
    },
    WtfLockContract {
        protects: "automatic-thread",
        authority: WtfMutationAuthority::LockGuard,
        supports_shared_mode: false,
        required_for_resize: false,
    },
];

pub const STATIC_WTF_THREADING_CONTRACTS: &[WtfThreadingContract] = &[
    WtfThreadingContract {
        name: "parallel-helper-pool",
        may_spawn_helper_threads: true,
        may_block_mutator: false,
        task_ref_counted: true,
    },
    WtfThreadingContract {
        name: "automatic-thread",
        may_spawn_helper_threads: true,
        may_block_mutator: true,
        task_ref_counted: true,
    },
];

pub const STATIC_WTF_CONTAINER_CONTRACTS: &[WtfContainerContract] = &[
    WtfContainerContract {
        dependency: WtfDependencyKind::Vector,
        preserves_pointer_stability: false,
        intrusive_links: false,
        hash_deleted_value_required: false,
        allocation_domain: WtfAllocationDomain::RustGlobal,
    },
    WtfContainerContract {
        dependency: WtfDependencyKind::SentinelLinkedList,
        preserves_pointer_stability: true,
        intrusive_links: true,
        hash_deleted_value_required: false,
        allocation_domain: WtfAllocationDomain::RustGlobal,
    },
    WtfContainerContract {
        dependency: WtfDependencyKind::HashMap,
        preserves_pointer_stability: false,
        intrusive_links: false,
        hash_deleted_value_required: true,
        allocation_domain: WtfAllocationDomain::RustGlobal,
    },
];

pub const STATIC_WTF_REPLACEMENT_POLICY_REGISTRY: WtfReplacementPolicyRegistry =
    WtfReplacementPolicyRegistry {
        name: "wtf.static-replacement-policy",
        authority: WtfRegistryAuthority::StaticReadOnly,
        policies: STATIC_WTF_REPLACEMENT_POLICIES,
        locks: STATIC_WTF_LOCK_CONTRACTS,
        threading: STATIC_WTF_THREADING_CONTRACTS,
        containers: STATIC_WTF_CONTAINER_CONTRACTS,
    };

pub const fn static_wtf_replacement_policy_registry() -> &'static WtfReplacementPolicyRegistry {
    &STATIC_WTF_REPLACEMENT_POLICY_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_wtf_replacement_registry_is_structurally_valid() {
        assert_eq!(static_wtf_replacement_policy_registry().validate(), Ok(()));
    }

    #[test]
    fn wtf_policy_builder_constructs_fast_malloc_boundary() {
        let descriptor = WtfReplacementPolicyDescriptorBuilder::new(
            "FastMalloc",
            WtfDependencyKind::FastMalloc,
            RustReplacementPolicy::UnsafeBoundaryRequired,
        )
        .may_allocate(true)
        .mutation_authority(WtfMutationAuthority::VMOrHeapLifecycle)
        .allocation_domain(WtfAllocationDomain::FastMalloc)
        .build();

        assert_eq!(
            descriptor.map(|descriptor| descriptor.contract.allocation_domain),
            Ok(WtfAllocationDomain::FastMalloc)
        );
    }

    #[test]
    fn wtf_validator_rejects_fast_malloc_without_allocation() {
        let descriptor = WtfReplacementPolicyDescriptorBuilder::new(
            "bad",
            WtfDependencyKind::FastMalloc,
            RustReplacementPolicy::UnsafeBoundaryRequired,
        )
        .allocation_domain(WtfAllocationDomain::FastMalloc)
        .build();

        assert_eq!(
            descriptor,
            Err(WtfReplacementValidationError::AllocationDomainMismatch(
                WtfDependencyKind::FastMalloc
            ))
        );
    }

    #[test]
    fn wtf_selection_returns_container_policy_when_available() {
        let selection = static_wtf_replacement_policy_registry().select_replacement(
            WtfReplacementRequest::new(WtfDependencyKind::HashMap).allocation(true),
        );

        assert_eq!(
            selection.map(|selection| {
                (
                    selection.policy.contract.policy,
                    selection
                        .container
                        .map(|container| container.hash_deleted_value_required),
                )
            }),
            Ok((RustReplacementPolicy::CustomEngineType, Some(true)))
        );
    }

    #[test]
    fn wtf_selection_rejects_cpp_layout_when_policy_does_not_provide_it() {
        let selection = static_wtf_replacement_policy_registry().select_replacement(
            WtfReplacementRequest::new(WtfDependencyKind::Vector).cpp_layout(true),
        );

        assert_eq!(
            selection,
            Err(WtfReplacementSelectionError::CppLayoutUnavailable(
                WtfDependencyKind::Vector
            ))
        );
    }

    #[test]
    fn wtf_selection_accepts_tzone_layout_boundary() {
        let selection = static_wtf_replacement_policy_registry().select_replacement(
            WtfReplacementRequest::new(WtfDependencyKind::TZoneMalloc)
                .cpp_layout(true)
                .allocation(true)
                .allocation_domain(WtfAllocationDomain::TZone),
        );

        assert_eq!(
            selection.map(|selection| selection.policy.contract.policy),
            Ok(RustReplacementPolicy::UnsafeBoundaryRequired)
        );
    }

    #[test]
    fn wtf_semantics_accepts_fast_malloc_lifecycle_allocation() {
        let outcome = static_wtf_replacement_policy_registry().evaluate_semantics(
            WtfSemanticRequest::new(
                WtfDependencyKind::FastMalloc,
                WtfSemanticOperation::Allocate,
            )
            .authority(WtfMutationAuthority::VMOrHeapLifecycle)
            .allocation_domain(WtfAllocationDomain::FastMalloc),
        );

        assert_eq!(
            outcome.map(|outcome| outcome.allocation_domain),
            Ok(WtfAllocationDomain::FastMalloc)
        );
    }

    #[test]
    fn wtf_semantics_rejects_wrong_allocation_domain() {
        let outcome = static_wtf_replacement_policy_registry().evaluate_semantics(
            WtfSemanticRequest::new(
                WtfDependencyKind::TZoneMalloc,
                WtfSemanticOperation::Allocate,
            )
            .authority(WtfMutationAuthority::VMOrHeapLifecycle)
            .allocation_domain(WtfAllocationDomain::FastMalloc),
        );

        assert_eq!(
            outcome,
            Err(WtfSemanticError::AllocationDomainMismatch {
                dependency: WtfDependencyKind::TZoneMalloc,
                expected: WtfAllocationDomain::TZone,
                actual: WtfAllocationDomain::FastMalloc
            })
        );
    }

    #[test]
    fn wtf_semantics_requires_lock_authority_for_helper_pool() {
        let outcome = static_wtf_replacement_policy_registry().evaluate_semantics(
            WtfSemanticRequest::new(
                WtfDependencyKind::ParallelHelperPool,
                WtfSemanticOperation::SpawnHelperThread,
            )
            .authority(WtfMutationAuthority::SingleThreadedOwner)
            .lock_protects("parallel-helper-pool"),
        );

        assert_eq!(
            outcome,
            Err(WtfSemanticError::WrongAuthority {
                dependency: WtfDependencyKind::ParallelHelperPool,
                expected: WtfMutationAuthority::LockGuard,
                actual: WtfMutationAuthority::SingleThreadedOwner
            })
        );
    }

    #[test]
    fn wtf_semantics_accepts_automatic_thread_mutator_blocking_contract() {
        let outcome = static_wtf_replacement_policy_registry().evaluate_semantics(
            WtfSemanticRequest::new(
                WtfDependencyKind::AutomaticThread,
                WtfSemanticOperation::BlockMutator,
            )
            .authority(WtfMutationAuthority::LockGuard)
            .lock_protects("automatic-thread"),
        );

        assert_eq!(outcome.map(|outcome| outcome.may_block_mutator), Ok(true));
    }
}
