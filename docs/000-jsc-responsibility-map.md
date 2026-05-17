# JavaScriptCore Responsibility Map

This document is the first design artifact for the Rust JavaScriptCore rewrite.
It maps JavaScriptCore by subsystem responsibility before proposing Rust modules
or implementation tasks.

The goal is to understand what JavaScriptCore owns, mutates, and assumes. The
Rust design should follow those responsibilities, not the current C++ file
layout.

This is not an implementation plan. It is the input to later design documents:

- `001-rust-architecture-principles.md`
- `002-ownership-and-gc-model.md`
- `003-module-layout.md`

## High-Level Shape

JavaScriptCore is organized around these major responsibility groups:

1. Source and parser frontend
2. AST, parser arena, and early semantic analysis
3. Bytecode generation and bytecode schema
4. Unlinked and linked code
5. Interpreter entry, call frames, and VM execution state
6. `JSValue`, `JSCell`, objects, structures, and storage
7. Names, strings, symbols, and property keys
8. Global object, scopes, and lexical environments
9. Functions, executables, and code specialization
10. Exceptions and VM-side control state
11. GC heap, roots, write barriers, weak references, and finalization
12. Modules and host loading
13. Builtins
14. Public embedding API
15. JIT tiers, deferred for the first Rust design
16. Wasm, deferred for the first Rust design

The initial Rust rewrite should not try to make a small JavaScript program run
before these groups have explicit ownership and interface contracts.

## Dependency Flow

At a simplified level, the engine flows like this:

```text
SourceProvider / SourceCode
  -> Lexer
  -> Parser
  -> AST / SyntaxChecker
  -> BytecodeGenerator
  -> UnlinkedCodeBlock / UnlinkedFunctionExecutable
  -> CodeBlock / ExecutableBase / FunctionExecutable
  -> Interpreter / LLInt / JIT entrypoints
  -> Runtime objects / VM / Heap / GC
```

The runtime object model and GC are not leaf dependencies. They feed back into
nearly every stage:

- The parser interns identifiers using runtime identifier/string machinery.
- The bytecode generator creates runtime-facing constants, scopes, and
  executables.
- `CodeBlock` is a GC-managed cell.
- Call frames and bytecode metadata are visible to the GC and JIT.
- Write barriers and watchpoints affect correctness, not only performance.

## 1. Source And Parser Frontend

Reviewed source includes:

- `parser/Lexer.h`
- `parser/Lexer.cpp`
- `parser/Parser.h`
- `parser/Parser.cpp`
- `parser/ParserModes.h`
- `parser/ParserTokens.h`
- `parser/SourceCode.h`
- `parser/SourceProvider.h`

### Responsibility

The lexer and parser convert source text into either AST nodes or syntax-check
results. The lexer is specialized by source encoding, with `Lexer<T>` handling
Latin-1 or UTF-16 source. It tokenizes JavaScript, tracks line terminators,
string/template/regexp literal details, token spans, source URLs, and lexer
errors.

The parser is a recursive-descent parser over a lexer specialization. It handles
source modes, strict mode, module mode, eval and function parsing, declaration
collection, labels, and early feature tracking.

### Owns And Mutates

The lexer owns cursor state into source storage, current token state, token flags,
line state, lexer errors, temporary string buffers, and identifier lookup through
the parser identifier arena.

The parser owns current-token state, parser savepoints, a scope stack, declaration
lists, label state, closed-variable candidates, code feature flags, and
parser-mode state.

### Hidden Invariants

- `Parser<Lexer<T>>` is specialized by source encoding.
- Lexer cursors point into `SourceProvider` storage, so source lifetime and
  parser lifetime are coupled.
- Parser save and restore are semantic, not just syntactic. Ambiguous grammar
  paths re-lex tokens and preserve line-terminator behavior.
- `Parser::Scope` performs early semantic analysis. Sloppy hoisting and captured
  variable marking are order-sensitive.

### Rust Design Pressure

Rust should make source ownership explicit. A parser should borrow source storage
through a stable source object and should not let tokens outlive their source.

The parser should keep the tree-builder abstraction that lets the same parser
drive AST construction or syntax checking. That boundary is useful and should
not be lost in a direct C++ translation.

## 2. AST, Parser Arena, And Early Semantic Analysis

Reviewed source includes:

- `parser/ASTBuilder.h`
- `parser/SyntaxChecker.h`
- `parser/Nodes.h`
- `parser/Nodes.cpp`
- `parser/NodesAnalyzeModule.cpp`
- `parser/ParserArena.h`
- `parser/VariableEnvironment.h`
- `parser/ModuleAnalyzer.h`

