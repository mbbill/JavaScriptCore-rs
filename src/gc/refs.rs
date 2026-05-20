//! Typed references and rooting handles for heap-owned cells.
//!
//! These types do not own memory. They express how Rust code is allowed to keep
//! GC things alive or observe them while the heap remains the owner.

use core::fmt;
use core::{marker::PhantomData, ptr::NonNull};

use crate::gc::{HeapId, RootId, WeakId};

/// Typed reference to a heap-owned cell.
///
/// `GcRef` is a non-owning borrow of heap storage. It carries neither rooting
/// authority nor destruction authority; callers must pair it with an active
/// root, handle, barrier, allocation-init token, or collector traversal proof.
#[repr(transparent)]
pub struct GcRef<T: ?Sized> {
    ptr: NonNull<T>,
}

impl<T: ?Sized> Copy for GcRef<T> {}

impl<T: ?Sized> Clone for GcRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> GcRef<T> {
    /// Creates a GC reference from a raw non-null pointer.
    ///
    /// # Safety
    ///
    /// The pointer must refer to a live cell owned by the active `Heap`. The
    /// caller must also prove that the cell is pinned for the duration of this
    /// reference and that an appropriate root, handle, or barrier keeps it live.
    pub unsafe fn from_non_null(ptr: NonNull<T>) -> Self {
        Self { ptr }
    }

    pub fn as_ptr(self) -> *mut T {
        self.ptr.as_ptr()
    }

    pub fn as_non_null(self) -> NonNull<T> {
        self.ptr
    }
}

impl<T: ?Sized> fmt::Debug for GcRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GcRef").field(&self.ptr).finish()
    }
}

/// Opaque handle scope identity.
///
/// This identifies a VM handle-scope record, not a heap cell.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HandleScopeId(pub u64);

/// Opaque handle slot identity owned by the VM handle set.
///
/// Slots borrow references to cells; they do not own cell identity or storage.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HandleSlotId(pub u64);

/// Handle slot membership in the strong-handle list.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HandleSlotState {
    #[default]
    Free,
    Temporary,
    Strong,
}

/// Descriptor for JSC's VM-owned handle set.
///
/// The handle set owns slots. `Handle`, `Root`, and strong references borrow
/// slot identity and must not deallocate slots directly.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HandleSetDescriptor {
    pub heap: HeapId,
    pub strong_slots: Vec<HandleSlotId>,
    pub free_slots: Vec<HandleSlotId>,
    pub protected_global_object_count: usize,
}

/// Lexical scope for temporary handles.
///
/// The lifetime records scoped borrowing authority for handle slots associated
/// with one heap. Dropping the scope ends temporary-root authority; it does not
/// mutate cell storage directly.
#[derive(Debug)]
pub struct HandleScope<'heap> {
    heap: HeapId,
    id: HandleScopeId,
    _scope: PhantomData<&'heap mut ()>,
}

impl<'heap> HandleScope<'heap> {
    pub fn new(heap: HeapId, id: HandleScopeId) -> Self {
        Self {
            heap,
            id,
            _scope: PhantomData,
        }
    }

    pub fn heap(&self) -> HeapId {
        self.heap
    }

    pub fn id(&self) -> HandleScopeId {
        self.id
    }
}

/// Scoped rooted reference usable by Rust runtime code.
///
/// A handle borrows a VM-owned slot for `'heap`. The referenced cell remains
/// owned by `Heap`; mutation of fields must still pass through barrier APIs.
#[derive(Clone, Copy, Debug)]
pub struct Handle<'heap, T: ?Sized> {
    reference: GcRef<T>,
    scope: HandleScopeId,
    _scope: PhantomData<&'heap T>,
}

impl<'heap, T: ?Sized> Handle<'heap, T> {
    pub fn new(reference: GcRef<T>, scope: &HandleScope<'heap>) -> Self {
        Self {
            reference,
            scope: scope.id(),
            _scope: PhantomData,
        }
    }

    pub fn get(self) -> GcRef<T> {
        self.reference
    }

    pub fn scope(self) -> HandleScopeId {
        self.scope
    }
}

/// Long-lived explicit root registered with the heap or VM.
///
/// The root owns registration metadata only. It keeps the borrowed cell
/// discoverable by GC but cannot allocate, move, destroy, or reinterpret it.
#[derive(Debug)]
pub struct Root<T: ?Sized> {
    reference: GcRef<T>,
    id: RootId,
    heap: HeapId,
}

impl<T: ?Sized> Root<T> {
    pub fn new(reference: GcRef<T>, id: RootId, heap: HeapId) -> Self {
        Self {
            reference,
            id,
            heap,
        }
    }

    pub fn get(&self) -> GcRef<T> {
        self.reference
    }

    pub fn id(&self) -> RootId {
        self.id
    }

    pub fn heap(&self) -> HeapId {
        self.heap
    }
}

/// Long-lived strong handle that keeps a handle-set slot on the strong list.
///
/// Strong handles own slot membership in the VM handle set, not the target
/// cell. Clearing or retargeting the slot is handle-set mutation authority.
#[derive(Debug)]
pub struct StrongHandle<T: ?Sized> {
    pub reference: Option<GcRef<T>>,
    pub slot: HandleSlotId,
    pub heap: HeapId,
}

/// Weak reference cleared by GC weak processing.
///
/// Weak references borrow optional reachability information. Only weak
/// processing or an owning weak registry should clear the target in response to
/// liveness; callers must not infer that `WeakId` is a cell identity.
#[derive(Debug)]
pub struct Weak<T: ?Sized> {
    reference: Option<GcRef<T>>,
    id: WeakId,
}

impl<T: ?Sized> Weak<T> {
    pub fn new(reference: GcRef<T>, id: WeakId) -> Self {
        Self {
            reference: Some(reference),
            id,
        }
    }

    pub fn empty() -> Self {
        Self {
            reference: None,
            id: WeakId::default(),
        }
    }

    pub fn get(&self) -> Option<GcRef<T>> {
        self.reference
    }

    pub fn id(&self) -> WeakId {
        self.id
    }

    pub fn clear(&mut self) {
        self.reference = None;
    }
}
