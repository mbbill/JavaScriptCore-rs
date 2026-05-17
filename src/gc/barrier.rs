//! Owner-aware barrier slots.
//!
//! Writes from a GC-owned object to another GC thing must carry owner context.
//! The barrier algorithm is deferred; these APIs reserve the mutation boundary.

use crate::gc::GcRef;

/// Barrier family selected by the collector and field kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierKind {
    Initialization,
    Store,
    StoreCellValue,
    StoreStructureId,
    AuxiliaryOwner,
    RememberedSet,
    MutatorFence,
}

/// Threshold that determines when a write needs a slow barrier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierThreshold {
    #[default]
    None,
    PossiblyGrey,
    PossiblyBlack,
}

/// Owner edge recorded by a barrier. This deliberately avoids exposing card
/// tables, remembered sets, or snapshot algorithms.
#[derive(Clone, Copy, Debug)]
pub struct BarrierEdge<O: ?Sized, T: ?Sized> {
    pub owner: GcRef<O>,
    pub target: Option<GcRef<T>>,
    pub kind: BarrierKind,
}

/// Remembered-set entry reserved by an inter-generational or incremental write.
#[derive(Clone, Copy, Debug)]
pub struct RememberedSetEntry<O: ?Sized> {
    pub owner: GcRef<O>,
    pub kind: BarrierKind,
}

/// Descriptor for the barrier work a write would enqueue.
#[derive(Clone, Copy, Debug)]
pub struct WriteBarrierPlan<O: ?Sized, T: ?Sized> {
    pub edge: BarrierEdge<O, T>,
    pub threshold: BarrierThreshold,
    pub remembered_set: Option<RememberedSetEntry<O>>,
}

/// Barriered reference field inside a GC-owned object.
#[derive(Debug, Default)]
pub struct WriteBarrier<T: ?Sized> {
    slot: Option<GcRef<T>>,
    initialized: bool,
}

impl<T: ?Sized> WriteBarrier<T> {
    pub fn empty() -> Self {
        Self {
            slot: None,
            initialized: false,
        }
    }

    pub fn get(&self) -> Option<GcRef<T>> {
        self.slot
    }

    pub fn initialize_without_barrier(&mut self, value: Option<GcRef<T>>) {
        // Initialization-only path for unpublished cells. Callers must not use
        // this after the owning cell can be observed by the mutator or GC.
        self.slot = value;
        self.initialized = true;
    }

    pub fn set<O: ?Sized>(
        &mut self,
        owner: GcRef<O>,
        value: Option<GcRef<T>>,
    ) -> BarrierEdge<O, T> {
        // Future implementation performs the selected write barrier here.
        self.slot = value;
        self.initialized = true;
        BarrierEdge {
            owner,
            target: value,
            kind: BarrierKind::Store,
        }
    }

    pub fn set_with_plan<O: ?Sized>(
        &mut self,
        owner: GcRef<O>,
        value: Option<GcRef<T>>,
        threshold: BarrierThreshold,
    ) -> WriteBarrierPlan<O, T> {
        let edge = self.set(owner, value);
        WriteBarrierPlan {
            edge,
            threshold,
            remembered_set: Some(RememberedSetEntry {
                owner,
                kind: BarrierKind::RememberedSet,
            }),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

/// Barriered JavaScript value field.
///
/// Kept generic so `gc` remains independent from the concrete `JsValue`
/// module while still naming the mutation boundary.
#[derive(Clone, Copy, Debug)]
pub struct ValueBarrier<V> {
    value: V,
    initialized: bool,
}

impl<V: Copy> ValueBarrier<V> {
    pub fn new_initial(value: V) -> Self {
        Self {
            value,
            initialized: true,
        }
    }

    pub fn get(&self) -> V {
        self.value
    }

    pub fn initialize_without_barrier(&mut self, value: V) {
        // Initialization-only path for unpublished cells.
        self.value = value;
        self.initialized = true;
    }

    pub fn set<O: ?Sized>(&mut self, _owner: GcRef<O>, value: V) -> BarrierKind {
        // Future implementation inspects cell-containing values and records the
        // owner-to-child edge required by the collector.
        self.value = value;
        self.initialized = true;
        BarrierKind::StoreCellValue
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}