### Responsibility

AST nodes represent parsed syntax and semantic metadata. JSC also supports
AST-less syntax checking through a tree-builder interface shared with AST
construction.

The parser arena provides parser-product lifetime storage. AST nodes and some
parser helper objects are arena-allocated.

### Owns And Mutates

AST construction owns arena-allocated nodes, source divots, function metadata,
var and lexical declaration data, feature bits, and constant counts.

`ParserArena` owns bump-allocated memory pools, tracked deletable objects, and an
`IdentifierArena`. `IdentifierArena` owns parser-local identifier values and
short/recent identifier caches.

### Hidden Invariants

- AST nodes emit bytecode through virtual `emitBytecode` methods implemented in
  `bytecompiler/NodesCodegen.cpp`. Parsing and bytecode generation are therefore
  coupled in C++.
- `ParserArenaFreeable` destructors are not run. Arena lifetime is a bulk
  lifetime, not normal C++ object ownership.
- Parser products often carry early semantic decisions used later by bytecode
  generation.

### Rust Design Pressure

Rust should not model AST ownership as ordinary recursive `Box` ownership if the
design wants JSC-like arena lifetime behavior. A parser arena with explicit
lifetimes is likely the right model.

The Rust design should decide whether to preserve AST-node-driven codegen or
introduce a separate lowered IR. A lowered IR would make ownership boundaries
cleaner, but it must preserve source-position and debug metadata.

## 3. Bytecode Generation And Bytecode Schema

Reviewed source includes:

- `bytecompiler/BytecodeGenerator.h`
- `bytecompiler/BytecodeGenerator.cpp`
- `bytecompiler/BytecodeGeneratorBase.h`
- `bytecompiler/NodesCodegen.cpp`
- `bytecode/BytecodeList.rb`
- `bytecode/Instruction.h`
- `bytecode/InstructionStream.h`
- `bytecode/Opcode.h`
- `bytecode/OpcodeSize.h`
- `bytecode/VirtualRegister.h`
- `bytecode/UnlinkedCodeBlockGenerator.h`

### Responsibility

The bytecode generator traverses AST nodes and emits bytecode, constants,
register allocation data, labels, exception handlers, lexical scope data,
function executables, TDZ/private-name data, and metadata used by later runtime
and JIT stages.

`BytecodeList.rb` is effectively the bytecode schema. It defines opcodes,
operand layouts, checkpoints, metadata, and generated tables.

### Owns And Mutates

`BytecodeGenerator` owns an `UnlinkedCodeBlockGenerator`, instruction writer,
register allocator state, label and control-flow stacks, scope stacks, private
name and TDZ stacks, function initialization lists, and source metadata.

The instruction writer mutates raw instruction-stream bytes and jump targets
before finalization.

### Hidden Invariants

- Opcode order, operand width, checkpoint placement, metadata alignment, and
  wide-prefix handling are coupled to generated C++ and LLInt assembly.
- `InstructionStream` is raw byte storage decoded by typed casts.
- `VirtualRegister` encoding is fundamental: locals are negative,
  arguments/header slots are nonnegative, and constants start at
  `FirstConstantRegisterIndex`.
- Bytecode generation has runtime dependencies: `JSValue`, `JSFunction`,
  scopes, executables, GC barriers, module data, and code cache infrastructure.

### Rust Design Pressure

The Rust design should keep a generated bytecode schema equivalent to
`BytecodeList.rb` as the single source of truth.

An early decision is required:

- Define a typed Rust bytecode IR first, then lower to a packed representation.
- Directly emit packed bytecode.

Direct packed emission requires exact operand-width and metadata contracts. A
typed IR is safer for a Rust rewrite, but it must not hide decisions that the VM,
GC, and future JIT tiers need.

## 4. Unlinked And Linked Code

Reviewed source includes:

- `bytecode/UnlinkedCodeBlock.h`
- `bytecode/UnlinkedCodeBlock.cpp`
- `bytecode/UnlinkedFunctionCodeBlock.h`
- `bytecode/UnlinkedProgramCodeBlock.h`
- `bytecode/UnlinkedEvalCodeBlock.h`
- `bytecode/UnlinkedModuleProgramCodeBlock.h`
- `bytecode/CodeBlock.h`
- `bytecode/CodeBlock.cpp`
- `bytecode/FunctionCodeBlock.h`
- `bytecode/ProgramCodeBlock.h`
- `bytecode/EvalCodeBlock.h`
- `bytecode/ModuleProgramCodeBlock.h`
- `runtime/CodeCache.cpp`

