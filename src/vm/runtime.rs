//! VM-wide runtime structures, caches, and host services.

use crate::gc::{
    CellId, HeapId, Root, RootId, RootKind, RootRecord, RootSetSemanticError, TargetedRootRecord,
    TargetedRootSet,
};
use crate::object::{Structure, StructureDescriptorTable, StructureDescriptorValidationError};
use crate::runtime::ObjectId;
pub use crate::runtime::{GlobalObjectId, HostHookId, ScriptExecutionStatus};
use crate::value::JsValue;
use std::collections::HashSet;

const GLOBAL_ROOT_ID_BASE: u64 = 5_000_000;

/// VM-owned global object record. The actual object type belongs to object/runtime modules.
#[derive(Clone, Debug)]
pub struct GlobalObjectRecord {
    pub id: GlobalObjectId,
    pub object_value: JsValue,
    pub default_structure_epoch: u64,
    pub script_execution_status: ScriptExecutionStatus,
}

/// VM lifetime state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VmLifecycleState {
    #[default]
    Creating,
    Running,
    DrainingMicrotasks,
    Collecting,
    TearingDown,
}

/// Threading and lock authority for runtime-wide mutation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VmMutationAuthority {
    #[default]
    VmEntryThread,
    ApiContextGroup,
    ConcurrentCompilerReadOnly,
    GcThread,
}

/// Canonical structures shared by the VM.
#[derive(Debug, Default)]
pub struct RuntimeStructures {
    object_structure: Option<Root<Structure>>,
    function_structure: Option<Root<Structure>>,
    global_object_structure: Option<Root<Structure>>,
    structure_epoch: u64,
}

impl RuntimeStructures {
    pub fn object_structure(&self) -> Option<&Root<Structure>> {
        self.object_structure.as_ref()
    }

    pub fn set_object_structure(&mut self, structure: Root<Structure>) {
        self.object_structure = Some(structure);
        self.structure_epoch = self.structure_epoch.saturating_add(1);
    }

    pub fn function_structure(&self) -> Option<&Root<Structure>> {
        self.function_structure.as_ref()
    }

    pub fn set_function_structure(&mut self, structure: Root<Structure>) {
        self.function_structure = Some(structure);
        self.structure_epoch = self.structure_epoch.saturating_add(1);
    }

    pub fn global_object_structure(&self) -> Option<&Root<Structure>> {
        self.global_object_structure.as_ref()
    }

    pub fn structure_epoch(&self) -> u64 {
        self.structure_epoch
    }
}

/// Owner of immutable VM descriptor tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmDescriptorOwner {
    Vm,
    Realm,
    GlobalObject,
    HostEmbedding,
    GeneratedStaticData,
}

/// Provenance for VM descriptor tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmDescriptorProvenance {
    RuntimeSchema,
    GeneratedFromCppMetadata,
    HostEmbedding,
}

/// Immutable descriptor for the VM's canonical structure table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmStructureTableDescriptor {
    pub name: &'static str,
    pub owner: VmDescriptorOwner,
    pub provenance: VmDescriptorProvenance,
    pub structures: &'static StructureDescriptorTable,
}

/// Immutable descriptor for a global object slot known to the VM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalObjectDescriptor {
    pub name: &'static str,
    pub id: GlobalObjectId,
    pub execution_status: ScriptExecutionStatus,
    pub default_structure_epoch: u64,
}

/// Targeted root descriptor for one VM-owned global object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalRootDescriptor {
    pub global: GlobalObjectId,
    pub record: TargetedRootRecord,
}

/// Validated targeted-root plan for VM-owned global objects.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GlobalRootPlan {
    descriptors: Vec<GlobalRootDescriptor>,
}

impl GlobalRootPlan {
    fn new(
        heap: HeapId,
        descriptors: Vec<GlobalRootDescriptor>,
    ) -> Result<Self, RootSetSemanticError> {
        let plan = Self { descriptors };
        plan.validate(heap)?;
        Ok(plan)
    }

    pub fn descriptors(&self) -> &[GlobalRootDescriptor] {
        &self.descriptors
    }

    pub fn targeted_records(&self) -> Vec<TargetedRootRecord> {
        self.descriptors
            .iter()
            .map(|descriptor| descriptor.record)
            .collect()
    }

