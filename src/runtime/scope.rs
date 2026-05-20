use crate::runtime::state::{
    ModuleRecordId, ObjectId, RuntimeValue, StringId, SymbolCellId, WatchpointGeneration,
};

/// GC-managed scope cell contract.
#[derive(Clone, Debug)]
pub struct Scope {
    /// Parent-linked scope cell.
    ///
    /// The `object` field represents the object observed by resolution and
    /// debugger code, while `environment` names the binding storage contract.
    pub id: Option<ScopeId>,
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    pub object: Option<ObjectId>,
    pub environment: EnvironmentRef,
    pub flags: ScopeFlags,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ScopeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ScopeKind {
    Global,
    Lexical,
    Function,
    Module,
    Eval,
    With,
    Catch,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScopeFlags {
    pub is_var_scope: bool,
    pub is_lexical_scope: bool,
    pub is_module_scope: bool,
    pub is_nested_lexical_scope: bool,
    pub is_function_name_scope: bool,
    pub tainted_by_with_scope: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum EnvironmentRef {
    #[default]
    None,
    Declarative(EnvironmentId),
    Object(ObjectId),
    Global(EnvironmentId),
    Module(EnvironmentId),
    Segmented(EnvironmentId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct EnvironmentId(pub u32);

/// Parent-linked scope-chain view.
#[derive(Clone, Debug, Default)]
pub struct ScopeChain {
    pub head: Option<ScopeId>,
    pub lexical_global: Option<ScopeId>,
    pub depth: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScopeChainNode {
    pub scope: ScopeId,
    pub parent: Option<ScopeId>,
    pub depth_from_head: u32,
}

/// Binding storage abstraction.
///
/// Value stores must go through `BindingSlot` so TDZ/read-only checks and write
/// barriers have one owner-aware boundary.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    /// Declarative binding storage for lexical, function, global, module, eval,
    /// catch, and segmented environments. Resolution may see object scopes, but
    /// value mutation is centralized through `BindingSlot`.
    pub id: Option<EnvironmentId>,
    pub kind: EnvironmentRecordKind,
    pub symbol_table: SymbolTable,
    pub slots: Vec<BindingSlot>,
    pub parent_scope: Option<ScopeId>,
    pub watchpoint_generation: WatchpointGeneration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum EnvironmentRecordKind {
    #[default]
    Declarative,
    Function,
    Object,
    GlobalObject,
    GlobalLexical,
    Module,
    Eval,
    Catch,
    With,
    Segmented,
}

#[derive(Clone, Debug, Default)]
pub struct LexicalEnvironment {
    pub environment: Environment,
    pub initialization_state: EnvironmentInitializationState,
}

#[derive(Clone, Debug, Default)]
pub struct SegmentedEnvironment {
    pub environment: Environment,
    pub segment_count: usize,
    pub segment_capacity: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GlobalLexicalEnvironment {
    pub environment: Environment,
    pub global_object: Option<ObjectId>,
}

#[derive(Clone, Debug, Default)]
pub struct ModuleEnvironment {
    /// Module environments carry the module-record edge separately from normal
    /// lexical bindings so import/export linkage can be tracked independently.
    pub environment: Environment,
    pub module_record: Option<ModuleRecordId>,
    pub import_export_table_generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum EnvironmentInitializationState {
    #[default]
    Allocated,
    Initializing,
    Initialized,
    Published,
}

#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    entries: Vec<SymbolTableEntry>,
    pub scope_kind: Option<ScopeKind>,
    pub capture_count: u32,
    pub watchpoint_generation: WatchpointGeneration,
}

impl SymbolTable {
    pub fn entries(&self) -> &[SymbolTableEntry] {
        &self.entries
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolTableEntry {
    pub symbol: SymbolCellId,
    pub name: Option<StringId>,
    pub offset: ScopeOffset,
    pub attributes: BindingAttributes,
    pub kind: BindingKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BindingAttributes {
    pub read_only: bool,
    pub tdz_protected: bool,
    pub can_delete: bool,
    pub is_const: bool,
    pub is_private_name: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum BindingKind {
    #[default]
    Var,
    Let,
    Const,
    Function,
    Class,
    Import,
    CatchParameter,
    PrivateName,
}

#[derive(Clone, Debug, Default)]
pub struct BindingSlot {
    value: RuntimeValue,
    pub state: BindingState,
    pub owner_environment: Option<EnvironmentId>,
    pub offset: ScopeOffset,
}

impl BindingSlot {
    pub fn value(&self) -> RuntimeValue {
        self.value
    }

    pub fn initialize_placeholder(&mut self, value: RuntimeValue) {
        self.value = value;
    }

    pub fn read(&self, attributes: BindingAttributes) -> Result<RuntimeValue, BindingError> {
        if self.state == BindingState::Deleted {
            return Err(BindingError::Deleted);
        }
        if attributes.tdz_protected && self.state == BindingState::Uninitialized {
            return Err(BindingError::TemporalDeadZone);
        }
        Ok(self.value)
    }

    pub fn initialize(&mut self, value: RuntimeValue, immutable: bool) -> Result<(), BindingError> {
        if self.state != BindingState::Uninitialized {
            return Err(BindingError::AlreadyInitialized);
        }
        self.value = value;
        self.state = if immutable {
            BindingState::Immutable
        } else {
            BindingState::Mutable
        };
        Ok(())
    }

    pub fn set_mutable(
        &mut self,
        value: RuntimeValue,
        attributes: BindingAttributes,
        strict: bool,
    ) -> Result<BindingWriteOutcome, BindingError> {
        if self.state == BindingState::Deleted {
            return Err(BindingError::Deleted);
        }
        if attributes.tdz_protected && self.state == BindingState::Uninitialized {
            return Err(BindingError::TemporalDeadZone);
        }
        if attributes.read_only || matches!(self.state, BindingState::Immutable) {
            return if strict {
                Err(BindingError::ReadOnly)
            } else {
                Ok(BindingWriteOutcome::IgnoredSloppyReadOnly)
            };
        }

        self.value = value;
        self.state = BindingState::Mutable;
        Ok(BindingWriteOutcome::Updated)
    }

    pub fn delete(&mut self, attributes: BindingAttributes) -> BindingDeleteOutcome {
        if self.state == BindingState::Deleted {
            return BindingDeleteOutcome::Missing;
        }
        if !attributes.can_delete {
            return BindingDeleteOutcome::Rejected;
        }
        self.state = BindingState::Deleted;
        BindingDeleteOutcome::Deleted
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum BindingState {
    #[default]
    Uninitialized,
    Initialized,
    Mutable,
    Immutable,
    Deleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BindingError {
    TemporalDeadZone,
    ReadOnly,
    AlreadyInitialized,
    Deleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BindingWriteOutcome {
    Updated,
    IgnoredSloppyReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BindingDeleteOutcome {
    Deleted,
    Missing,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedBinding {
    pub scope: ScopeId,
    pub environment: EnvironmentRef,
    pub offset: ScopeOffset,
    pub attributes: BindingAttributes,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalDeadZoneSet {
    pub binding_count: u32,
    pub private_name_count: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ScopeOffset(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct VarOffset(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_slot_enforces_tdz_and_initialization() {
        let mut slot = BindingSlot::default();
        let attributes = BindingAttributes {
            tdz_protected: true,
            ..BindingAttributes::default()
        };

        assert_eq!(slot.read(attributes), Err(BindingError::TemporalDeadZone));
        assert_eq!(slot.initialize(RuntimeValue::from_i32(4), false), Ok(()));
        assert_eq!(slot.read(attributes), Ok(RuntimeValue::from_i32(4)));
    }

    #[test]
    fn immutable_binding_write_is_strict_error_or_sloppy_ignore() {
        let mut slot = BindingSlot::default();
        slot.initialize(RuntimeValue::from_i32(1), true).unwrap();
        let attributes = BindingAttributes {
            read_only: true,
            ..BindingAttributes::default()
        };

        assert_eq!(
            slot.set_mutable(RuntimeValue::from_i32(2), attributes, false),
            Ok(BindingWriteOutcome::IgnoredSloppyReadOnly)
        );
        assert_eq!(
            slot.set_mutable(RuntimeValue::from_i32(2), attributes, true),
            Err(BindingError::ReadOnly)
        );
    }
}
