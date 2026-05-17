//! Typed references and rooting handles for heap-owned cells.
//!
//! These types do not own memory. They express how Rust code is allowed to keep
//! GC things alive or observe them while the heap remains the owner.

use core::fmt;
use core::{marker::PhantomData, ptr::NonNull};

use crate::gc::{HeapId, RootId, WeakId};

/// Typed reference to a heap-owned cell.
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
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HandleScopeId(pub u64);

/// Lexical scope for temporary handles.
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

/// Weak reference cleared by GC weak processing.
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