    fn validate(&self, heap: HeapId) -> Result<(), RootSetSemanticError> {
        let mut globals = HashSet::new();
        for descriptor in &self.descriptors {
            let expected = targeted_global_root_record(heap, descriptor.global)?;
            if !globals.insert(descriptor.global) {
                return Err(RootSetSemanticError::DuplicateRoot(expected.root.id));
            }
            if descriptor.record.root != expected.root {
                return Err(RootSetSemanticError::InvalidRootId(
                    descriptor.record.root.id,
                ));
            }
            if descriptor.record.target != expected.target {
                return Err(RootSetSemanticError::InvalidRootTarget {
                    root: descriptor.record.root.id,
                    target: descriptor.record.target,
                });
            }
        }

        TargetedRootSet::from_records(heap, self.targeted_records())?;
        Ok(())
    }
}

pub fn global_root_id(global: GlobalObjectId) -> RootId {
    RootId(GLOBAL_ROOT_ID_BASE.saturating_add(u64::from(global_object_cell(global).0)))
}

fn global_object_cell(global: GlobalObjectId) -> CellId {
    let GlobalObjectId(ObjectId(cell)) = global;
    cell
}

fn targeted_global_root_record(
    heap: HeapId,
    global: GlobalObjectId,
) -> Result<TargetedRootRecord, RootSetSemanticError> {
    let target = global_object_cell(global);
    let record = TargetedRootRecord {
        root: RootRecord {
            id: global_root_id(global),
            kind: RootKind::ExplicitRoot,
            heap,
        },
        target,
    };
    TargetedRootSet::from_records(heap, vec![record])?;
    Ok(record)
}

/// Static service category exposed by VM descriptor tables.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VmServiceKind {
    Watchdog,
    MicrotaskHook,
    ModuleLoader,
    PromiseRejectionTracker,
    UncaughtExceptionReporter,
    ScriptInterrupt,
    RunLoopTimer,
    DeferredWorkTimer,
    ClientData,
}

/// Immutable descriptor for one VM service or host callback slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmServiceDescriptor {
    pub name: &'static str,
    pub kind: VmServiceKind,
    pub hook: Option<HostHookId>,
    pub owner: VmDescriptorOwner,
    pub provenance: VmDescriptorProvenance,
    pub allows_reentry: bool,
}

/// Immutable VM/global/static-service descriptor tables.
///
/// These tables describe existing static metadata. Runtime installation,
/// service registration, rooting, and teardown remain owned by `Vm`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmStaticDescriptorTables {
    pub name: &'static str,
    pub owner: VmDescriptorOwner,
    pub provenance: VmDescriptorProvenance,
    structure_tables: &'static [VmStructureTableDescriptor],
    globals: &'static [GlobalObjectDescriptor],
    services: &'static [VmServiceDescriptor],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VmStaticDescriptorValidationError {
    EmptyName,
    EmptyGlobalName,
    EmptyServiceName,
    DuplicateStructureTableName(&'static str),
    DuplicateGlobal(GlobalObjectId),
    DuplicateServiceKind(VmServiceKind),
    DuplicateServiceHook(HostHookId),
    ServiceRequiresHook(VmServiceKind),
    StructureTable(StructureDescriptorValidationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmStaticDescriptorTablesBuilder {
    name: &'static str,
    owner: VmDescriptorOwner,
    provenance: VmDescriptorProvenance,
    structure_tables: &'static [VmStructureTableDescriptor],
    globals: &'static [GlobalObjectDescriptor],
    services: &'static [VmServiceDescriptor],
}

impl VmStaticDescriptorTablesBuilder {
    pub const fn new(
        name: &'static str,
        owner: VmDescriptorOwner,
        provenance: VmDescriptorProvenance,
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            structure_tables: &[],
            globals: &[],
            services: &[],
        }
    }

    pub const fn structure_tables(
        mut self,
        structure_tables: &'static [VmStructureTableDescriptor],
    ) -> Self {
        self.structure_tables = structure_tables;
        self
    }

    pub const fn globals(mut self, globals: &'static [GlobalObjectDescriptor]) -> Self {
        self.globals = globals;
        self
    }

    pub const fn services(mut self, services: &'static [VmServiceDescriptor]) -> Self {
        self.services = services;
        self
    }

    pub fn build(self) -> Result<VmStaticDescriptorTables, VmStaticDescriptorValidationError> {
        let descriptor = VmStaticDescriptorTables::new(
            self.name,
            self.owner,
            self.provenance,
            self.structure_tables,
            self.globals,
            self.services,
        );
        descriptor.validate()?;
        Ok(descriptor)
    }
}

impl VmStaticDescriptorTables {
    pub const fn new(
        name: &'static str,
        owner: VmDescriptorOwner,
        provenance: VmDescriptorProvenance,
        structure_tables: &'static [VmStructureTableDescriptor],
        globals: &'static [GlobalObjectDescriptor],
        services: &'static [VmServiceDescriptor],
    ) -> Self {
        Self {
            name,
            owner,
            provenance,
            structure_tables,
            globals,
            services,
        }
    }

    /// Returns immutable VM structure table descriptors.
    pub const fn structure_tables(&self) -> &'static [VmStructureTableDescriptor] {
        self.structure_tables
    }

    /// Returns immutable global object descriptors.
    pub const fn globals(&self) -> &'static [GlobalObjectDescriptor] {
        self.globals
    }

    /// Returns immutable service descriptors.
    pub const fn services(&self) -> &'static [VmServiceDescriptor] {
        self.services
    }

    /// Returns one existing service descriptor by table index.
    pub const fn service_at(&self, index: usize) -> Option<&'static VmServiceDescriptor> {
        if index < self.services.len() {
            Some(&self.services[index])
        } else {
            None
        }
    }

    pub fn validate(&self) -> Result<(), VmStaticDescriptorValidationError> {
        validate_vm_static_descriptor_tables(self)
    }
}

