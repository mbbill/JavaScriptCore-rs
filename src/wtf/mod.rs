//! WTF dependency contracts.
//!
//! JSC assumes WTF containers, strings, threading primitives, reference
//! counting, hashing, and platform abstractions. This module records those
//! assumptions so Rust replacements are explicit.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WtfDependencyKind {
    Vector,
    HashMap,
    StringImpl,
    RefCounted,
    Threading,
    Locking,
    Atomics,
    PlatformMemory,
    Assertions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustReplacementPolicy {
    StandardLibrary,
    CustomEngineType,
    HostPlatformAdapter,
    UnsafeBoundaryRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WtfReplacementContract {
    pub dependency: WtfDependencyKind,
    pub policy: RustReplacementPolicy,
    pub must_match_cpp_layout: bool,
    pub may_allocate: bool,
}