### Responsibility

JSC separates source-derived, runtime-independent code from runtime-linked code.

`UnlinkedCodeBlock` stores bytecode and metadata that can be reused or linked in
a specific runtime context. `CodeBlock` links that bytecode to runtime state:
global object, constants, scopes, metadata tables, inline caches, exception
handlers, profiling data, and JIT code references.

### Owns And Mutates

Unlinked code owns instruction streams, identifiers, constants, metadata,
exception tables, source mapping data, and function executable references.

Linked `CodeBlock` owns resolved constants/functions, global object linkage,
runtime metadata, inline cache state, profiling counters, exception handlers,
and JIT code references.

### Hidden Invariants

- `CodeBlock` is itself a `JSCell` and therefore GC-managed.
- `CodeBlock::finishCreation` must not introduce new control flow or locals
  after certain analyses because liveness and baseline data depend on the
  unlinked bytecode shape.
- `CodeBlock` is a central coupling point for interpreter, LLInt, Baseline,
  DFG, FTL, profiler, inline caches, and GC.

### Rust Design Pressure

The Rust rewrite should preserve the unlinked/linked split. It is the natural
place to separate parser/bytecode products from runtime-owned execution state.

Even if JIT is deferred, the Rust design should keep a `CodeBlock`-equivalent
execution object that can later carry profiling, inline cache, and tiering data
without changing interpreter semantics.

## 5. Interpreter Entry, Call Frames, And VM Execution State

Reviewed source includes:

- `interpreter/Interpreter.h`
- `interpreter/Interpreter.cpp`
- `interpreter/CallFrame.h`
- `interpreter/Register.h`
- `interpreter/ProtoCallFrame.h`
- `interpreter/VMEntryRecord.h`
- `interpreter/FrameTracers.h`
- `runtime/VM.h`
- `runtime/VMEntryScope.h`
- `llint/LLIntEntrypoint.cpp`
- `llint/LLIntData.cpp`
- `llint/LowLevelInterpreter.asm`

### Responsibility

The interpreter layer prepares execution for programs, eval, calls, and
constructors. It initializes executable and code-block state, builds proto call
frames, manages VM entry scope, and enters LLInt, JIT, or native entrypoints.

The call-frame layer defines the ABI-visible stack layout used by interpreter,
LLInt, JIT, stack walking, exception handling, and GC.

### Owns And Mutates

Interpreter entry mutates VM entry state, proto call frame headers and
arguments, top call-frame state, and pending exception state.

Call frames own stack slots, callee/codeblock/header fields, call-site index,
return PC/current bytecode PC, and links to caller frames or entry frames.

The VM owns top-frame bookkeeping such as `topCallFrame`, `topEntryFrame`, and
entry records.

### Hidden Invariants

- `CallFrame` layout is ABI-coupled to OfflineASM constants.
- `VM::topCallFrame`, `topEntryFrame`, and `VMEntryRecord` adjacency are assumed
  by assembly sequences.
- LLInt slow paths depend on VM exceptions, GC barriers, inline caches, metadata,
  profiling, and runtime object layout.
- The execution boundary is not just a Rust function call. It is a stack and VM
  state protocol.

### Rust Design Pressure

The frame and VM-entry boundary is one of the hardest rewrite surfaces. Rewriting
only the frontend can be done independently; rewriting interpreter dispatch or
call frames likely requires moving LLInt entry, frame layout, and VM top-frame
bookkeeping together.

For a Rust-first engine, design `CallFrame`, `Register`, and `VMEntry` as one
contract, not as separate modules discovered during implementation.

## 6. `JSValue`, `JSCell`, Objects, Structures, And Storage

Reviewed source includes:

- `runtime/JSCJSValue.h`
- `runtime/EncodedValueDescriptor.h`
- `runtime/JSCell.h`
- `runtime/JSCellInlines.h`
- `runtime/JSObject.h`
- `runtime/JSObjectInlines.h`
- `runtime/Structure.h`
- `runtime/Structure.cpp`
- `runtime/StructureID.h`
- `runtime/StructureTransitionTable.h`
- `runtime/StructureRareData.h`
- `runtime/Butterfly.h`
- `runtime/ButterflyInlines.h`
- `runtime/PropertyTable.h`
- `runtime/PropertyOffset.h`
- `runtime/PropertySlot.h`
- `runtime/PutPropertySlot.h`

### Responsibility