pub fn validate_vm_static_descriptor_tables(
    descriptor: &VmStaticDescriptorTables,
) -> Result<(), VmStaticDescriptorValidationError> {
    if descriptor.name.is_empty() {
        return Err(VmStaticDescriptorValidationError::EmptyName);
    }

    let mut structure_names = HashSet::new();
    for structure_table in descriptor.structure_tables {
        structure_table
            .structures
            .validate()
            .map_err(VmStaticDescriptorValidationError::StructureTable)?;
        if !structure_names.insert(structure_table.name) {
            return Err(
                VmStaticDescriptorValidationError::DuplicateStructureTableName(
                    structure_table.name,
                ),
            );
        }
    }

    let mut globals = HashSet::new();
    for global in descriptor.globals {
        if global.name.is_empty() {
            return Err(VmStaticDescriptorValidationError::EmptyGlobalName);
        }
        if !globals.insert(global.id) {
            return Err(VmStaticDescriptorValidationError::DuplicateGlobal(
                global.id,
            ));
        }
    }

    let mut service_kinds = HashSet::new();
    let mut service_hooks = HashSet::new();
    for service in descriptor.services {
        validate_vm_service_descriptor(service)?;
        if !service_kinds.insert(service.kind) {
            return Err(VmStaticDescriptorValidationError::DuplicateServiceKind(
                service.kind,
            ));
        }
        if let Some(hook) = service.hook {
            if !service_hooks.insert(hook) {
                return Err(VmStaticDescriptorValidationError::DuplicateServiceHook(
                    hook,
                ));
            }
        }
    }

    Ok(())
}

pub fn validate_vm_service_descriptor(
    service: &VmServiceDescriptor,
) -> Result<(), VmStaticDescriptorValidationError> {
    if service.name.is_empty() {
        return Err(VmStaticDescriptorValidationError::EmptyServiceName);
    }

    let hook_required = !matches!(service.kind, VmServiceKind::ClientData);
    if hook_required && service.hook.is_none() {
        return Err(VmStaticDescriptorValidationError::ServiceRequiresHook(
            service.kind,
        ));
    }

    Ok(())
}

pub fn find_vm_service_descriptor(
    descriptor: &VmStaticDescriptorTables,
    kind: VmServiceKind,
) -> Option<&'static VmServiceDescriptor> {
    descriptor
        .services
        .iter()
        .find(|service| service.kind == kind)
}

pub fn find_vm_global_descriptor(
    descriptor: &VmStaticDescriptorTables,
    id: GlobalObjectId,
) -> Option<&'static GlobalObjectDescriptor> {
    descriptor.globals.iter().find(|global| global.id == id)
}

pub fn find_vm_structure_table_descriptor(
    descriptor: &VmStaticDescriptorTables,
    name: &str,
) -> Option<&'static VmStructureTableDescriptor> {
    descriptor
        .structure_tables
        .iter()
        .find(|structure_table| structure_table.name == name)
}

/// Selects VM service capability flags from validated static descriptors.
pub fn select_vm_service_capabilities(
    descriptor: &VmStaticDescriptorTables,
) -> Result<VmServices, VmStaticDescriptorValidationError> {
    descriptor.validate()?;

    let mut services = VmServices::default();
    for service in descriptor.services {
        match service.kind {
            VmServiceKind::Watchdog | VmServiceKind::ScriptInterrupt => {
                services.has_watchdog = true;
            }
            VmServiceKind::MicrotaskHook => {
                services.has_microtask_hook = true;
            }
            VmServiceKind::ModuleLoader
            | VmServiceKind::PromiseRejectionTracker
            | VmServiceKind::UncaughtExceptionReporter => {
                services.allows_vm_inquiry = true;
            }
            VmServiceKind::RunLoopTimer => {
                services.has_run_loop_timer = true;
            }
            VmServiceKind::DeferredWorkTimer => {
                services.has_deferred_work_timer = true;
            }
            VmServiceKind::ClientData => {
                services.has_client_data = true;
            }
        }
        services.allows_reentrant_host_calls |= service.allows_reentry;
    }

    Ok(services)
}

