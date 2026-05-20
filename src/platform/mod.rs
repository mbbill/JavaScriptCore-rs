//! Platform-facing integration records and owned host resources.
//!
//! Most modules describe boundary proofs that a host platform would normally
//! provide to the VM. The W^X executable-memory compartment is the narrow
//! exception: it owns the actual host mapping while keeping raw pointers and
//! syscalls in a private platform backend.

#![deny(unsafe_code)]

pub mod executable_memory;
pub mod executable_memory_compartment;

#[cfg(unix)]
mod unix_executable_memory;