`JSValue` is the tagged value transport for JavaScript values. It represents
immediates and references to GC-managed cells.

`JSCell` is the common GC header and dynamic type root for heap values.

`JSObject` represents object identity and storage access. `Structure` represents
shape, prototype, class info, property offsets, dictionary state, transitions,
and watchpoints. `Butterfly` is the out-of-line storage layout for properties
and indexed elements.

### Owns And Mutates

`JSValue` owns no heap object. It owns only encoded bits.

`JSCell` owns `StructureID`, type info, inline flags, indexing flags, and
`CellState`. Some of these fields use atomic or fence-sensitive mutation.

`JSObject` owns inline storage and a butterfly pointer. It mutates properties,
indexed elements, and storage capacity.

`Structure` owns prototype, global object or realm linkage, class info,
property-offset metadata, transition tables, property tables, dictionary state,
and watchpoints.

`Butterfly` owns out-of-line property storage plus indexed payload and indexed
length/capacity header state.

### Hidden Invariants

- On 64-bit, `JSValue` uses a representation-sensitive encoding. JIT, LLInt, and
  ABI code depend on exact bits and pointer alignment.
- `JSCell` header layout and offsets are visible to interpreter/JIT code.
- `m_indexingTypeAndMisc` has constrained mutation rules after a cell escapes.
- Cell locking has ordering constraints: cell lock before structure lock.
- Shape and storage move together. Adding a property can transition `Structure`,
  grow butterfly storage, then write a slot.
- Watchpoints are correctness mechanisms for optimized code, not just cache
  invalidation.
- `Butterfly` is pointer-arithmetic-based. Property storage lives before indexed
  data relative to the butterfly pointer.

### Rust Design Pressure

If any binary compatibility with C++ or JIT code is required, `JSValue` should be
a `repr(transparent)` raw word with narrow safe constructors, not a high-level
Rust enum.

The object model should separate:

- value representation
- GC cell header
- object identity
- object storage
- shape/structure
- property table

Unsafe layout and pointer arithmetic should be isolated in small modules. Normal
runtime code should use typed accessors that enforce write barriers and shape
contracts.

## 7. Names, Strings, Symbols, And Property Keys

Reviewed source includes:

- `runtime/Identifier.h`
- `runtime/IdentifierInlines.h`
- `runtime/CommonIdentifiers.h`
- `runtime/PropertyName.h`
- `runtime/JSString.h`
- `runtime/Symbol.h`
- `runtime/SymbolTable.h`
- `runtime/CacheableIdentifier.h`

### Responsibility

JSC uses uniqued strings and symbols as the basis for property names,
identifiers, and many caches. Property keys are often represented by identity of
`UniquedStringImpl` or symbol/private-name implementations.

`JSString` represents runtime strings and may hold rope state. `Symbol` represents
public symbols, private names, and well-known symbols.

### Owns And Mutates

Identifiers and property names own or reference uniqued string/symbol identity.
`JSString` owns string or rope representation. Symbol-related types own symbol
identity and private-name state.

### Hidden Invariants

- `Identifier::fromString` can discard symbol-ness; `fromUid` preserves it.
- `PropertyName` is pointer-sized for JIT use.
- `JSString` uses representation tricks, including rope flags in low pointer
  bits and single-load concurrency-sensitive accessors.

### Rust Design Pressure

Rust should make string keys and symbol keys distinct at the type level, with an
explicit path for "maybe symbol" identifiers. This avoids accidentally losing
symbol/private-name identity during property lookup.

String and identifier interning should be designed before parser or object
property implementation begins.

## 8. Global Object, Scopes, And Lexical Environments

Reviewed source includes:

- `runtime/JSGlobalObject.h`
- `runtime/JSGlobalObject.cpp`
- `runtime/JSGlobalObjectInlines.h`
- `runtime/JSScope.h`
- `runtime/JSSymbolTableObject.h`
- `runtime/JSLexicalEnvironment.h`
- `runtime/JSGlobalLexicalEnvironment.h`
- `runtime/JSSegmentedVariableObject.h`
- `runtime/JSModuleEnvironment.h`
- `runtime/SymbolTable.h`
- `runtime/SymbolTableInlines.h`

### Responsibility

`JSGlobalObject` is both object and realm root. It owns or reaches global state:
global this, global lexical environment, constructors, prototypes, structures,
builtin caches, symbol tables, watchpoints, and host method tables.

Scopes and lexical environments store bindings and support variable lookup,
direct variable access, TDZ behavior, eval semantics, and module environments.

### Owns And Mutates

