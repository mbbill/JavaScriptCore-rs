//! Object identity and object-owned storage contracts.

use crate::gc::{CellType, GcRef, JsCell, JsCellHeader, StructureId, Trace, TraceCell, Tracer};
use crate::value::JsValue;

use crate::object::{ButterflyHandle, InlineStorage, PropertyOffset};

/// Object-specific header after the common cell header.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct ObjectHeader {
    pub structure_id: StructureId,
    pub butterfly: Option<ButterflyHandle>,
    pub inline_capacity: u16,
    pub flags: ObjectFlags,
    pub storage_epoch: u64,
}

/// Object state flags whose concrete packing belongs to a later layout pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct ObjectFlags(pub u16);

impl ObjectFlags {
    pub const EXTENSIBLE: Self = Self(1 << 0);
    pub const HAS_INDEXED_STORAGE: Self = Self(1 << 1);
    pub const HAS_OUT_OF_LINE_STORAGE: Self = Self(1 << 2);
    pub const MAY_INTERCEPT_ACCESS: Self = Self(1 << 3);

    pub const fn empty() -> Self {
        Self(0)
    }
}

/// JavaScript object cell.
#[derive(Debug)]
#[repr(C)]
pub struct JsObject {
    cell_header: JsCellHeader,
    object_header: ObjectHeader,
    inline_storage: InlineStorage,
}

impl JsObject {
    pub fn new_unpublished(structure_id: StructureId, inline_capacity: u16) -> Self {
        Self {
            cell_header: JsCellHeader {
                structure_id,
                cell_type: CellType::Object,
                ..JsCellHeader::default()
            },
            object_header: ObjectHeader {
                structure_id,
                inline_capacity,
                flags: ObjectFlags::EXTENSIBLE,
                ..ObjectHeader::default()
            },
            inline_storage: InlineStorage::new(inline_capacity as usize, JsValue::undefined()),
        }
    }

    pub fn object_header(&self) -> &ObjectHeader {
        &self.object_header
    }

    pub fn structure_id(&self) -> StructureId {
        self.object_header.structure_id
    }

    pub fn transition_structure(&mut self, new_structure: StructureId) {
        // Shape changes must be coupled with storage migration and watchpoint
        // invalidation by the future object model implementation.
        self.cell_header.structure_id = new_structure;
        self.object_header.structure_id = new_structure;
        self.object_header.storage_epoch = self.object_header.storage_epoch.saturating_add(1);
    }

    pub fn initialize_inline_slot(&mut self, offset: PropertyOffset, value: JsValue) {
        self.inline_storage.initialize_slot(offset, value);
    }

    pub fn set_inline_slot(
        &mut self,
        owner: GcRef<JsCell>,
        offset: PropertyOffset,
        value: JsValue,
    ) {
        let _barrier_kind = self.inline_storage.set_slot(owner, offset, value);
        self.object_header.storage_epoch = self.object_header.storage_epoch.saturating_add(1);
    }

    pub fn attach_butterfly(&mut self, butterfly: ButterflyHandle) {
        self.object_header.butterfly = Some(butterfly);
        self.object_header.flags.0 |= ObjectFlags::HAS_OUT_OF_LINE_STORAGE.0;
        self.object_header.storage_epoch = self.object_header.storage_epoch.saturating_add(1);
    }
}

impl Trace for JsObject {
    fn trace(&self, _tracer: &mut dyn Tracer) {
        // Future implementation visits cell-containing inline slots,
        // butterfly storage, prototype/structure edges, and any rare data.
    }
}

impl TraceCell for JsObject {
    fn cell_header(&self) -> &JsCellHeader {
        &self.cell_header
    }
}