/// VM-wide caches for structures, executable data, strings, and service state.
#[derive(Debug, Default)]
pub struct RuntimeCaches {
    pub structure_epoch: u64,
    pub executable_cache_epoch: u64,
    pub atom_table_epoch: u64,
    pub property_cache_epoch: u64,
    pub global_object_epoch: u64,
    pub key_atom_string_cache_epoch: u64,
    pub json_atom_string_cache_epoch: u64,
    pub string_replace_cache_epoch: u64,
    pub string_split_cache_epoch: u64,
    pub regexp_cache_epoch: u64,
    pub intl_cache_epoch: u64,
}

/// Global runtime state that must be explicitly rooted or reset by VM teardown.
#[derive(Debug, Default)]
pub struct GlobalRuntimeState {
    globals: Vec<GlobalObjectRecord>,
    active_global: Option<GlobalObjectId>,
    lifecycle: VmLifecycleState,
    mutation_authority: VmMutationAuthority,
}

impl GlobalRuntimeState {
    pub fn globals(&self) -> &[GlobalObjectRecord] {
        &self.globals
    }

    pub fn active_global(&self) -> Option<GlobalObjectId> {
        self.active_global
    }

    pub fn lifecycle(&self) -> VmLifecycleState {
        self.lifecycle
    }

    pub fn mutation_authority(&self) -> VmMutationAuthority {
        self.mutation_authority
    }

    pub fn describe_global(
        &mut self,
        record: GlobalObjectRecord,
    ) -> Result<(), RootSetSemanticError> {
        let root = targeted_global_root_record(HeapId::default(), record.id)?;
        if self.globals.iter().any(|global| global.id == record.id) {
            return Err(RootSetSemanticError::DuplicateRoot(root.root.id));
        }
        self.active_global = Some(record.id);
        self.globals.push(record);
        Ok(())
    }