`JSGlobalObject` owns global runtime objects, builtin constructor/prototype
state, structure caches, symbol table cache, global lexical environment, and
many watchpoints.

`JSLexicalEnvironment` stores variables inline after the cell.
`JSSegmentedVariableObject` preserves stable variable addresses after creation.
`SymbolTable` maps names to compact or expanded entries, offsets, attributes,
private names, and watchpoints.

### Hidden Invariants

- `globalScope()` returns the global lexical environment.
- Global var/function binding can use a symbol table entry or `putDirect`,
  depending on global/eval context.
- Scope variable storage needs stable addresses for direct access and JIT use.
- `SymbolTableEntry` is a compact state machine that can inflate to carry shared
  watchpoints.

### Rust Design Pressure

Scope storage should be pinned or otherwise address-stable if the design keeps
direct variable access. Symbol table entries should be modeled as explicit state,
not as a loose map from names to values.

The global object should be treated as a realm root with explicit ownership of
intrinsics, structures, and host hooks.

## 9. Functions, Executables, And Code Specialization

Reviewed source includes:

- `runtime/JSFunction.h`
- `runtime/JSFunction.cpp`
- `runtime/ExecutableBase.h`
- `runtime/ExecutableBase.cpp`
- `runtime/ScriptExecutable.h`
- `runtime/ScriptExecutable.cpp`
- `runtime/FunctionExecutable.h`
- `runtime/FunctionExecutable.cpp`
- `runtime/ProgramExecutable.h`
- `runtime/EvalExecutable.h`
- `runtime/ModuleProgramExecutable.h`
- `runtime/NativeExecutable.h`
- `bytecode/UnlinkedFunctionExecutable.h`

### Responsibility

Function objects carry callable object identity and scope. Executables carry
source-derived and runtime-linked code state. JSC separates call and construct
specializations and can lazily link or tier code.

### Owns And Mutates

`JSFunction` owns scope plus executable or rare-data state. Rare data can hold
allocation profiles, lazy length/name data, bound-function structure, and
watchpoints.

`ExecutableBase` owns call and construct JIT code entrypoints separately.
`FunctionExecutable` owns unlinked executable state, call and construct
`CodeBlock`s, singleton inference, cached poly-proto structure, and template
objects. `ScriptExecutable` owns source metadata and parse flags.

### Hidden Invariants

- Function object state is separate from executable/code state.
- Call and construct are separate specializations.
- Executables are tightly coupled to parser products, `CodeBlock`, scope,
  structures, watchpoints, and JIT entrypoints.

### Rust Design Pressure

Rust should avoid making a function "just a closure." A function object needs a
separate executable/code identity and separate call/construct behavior.

Executable state should be designed before implementing function calls.

## 10. Exceptions And VM-Side Control State

Reviewed source includes:

- `runtime/Exception.h`
- `runtime/Exception.cpp`
- `runtime/ExceptionHelpers.h`
- `runtime/ThrowScope.h`
- `runtime/ExceptionScope.h`
- `runtime/TopExceptionScope.h`
- `runtime/VM.h`
- `interpreter/CallFrame.h`

### Responsibility

JSC represents thrown values through a VM-side pending exception channel.
`Exception` owns the thrown `JSValue` and captured stack data. `ThrowScope` and
`ExceptionScope` encode the discipline around throwing and checking exceptions.

### Owns And Mutates

The VM owns pending exception state and termination exception state. Throwing
code mutates this channel. Callers are expected to check it according to JSC's
scope macros and conventions.

### Hidden Invariants

- Throwing functions must declare/check exception scopes.
- Termination exceptions have special behavior and are not casually overwritten.
- Interpreter, debugger hooks, traps, and stack frames depend on the VM-side
  exception channel.

### Rust Design Pressure

A pure `Result<T, E>` model will not directly match JSC semantics. Rust can use
`Result` internally, but the engine design needs an explicit pending-exception
channel at VM boundaries.

Exception design should be done together with VM entry and call-frame design.

## 11. GC Heap, Roots, Write Barriers, Weak References, And Finalization

Reviewed source includes:

- `heap/Heap.h`
- `heap/Heap.cpp`
- `heap/MarkedSpace.h`
- `heap/MarkedBlock.h`
- `heap/Subspace.h`
- `heap/CompleteSubspace.h`
- `heap/IsoSubspace.h`
- `heap/PreciseSubspace.h`
- `heap/LocalAllocator.h`
- `heap/Allocator.h`
- `heap/SlotVisitor.h`
- `heap/AbstractSlotVisitor.h`
- `heap/MarkingConstraint*.h`
- `heap/Handle.h`
- `heap/Strong.h`
- `heap/Weak.h`
- `heap/WeakSet.h`
- `runtime/WriteBarrier.h`
- `runtime/WriteBarrierInlines.h`
- `runtime/WeakMapImpl.h`
- `runtime/JSFinalizationRegistry.h`
- `runtime/JSWeakObjectRef.h`
- `runtime/VM.h`

