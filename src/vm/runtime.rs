//! VM-wide runtime structures, caches, and host services.

use crate::gc::Root;
use crate::object::Structure;
pub use crate::runtime::GlobalObjectId;
use crate::value::JsValue;

/// VM-owned global object record. The actual object type belongs to object/runtime modules.
#[derive(Clone, Debug)]
pub struct GlobalObjectRecord {
    pub id: GlobalObjectId,
    pub object_value: JsValue,
    pub default_structure_epoch: u64,
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

/// VM-wide caches for structures, executable data, strings, and service state.
#[derive(Debug, Default)]
pub struct RuntimeCaches {
    pub structure_epoch: u64,
    pub executable_cache_epoch: u64,
    pub atom_table_epoch: u64,
    pub property_cache_epoch: u64,
    pub global_object_epoch: u64,
}

/// Global runtime state that must be explicitly rooted or reset by VM teardown.
#[derive(Debug, Default)]
pub struct GlobalRuntimeState {
    globals: Vec<GlobalObjectRecord>,
    active_global: Option<GlobalObjectId>,
}

impl GlobalRuntimeState {
    pub fn globals(&self) -> &[GlobalObjectRecord] {
        &self.globals
    }

    pub fn active_global(&self) -> Option<GlobalObjectId> {
        self.active_global
    }

    pub fn describe_global(&mut self, record: GlobalObjectRecord) {
        self.active_global = Some(record.id);
        self.globals.push(record);
    }
}

/// Host services, watchdogs, timers, microtask hooks, and integration callbacks.
#[derive(Debug, Default)]
pub struct VmServices {
    pub has_watchdog: bool,
    pub has_microtask_hook: bool,
    pub allows_reentrant_host_calls: bool,
    pub allows_vm_inquiry: bool,
}