    pub fn global_root_plan(&self, heap: HeapId) -> Result<GlobalRootPlan, RootSetSemanticError> {
        let descriptors = self
            .globals
            .iter()
            .map(|global| {
                targeted_global_root_record(heap, global.id).map(|record| GlobalRootDescriptor {
                    global: global.id,
                    record,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        GlobalRootPlan::new(heap, descriptors)
    }
}

/// Host services, watchdogs, timers, microtask hooks, and integration callbacks.
#[derive(Debug, Default)]
pub struct VmServices {
    pub has_watchdog: bool,
    pub has_microtask_hook: bool,
    pub allows_reentrant_host_calls: bool,
    pub allows_vm_inquiry: bool,
    pub has_run_loop_timer: bool,
    pub has_deferred_work_timer: bool,
    pub has_client_data: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{CellId, HeapId, RootSetSemanticError};
    use crate::object::{StructureDescriptorTable, StructureSchemaOwner};

    static STRUCTURES: StructureDescriptorTable =
        StructureDescriptorTable::new("structures", StructureSchemaOwner::VmStructures, &[]);
    static STRUCTURE_TABLES: &[VmStructureTableDescriptor] = &[VmStructureTableDescriptor {
        name: "vm-structures",
        owner: VmDescriptorOwner::Vm,
        provenance: VmDescriptorProvenance::RuntimeSchema,
        structures: &STRUCTURES,
    }];

    #[test]
    fn vm_static_descriptor_builder_accepts_unique_services_and_globals() {
        static GLOBALS: &[GlobalObjectDescriptor] = &[GlobalObjectDescriptor {
            name: "global",
            id: GlobalObjectId(ObjectId(CellId(1))),
            execution_status: ScriptExecutionStatus::Running,
            default_structure_epoch: 0,
        }];
        static SERVICES: &[VmServiceDescriptor] = &[VmServiceDescriptor {
            name: "microtask",
            kind: VmServiceKind::MicrotaskHook,
            hook: Some(HostHookId(1)),
            owner: VmDescriptorOwner::Vm,
            provenance: VmDescriptorProvenance::RuntimeSchema,
            allows_reentry: true,
        }];

        let descriptor = VmStaticDescriptorTablesBuilder::new(
            "vm",
            VmDescriptorOwner::Vm,
            VmDescriptorProvenance::RuntimeSchema,
        )
        .structure_tables(STRUCTURE_TABLES)
        .globals(GLOBALS)
        .services(SERVICES)
        .build()
        .unwrap();

        assert_eq!(descriptor.services().len(), 1);
    }

    #[test]
    fn vm_static_descriptor_rejects_hook_service_without_hook() {
        static SERVICES: &[VmServiceDescriptor] = &[VmServiceDescriptor {
            name: "watchdog",
            kind: VmServiceKind::Watchdog,
            hook: None,
            owner: VmDescriptorOwner::Vm,
            provenance: VmDescriptorProvenance::RuntimeSchema,
            allows_reentry: false,
        }];

        let error = VmStaticDescriptorTables::new(
            "vm",
            VmDescriptorOwner::Vm,
            VmDescriptorProvenance::RuntimeSchema,
            STRUCTURE_TABLES,
            &[],
            SERVICES,
        )
        .validate()
        .unwrap_err();

        assert_eq!(
            error,
            VmStaticDescriptorValidationError::ServiceRequiresHook(VmServiceKind::Watchdog)
        );
    }

    #[test]
    fn vm_descriptor_lookup_finds_services_and_selects_capabilities() {
        static SERVICES: &[VmServiceDescriptor] = &[
            VmServiceDescriptor {
                name: "microtask",
                kind: VmServiceKind::MicrotaskHook,
                hook: Some(HostHookId(1)),
                owner: VmDescriptorOwner::Vm,
                provenance: VmDescriptorProvenance::RuntimeSchema,
                allows_reentry: true,
            },
            VmServiceDescriptor {
                name: "client-data",
                kind: VmServiceKind::ClientData,
                hook: None,
                owner: VmDescriptorOwner::HostEmbedding,
                provenance: VmDescriptorProvenance::HostEmbedding,
                allows_reentry: false,
            },
        ];
        let descriptor = VmStaticDescriptorTables::new(
            "vm",
            VmDescriptorOwner::Vm,
            VmDescriptorProvenance::RuntimeSchema,
            STRUCTURE_TABLES,
            &[],
            SERVICES,
        );

        let service =
            find_vm_service_descriptor(&descriptor, VmServiceKind::MicrotaskHook).unwrap();
        let capabilities = select_vm_service_capabilities(&descriptor).unwrap();

        assert_eq!(service.name, "microtask");
        assert!(capabilities.has_microtask_hook);
        assert!(capabilities.has_client_data);
        assert!(capabilities.allows_reentrant_host_calls);
    }

    fn global_record(cell: u32) -> GlobalObjectRecord {
        GlobalObjectRecord {
            id: GlobalObjectId(ObjectId(CellId(cell))),
            object_value: JsValue::undefined(),
            default_structure_epoch: 0,
            script_execution_status: ScriptExecutionStatus::Running,
        }
    }

    #[test]
    fn global_runtime_state_root_plan_uses_global_cell_identity() {
        let mut state = GlobalRuntimeState::default();
        let first = global_record(42);
        let second = global_record(7);
        state.describe_global(first.clone()).unwrap();
        state.describe_global(second.clone()).unwrap();

        let plan = state.global_root_plan(HeapId(9)).unwrap();
        let descriptors = plan.descriptors();

        assert_eq!(descriptors.len(), 2);
        assert_eq!(descriptors[0].global, first.id);
        assert_eq!(descriptors[0].record.root.id, global_root_id(first.id));
        assert_eq!(descriptors[0].record.target, CellId(42));
        assert_eq!(descriptors[1].global, second.id);
        assert_eq!(descriptors[1].record.root.id, global_root_id(second.id));
        assert_eq!(descriptors[1].record.target, CellId(7));
        assert_ne!(descriptors[0].record.root.id, descriptors[1].record.root.id);
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.record.root.kind == RootKind::ExplicitRoot));
    }

    #[test]
    fn global_runtime_state_rejects_duplicate_and_default_global_targets() {
        let mut state = GlobalRuntimeState::default();
        let global = global_record(11);
        state.describe_global(global.clone()).unwrap();

        assert_eq!(
            state.describe_global(global.clone()),
            Err(RootSetSemanticError::DuplicateRoot(global_root_id(
                global.id
            )))
        );

        let default_global = global_record(0);
        assert_eq!(
            GlobalRuntimeState::default().describe_global(default_global.clone()),
            Err(RootSetSemanticError::InvalidRootTarget {
                root: global_root_id(default_global.id),
                target: CellId::default()
            })
        );
    }
}
