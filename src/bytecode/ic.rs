use crate::bytecode::code_block::{
    BytecodeIndex, CallSiteIndex, CodeSpecialization, ExecutableHandle, RuntimeSlot,
};
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;
use crate::gc::StructureId;

/// Inline-cache state owned by a linked code block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InlineCacheTable {
    pub property_accesses: Vec<PropertyInlineCache>,
    pub calls: Vec<CallLinkInfo>,
    pub structure_stubs: Vec<StructureStubInfo>,
    pub iteration_modes: Vec<IterationModeMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyInlineCache {
    pub bytecode_index: BytecodeIndex,
    pub kind: PropertyCacheKind,
    pub state: InlineCacheState,
    pub base: Option<VirtualRegister>,
    pub property: PropertyCacheKey,
    pub get_by_id: Option<GetByIdModeMetadata>,
    pub put_by_id: Option<PutByIdModeMetadata>,
    pub watchpoint: Option<RuntimeSlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PropertyCacheKind {
    GetById,
    GetByIdWithThis,
    TryGetById,
    PutById,
    PutByVal,
    InById,
    InByVal,
    DeleteById,
    PrivateName,
    PrivateBrand,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InlineCacheState {
    #[default]
    Unset,
    Monomorphic,
    Polymorphic,
    Megamorphic,
    Disabled,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PropertyCacheKey {
    #[default]
    None,
    Identifier(u32),
    Symbol(u32),
    PrivateName(u32),
    RuntimeValue(VirtualRegister),
}

/// LLInt get-by-id metadata variants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct GetByIdModeMetadata {
    pub mode: GetByIdMode,
    pub structure: Option<StructureId>,
    pub cached_offset: Option<PropertyOffset>,
    pub cached_slot: Option<RuntimeSlot>,
    pub hit_count_for_llint_caching: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum GetByIdMode {
    ProtoLoad,
    #[default]
    Default,
    Unset,
    ArrayLength,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PutByIdModeMetadata {
    pub mode: PutByIdMode,
    pub old_structure: Option<StructureId>,
    pub new_structure: Option<StructureId>,
    pub cached_offset: Option<PropertyOffset>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum PutByIdMode {
    #[default]
    Default,
    Replace,
    Transition,
    Setter,
    CustomAccessor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct PropertyOffset(pub i32);

/// Patchable structure stub metadata used by property inline caches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructureStubInfo {
    pub bytecode_index: BytecodeIndex,
    pub kind: StructureStubKind,
    pub cache_state: InlineCacheState,
    pub code_origin: CodeOrigin,
    pub access_cases: Vec<AccessCaseRef>,
    pub reset_by_gc: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StructureStubKind {
    GetById,
    PutById,
    InById,
    InstanceOf,
    PrivateName,
    ModuleNamespace,
    Proxyable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct AccessCaseRef(pub u32);

/// Call link metadata for data ICs, direct calls, and optimizing tiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLinkInfo {
    pub call_site: CallSiteIndex,
    pub bytecode_index: BytecodeIndex,
    pub call_type: CallType,
    pub mode: CallLinkMode,
    pub specialization: CodeSpecialization,
    pub origin: CodeOrigin,
    pub target: CallTarget,
    pub slow_path_count: u32,
    pub max_argument_count_including_this_for_varargs: u8,
    pub flags: CallLinkFlags,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CallType {
    #[default]
    None,
    Call,
    CallVarargs,
    Construct,
    ConstructVarargs,
    TailCall,
    TailCallVarargs,
    DirectCall,
    DirectConstruct,
    DirectTailCall,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CallLinkMode {
    #[default]
    Init,
    Monomorphic,
    Polymorphic,
    Virtual,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CallTarget {
    #[default]
    Unlinked,
    LastSeenCallee(RuntimeSlot),
    Monomorphic {
        callee: RuntimeSlot,
        code_block: Option<CodeBlockSlot>,
        entrypoint: Option<RuntimeSlot>,
    },
    PolymorphicStub(RuntimeSlot),
    DirectExecutable(ExecutableHandle),
    Virtual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct CodeBlockSlot(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CallLinkFlags {
    pub has_seen_should_repatch: bool,
    pub has_seen_closure: bool,
    pub cleared_by_gc: bool,
    pub cleared_by_virtual: bool,
    pub uses_data_ic: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CallSlot {
    pub callee_or_executable: Option<RuntimeSlot>,
    pub count: u32,
    pub index: u8,
    pub arity_check: ArityCheckMode,
    pub target: Option<RuntimeSlot>,
    pub code_block: Option<CodeBlockSlot>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArityCheckMode {
    #[default]
    MustCheckArity,
    ArityCheckNotRequired,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct IterationModeMetadata {
    pub bytecode_index: BytecodeIndex,
    pub seen_modes: IterationModes,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct IterationModes {
    pub generic: bool,
    pub fast_array: bool,
    pub fast_map: bool,
    pub fast_set: bool,
}