### Responsibility

The GC owns JavaScript-managed cells. The VM embeds the heap and stores many
barriered roots. The heap coordinates allocation, marking, sweeping, weak
cleanup, finalization, JIT/code liveness, and stop-the-world/access protocols.

`SlotVisitor` performs tracing. `WriteBarrier` records mutations from one
GC-managed owner to another. `Handle`, `Strong`, and protection APIs create
explicit roots. Weak sets, weak maps, weak refs, and finalization registries
implement weak semantics.

### Owns And Mutates

`VM` owns `Heap` by value and many `WriteBarrier<>` VM roots.

`Heap` owns `MarkedSpace`, visitors, mark stacks, marking constraints,
`HandleSet`, `CodeBlockSet`, `JITStubRoutineSet`, protected-value counts, weak
GC hash tables, and finalization queues.

`MarkedSpace` owns blocks, precise allocations, size classes, active weak sets,
and version counters.

`MarkedBlock` owns block metadata, mark bits, newly allocated bits, weak set
storage, and fixed-size cells.

`Subspace` variants own allocation policy and local allocators.

`WriteBarrier` stores a pointer or value and calls the heap barrier protocol
with the owner cell.

### Hidden Invariants

- Cells are arena-owned by the GC, not by normal C++ object ownership.
- Current JSC is strongly address-based and effectively non-moving, even though
  some handle comments mention moving support.
- `MarkedBlock::blockFor(ptr)` relies on power-of-two page-aligned blocks.
- Precise allocation detection uses pointer/tag bits.
- `JSCell::finishCreation` must run a mutator fence before the object escapes.
- GC must not scan a cell with null or invalid structure ID.
- `CellState` is a hint; correctness also depends on mark bits and fences.
- Write barriers are owner-based, not slot-based.
- Weak-map mutation has no-GC windows that optimized code models.
- Some weak-map bucket layouts are assumed by DFG/FTL.
- Finalization and code-block cleanup have ordering constraints.

### Rust Design Pressure

Rust must model GC ownership separately from Rust ownership. JavaScript objects
are not `Box<T>`, `Rc<T>`, or normal borrowed Rust values.

Core types should probably include:

- `Heap`
- `Gc<T>` or equivalent typed heap pointer
- `Root<T>` or `Handle<T>`
- `WriteBarrier<T>`
- `Trace` or equivalent visitor contract
- an allocation phase that separates uninitialized allocation from
  `finish_creation`

The barrier API should make raw mutation of GC object fields difficult. A normal
store into a GC-owned field should require owner-cell context.

## 12. Modules And Host Loading

Reviewed source includes:

- `runtime/JSModuleLoader.h`
- `runtime/JSModuleLoader.cpp`
- `runtime/AbstractModuleRecord.h`
- `runtime/CyclicModuleRecord.h`
- `runtime/JSModuleRecord.h`
- `runtime/ModuleRegistryEntry.h`
- `runtime/JSModuleEnvironment.h`
- `runtime/ModuleProgramExecutable.h`
- `parser/ModuleAnalyzer.h`
- `parser/NodesAnalyzeModule.cpp`

### Responsibility

The module system manages host-integrated module loading, resolving, fetching,
registry caching, linking, evaluation, dynamic import, import attributes, JSON
module handling, and Wasm module handoff.

### Owns And Mutates

`JSModuleLoader` owns module maps, loaded-module state, and cached resolution
failures.

`ModuleRegistryEntry` owns per-key status, module record, promises, and
fetch/instantiation/evaluation errors.

`AbstractModuleRecord` owns import/export entries, requested modules, loaded
modules, namespace objects, async/TLA state, module environment, and resolution
cache.

`JSModuleRecord` owns source, variable environments, feature flags, and lazily
created `ModuleProgramExecutable`.

### Hidden Invariants

- Module map keys include both identifier and source type.
- Fetch errors can be duplicated while still being cached.
- Host-load operations carry opaque payload state.
- Export resolution is deliberately iterative to avoid C++ recursion.
- `CyclicModuleRecord` status transitions mirror spec states and are part of
  correctness.

### Rust Design Pressure

