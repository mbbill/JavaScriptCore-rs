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
