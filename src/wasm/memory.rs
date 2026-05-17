//! WebAssembly memory and global wrapper placeholders.
//!
//! Raw memory access, bounds checks, grow semantics, signal handling, and shared
//! memory synchronization are deliberately deferred. The descriptors here name
//! ownership and cache points between Wasm memory, JS wrappers, instances, and
//! JIT/bridge metadata.

use crate::runtime::ObjectId;
use crate::wasm::WasmInstanceId;

/// Stable identity for a Wasm memory object.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmMemoryId(pub u64);

/// Memory index in module memory index space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmMemoryIndex(pub u32);

/// Memory representation mode reserved for later implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmMemoryStyle {
    Deferred,
    BoundsChecked,
    Signaled,
    Shared,
    Memory64,
}

/// GC-owned public memory wrapper reserved for future WebAssembly.Memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmMemoryObject {
    pub id: WasmMemoryId,
    pub object: Option<ObjectId>,
    pub style: WasmMemoryStyle,
    pub sharing: WasmMemorySharing,
    pub address_type: WasmAddressType,
    pub minimum_pages: u32,
    pub maximum_pages: Option<u32>,
}

/// Static memory declaration from a module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmMemoryDescriptor {
    pub index: WasmMemoryIndex,
    pub minimum_pages: u64,
    pub maximum_pages: Option<u64>,
    pub sharing: WasmMemorySharing,
    pub address_type: WasmAddressType,
}

/// Memory sharing mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmMemorySharing {
    Unshared,
    Shared,
}

/// Address width selected by memory declarations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmAddressType {
    I32,
    I64,
}

/// Growth lifecycle for a memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmMemoryGrowthState {
    Stable,
    GrowRequested,
    GrowSucceeded,
    GrowFailed,
    Detached,
}

/// Instance-local cached memory base/size slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmMemoryCacheSlot {
    pub instance: WasmInstanceId,
    pub memory: WasmMemoryId,
    pub index: WasmMemoryIndex,
    pub generation: u64,
    pub style: WasmMemoryStyle,
    pub growth_state: WasmMemoryGrowthState,
}

/// Value category reserved for future Wasm globals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmGlobalKind {
    I32,
    I64,
    F32,
    F64,
    ExternRef,
    FuncRef,
    V128,
    EqRef,
    AnyRef,
    StructRef,
    ArrayRef,
}
