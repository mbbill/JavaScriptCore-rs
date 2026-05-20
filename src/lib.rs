//! Single-crate Rust design skeleton for JavaScriptCore.
//!
//! Components are modules, not crates, so planning can use `pub(crate)` and
//! shared internal contracts without manufacturing crate boundaries.

pub mod api;
pub mod assembler;
pub mod b3;
pub mod builtins;
pub mod bytecode;
pub mod bytecompiler;
pub mod debugger;
pub mod dfg;
pub mod disassembler;
pub mod domjit;
pub mod ftl;
pub mod fuzzilli;
pub mod gc;
pub mod generator;
pub mod inspector;
pub mod interpreter;
pub mod jit;
pub mod llint;
pub mod lol;
pub mod modules;
pub mod object;
pub mod offlineasm;
pub mod platform;
pub mod profiler;
pub mod runtime;
pub mod shell;
pub mod strings;
pub mod syntax;
pub mod tools;
pub mod ucd;
pub mod value;
pub mod vm;
pub mod wasm;
pub mod wtf;
pub mod yarr;
