//! Structure and shape-transition contracts.

use crate::gc::{
    CellType, GcRef, JsCell, JsCellHeader, StructureId, Trace, TraceCell, Tracer, TypeInfo,
    WriteBarrier,
};

use crate::object::{
    PropertyAttributes, PropertyKey, PropertyOffset, PropertyTable, WatchpointKind, WatchpointSet,
};

/// Indexed storage representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IndexingMode {
    #[default]
    None,
    UndecidedArray,
    Int32,
    Double,
    Contiguous,
    ArrayStorage,
    SlowPutArrayStorage,
    CopyOnWriteInt32,
    CopyOnWriteDouble,
    CopyOnWriteContiguous,
    Dictionary,
    IntegerIndexedExotic,
}

impl IndexingMode {
    pub const fn has_indexed_properties(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn is_copy_on_write(self) -> bool {
        matches!(
            self,
            Self::CopyOnWriteInt32 | Self::CopyOnWriteDouble | Self::CopyOnWriteContiguous
        )
    }

    pub const fn needs_slow_put(self) -> bool {
        matches!(
            self,
            Self::SlowPutArrayStorage | Self::Dictionary | Self::IntegerIndexedExotic
        )
    }
}

/// Shape, prototype, class info, transitions, and watchpoint state.
#[derive(Debug)]
#[repr(C)]
pub struct Structure {
    header: JsCellHeader,
    prototype: WriteBarrier<JsCell>,
    property_table: PropertyTable,
    indexing_mode: IndexingMode,
    watchpoints: WatchpointSet,
    inline_capacity: u16,
    out_of_line_capacity: u16,
    transition_epoch: u64,
    type_info: TypeInfo,
}

impl Structure {
    pub fn new_unpublished(id: StructureId) -> Self {
        Self {
            header: JsCellHeader {
                structure_id: id,
                cell_type: CellType::Structure,
                ..JsCellHeader::default()
            },
            prototype: WriteBarrier::empty(),
            property_table: PropertyTable::new(),
            indexing_mode: IndexingMode::None,
            watchpoints: WatchpointSet::default(),
            inline_capacity: 0,
            out_of_line_capacity: 0,
            transition_epoch: 0,
            type_info: TypeInfo {
                cell_type: CellType::Object,
                ..TypeInfo::default()
            },
        }
    }

    pub fn id(&self) -> StructureId {
        self.header.structure_id
    }

    pub fn property_table(&self) -> &PropertyTable {
        &self.property_table
    }

    pub fn watchpoints(&self) -> &WatchpointSet {
        &self.watchpoints
    }

    pub fn indexing_mode(&self) -> IndexingMode {
        self.indexing_mode
    }

    pub fn inline_capacity(&self) -> u16 {
        self.inline_capacity
    }

    pub fn out_of_line_capacity(&self) -> u16 {
        self.out_of_line_capacity
    }

    pub fn transition_epoch(&self) -> u64 {
        self.transition_epoch
    }

    pub fn type_info(&self) -> TypeInfo {
        self.type_info
    }

    pub fn set_prototype<O: ?Sized>(&mut self, owner: GcRef<O>, prototype: Option<GcRef<JsCell>>) {
        let _edge = self.prototype.set(owner, prototype);
        self.watchpoints.invalidate("prototype changed");
        self.transition_epoch = self.transition_epoch.saturating_add(1);
    }

    pub fn reserve_property_transition(
        &mut self,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) -> PropertyOffset {
        let offset = self.property_table.reserve_transition_slot(key, attributes);
        self.transition_epoch = self.transition_epoch.saturating_add(1);
        self.watchpoints.invalidate("property transition");
        offset
    }

    pub fn describe_transition(&self, transition: StructureTransition) -> StructureTransitionPlan {
        StructureTransitionPlan {
            base: self.id(),
            transition,
            invalidates_watchpoints: matches!(
                transition,
                StructureTransition::ChangePrototype
                    | StructureTransition::DeleteProperty { .. }
                    | StructureTransition::ChangeAttributes { .. }
                    | StructureTransition::EnterDictionaryMode
                    | StructureTransition::ChangeIndexingMode(_)
            ),
        }
    }

    pub fn start_watchpoints(&mut self, kind: WatchpointKind) {
        self.watchpoints.start_watching(kind);
    }
}

impl Trace for Structure {
    fn trace(&self, tracer: &mut dyn Tracer) {
        if let Some(prototype) = self.prototype.get() {
            tracer.visit_cell(prototype);
        }
    }
}

impl TraceCell for Structure {
    fn cell_header(&self) -> &JsCellHeader {
        &self.header
    }
}

/// Add/delete/attribute/prototype transition descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructureTransition {
    AddProperty {
        key: PropertyKey,
        attributes: PropertyAttributes,
    },
    DeleteProperty {
        key: PropertyKey,
    },
    ChangeAttributes {
        key: PropertyKey,
        attributes: PropertyAttributes,
    },
    ChangePrototype,
    EnterDictionaryMode,
    ChangeIndexingMode(IndexingMode),
}

/// Planned transition away from a structure. The new structure allocation and
/// storage migration are separate responsibilities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructureTransitionPlan {
    pub base: StructureId,
    pub transition: StructureTransition,
    pub invalidates_watchpoints: bool,
}