The module loader should be a first-class typed state machine. Avoid recursive
graph algorithms. Keep registry entry status/errors separate from module record
contents.

Dynamic import and top-level await need an explicit relationship to the promise
and microtask model.

## 13. Builtins

Reviewed source includes:

- `builtins/BuiltinExecutables.h`
- `builtins/BuiltinExecutables.cpp`
- `builtins/BuiltinNames.h`
- `builtins/BuiltinNames.cpp`
- `builtins/BuiltinExecutableCreator.h`
- `builtins/BuiltinExecutableCreator.cpp`
- representative `builtins/*.js` files

### Responsibility

Builtins are JS-authored standard-library and internal algorithms compiled into
engine-internal executables. They are addressed through generated builtin indexes
and builtin names.

### Owns And Mutates

`BuiltinExecutables` owns a VM reference, combined source provider, and lazy
array of unlinked builtin executables.

`BuiltinNames` owns public identifiers, private symbols, and well-known symbol
maps.

### Hidden Invariants

- Builtin source has a constrained shape, usually a single anonymous function
  expression.
- Builtin executable creation hand-computes metadata to avoid parser recursion
  and validates against parser behavior in validation builds.
- `@name` syntax couples builtin JS to builtin names, bytecode intrinsics, and
  private-symbol lookup.

### Rust Design Pressure

Builtins should be treated as privileged engine-internal source modules or IR,
not ordinary user JavaScript. Their private-name and intrinsic namespace must be
stable and explicitly designed.

## 14. Public Embedding API

Reviewed source includes:

- `API/JSBase.h`
- `API/APICast.h`
- `API/JSContextRef.h`
- `API/JSContextRef.cpp`
- `API/JSObjectRef.h`
- `API/JSObjectRef.cpp`
- `API/JSValueRef.h`
- `API/JSValueRef.cpp`
- `API/JSClassRef.h`
- `API/JSClassRef.cpp`
- `API/JSCallbackObject.h`
- `API/JSCallbackObjectFunctions.h`
- `API/JSAPIGlobalObject.h`

### Responsibility

The public API exposes a stable opaque C ABI over VM/context groups, global
objects, values, callbacks, JS classes, scripts, protection, and exceptions.

### Owns And Mutates

Context creation creates or reuses VM context groups and global objects.
Retain/release interacts with VM lifetime. `JSValueProtect` and
`JSValueUnprotect` mutate GC root/protection state.

Callback object data owns host private data, class references, static functions,
static values, and private property maps.

### Hidden Invariants

- Opaque refs are pointer casts over internal structures.
- `JSContextGroupRef` maps to VM-like state.
- `JSContextRef` and `JSGlobalContextRef` map to global-object/context state.
- `JSObjectRef` is an object-shaped value reference.
- `JSValueRef` representation depends on value encoding and platform.
- Most entry points require JS locking discipline.
- Host callbacks can reenter the engine.

### Rust Design Pressure

The FFI boundary needs opaque handles, explicit rooting/protection, locking and
reentrancy rules, and a stable value representation story. Rust ownership cannot
leak directly through this API.

Whether the Rust rewrite preserves the existing C API or defines a new API with
a compatibility shim is an early architectural question.

## 15. JIT Tiers - Deferred

Reviewed source includes:

- `llint/*`
- `jit/JIT.h`
- `jit/JIT.cpp`
- `jit/JITCode.h`
- `jit/JITPlan.h`
- `jit/JITWorklist.h`
- `jit/BaselineJITPlan.h`
- `dfg/DFGPlan.h`
- `dfg/DFGGraph.h`
- `dfg/DFGByteCodeParser.h`
- `dfg/DFGOperations.h`
- `ftl/FTLState.h`
- `ftl/FTLCompile.h`
- `ftl/FTLLowerDFGToB3.h`
- `b3/*`

### Responsibility

JSC tiers execution through LLInt, Baseline JIT, DFG, and FTL/B3. These tiers
consume bytecode, profiling, inline caches, object shapes, watchpoints, GC
metadata, and call-frame layout.

### Owns And Mutates

`CodeBlock` owns linked bytecode state, metadata, profiles, inline caches,
exception handlers, counters, JIT code, and alternatives or replacements.

JIT plans and worklists own concurrent compilation jobs, weak references,
watchpoints, inline call frame metadata, optimized code, and finalization.

### Hidden Invariants

- LLInt is represented as an interpreter thunk tier.
- Baseline is the bottom compiled tier.
- DFG and FTL are optimized replacement tiers.
- Concurrent compilers can write into code-related state outside ordinary
  barrier patterns and therefore participate explicitly in GC liveness.
- Tier-up counters and OSR state are sensitive to bytecode cost and loop shape.
- JIT-visible offsets make runtime layout semantic.

### Rust Design Pressure

JIT implementation can be deferred, but the Rust architecture must still
preserve an execution abstraction equivalent to:

```text
Executable -> CodeBlock -> entrypoint/profile/metadata
```

Profiling and inline-cache storage should be optional side data, not hardwired
into interpreter semantics.

## 16. Wasm - Deferred

Reviewed source includes:

- `wasm/WasmModule.h`
- `wasm/WasmModule.cpp`
- `wasm/WasmPlan.h`
- `wasm/WasmIPIntPlan.h`
- `wasm/WasmModuleInformation.h`
- `wasm/WasmCalleeGroup.h`
- `wasm/WasmMemory.h`
- `wasm/WasmTable.h`
- `wasm/js/JSWebAssemblyModule.h`
- `wasm/js/JSWebAssemblyInstance.h`
- `wasm/js/WebAssemblyModuleRecord.h`
- `wasm/js/JSToWasm.h`
- `wasm/js/WasmToJS.h`

### Responsibility

Wasm handles JS `WebAssembly` objects, validation, compilation, module records,
instances, imports/exports, memories, tables, globals, JS/Wasm call bridges, and
Wasm tiering.

### Owns And Mutates

`Wasm::Module` owns module information, IPInt callees, callee groups per memory
mode, Wasm-to-JS stubs, and anchors.

`JSWebAssemblyInstance` owns memories, tables, globals, import function info,
cached memory/table pointers, wrappers, module record, and callee group.

`WebAssemblyModuleRecord` owns ESM integration state for Wasm modules.

### Hidden Invariants

- Wasm can enter JS module loading through source type handling.
- Instance layout is custom and used by generated code.
- Imported JS functions and Wasm-to-Wasm calls use specialized call-link and
  entrypoint machinery.
- Module anchors keep instances visible to concurrent compilation/profiling.

### Rust Design Pressure

Wasm should be deferred. The initial design should still reserve module-loader
and host-object extension points so Wasm can be added later without changing the
module and runtime object contracts.

If omitted initially, Wasm construction and WebAssembly source types should fail
cleanly at the host boundary.

## Cross-Cutting Constraints

These constraints cut across subsystem boundaries and must be captured before
implementation tasks begin:

- GC ownership is separate from Rust ownership.
- JS-managed objects need stable heap identity.
- Runtime layout is semantic because LLInt/JIT/API code reads offsets directly.
- Write barriers are owner-based and must be enforced at mutation points.
- Watchpoints are correctness mechanisms.
- Function object state is separate from executable/code state.
- Unlinked code and linked code should remain separate.
- Call frame layout, VM entry state, exception state, and GC stack scanning form
  one execution contract.
- Parser and bytecode generation currently share semantic state through AST
  nodes; a Rust rewrite must either preserve that or introduce a deliberate IR.
- Builtins are privileged engine code, not ordinary user scripts.
- Modules are a host-integrated state machine, not just parsed source files.
- JIT and Wasm can be deferred, but their coupling points must be reserved.

## First Follow-Up Design Questions

Before writing implementation code, answer these in separate design documents:

1. Will the Rust engine preserve JSC binary layout and C++/JIT ABI compatibility,
   or will it define a clean Rust-internal boundary?
2. Is the GC non-moving? If so, which types encode stable heap identity and
   rooting?
3. What is the exact `JSValue` representation for the first Rust target?
4. What is the safe API for storing into GC-managed object fields?
5. What is the Rust equivalent of `JSCell`, `Structure`, `JSObject`, and
   `Butterfly`?
6. Does bytecode generation target a typed Rust IR first or packed bytecode
   directly?
7. What `Executable` / `CodeBlock` abstraction preserves execution, profiling,
   metadata, and later JIT tiering without forcing an early executable path?
8. How are VM-side exceptions represented, and where is `Result` allowed?
9. How are global objects, realms, and intrinsic structures initialized?
10. What subset of modules, builtins, API, JIT, and Wasm is explicitly deferred?

## Guidance For Agent Work

Agents should use this map to choose bounded subsystem tasks. A valid next agent
task is to expand one section into a focused design note with source references,
ownership contracts, proposed Rust types, unsafe boundaries, and tests.

Agents should not start by making a JavaScript program execute end-to-end. That
would force unresolved decisions in nearly every section above.
