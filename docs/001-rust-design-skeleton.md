# Rust Design Skeleton

This document is the breadth-first Rust design skeleton for the JavaScriptCore
rewrite. It turns the responsibility map into Rust-side contracts: proposed
types, ownership relationships, mutation rules, unsafe boundaries, dependencies,
and unresolved questions.

This is not an implementation plan or a minimum executable path. The type names
below are design names. They may later map to modules, files, traits, or
generated code inside the single Rust crate, but they already define the shape
future implementation work must respect.

This document now coexists with executable Rust code. When this design skeleton
and the code diverge, the main agent should inspect the implementation, decide
whether the implementation is architecturally correct, and then update the
design or the code. Do not treat a passing test as proof that the design
boundary is settled.

Progress is tracked sparsely in `progress.md`. Work scheduling lives in
`002-bfs-rewrite-plan.md`.

## How To Use This Skeleton

Each subsystem section defines:

- the JSC responsibility it models
- proposed Rust concepts and type names
- ownership relationships
- mutation rules
- unsafe boundary
- dependencies on other components
- unresolved questions

Future agents may implement behind these contracts. They must not redesign
neighboring ownership, mutation, unsafe, or compatibility boundaries while
working on one component.

The main agent should delegate large component work and act as reviewer. Direct
main-agent implementation should be reserved for small glue, corrections, or
well-bounded fixes that do not alter subsystem priority.

## Cross-Cutting Design Rules

This skeleton is breadth-first. It is complete only when all major engine
responsibilities have named Rust concepts, ownership contracts, mutation rules,
and deferred extension points.

Agents must not use a small executable JavaScript path as a forcing function. A
path that evaluates even trivial JavaScript would prematurely choose value
representation, heap identity, rooting, strings, global object initialization,
bytecode shape, frames, exceptions, object storage, and builtin behavior. Those
choices belong in explicit contracts.

Now that an executable interpreter path exists, the same rule applies in a
different form: do not let local failing VM tests choose missing architecture.
If a feature needs handles, GC, module jobs, host hooks, or tiering contracts,
pause the local fix and schedule the shared dependency first.

Deferred does not mean absent. Wasm, public API layout, optimized code
metadata, and host integration may remain unimplemented, but their architectural
attachment points must be named and preserved. Baseline JIT is no longer only a
deferred hook: the rewrite needs one honest baseline tier, even if it supports
only a narrow opcode slice, so execution, frame, root, fallback, exception, and
GC contracts are tested against more than the interpreter.

## Execution Architecture Policy

The rewrite needs one reference interpreter and one baseline JIT tier before the
execution architecture can be considered proven. It does not need all
interpreter features or all JIT tiers first.

The interpreter is the semantic oracle. It owns the canonical bytecode
semantics, observable exception behavior, runtime-call behavior, root snapshots,
and fallback state. Baseline-enabled execution must remain comparable against
interpreter-only execution.

The first baseline JIT is an integration proof, not an optimization project. It
may support only constants, moves, returns, and a small arithmetic/control-flow
slice, but it must enter through VM-owned code-block/tiering state, expose the
same frame/root/exception/runtime-call/GC contracts as the interpreter, and fall
back through the real interpreter for unsupported bytecode.

The current serial dependency is the baseline execution contract. Memory
ownership integration, the first baseline frame/root ABI, VM-owned baseline
installation, CodeBlock-derived bytecode eligibility, the non-native typed
baseline generated-code artifact contract, and interpreter frame/register helper
boundary are already shaped. The VM now has a first generated-entry proof for a
narrow typed baseline body covering constants, moves, return, and int32
arithmetic with interpreter fallback for overflow/non-fast cases. It now also
covers selected no-call/no-heap int32 bitwise fast paths: bit-not, bitwise
or/xor/and, signed and unsigned shifts, int32 relational comparisons,
unconditional jumps, nullish conditional jumps, and primitive-local
`JumpIfFalse` truthiness. Unsigned shift now writes either Int32 or Double
primitive results without treating the widening as overflow, and primitive
`LogicalNot`, `StrictEqual`, and `StrictNotEqual` now use generated boolean
helpers while cell/string/symbol or otherwise runtime-dependent cases fall back
to the interpreter. Primitive number generation now covers `LoadDouble`,
numeric-only `DivNumber`/`ModNumber`, primitive-only `ToNumber`, effect-neutral
`Void`, primitive-coercing `NegateNumber`/`BitNotInt32`, pure-number
Int32/Double `AddInt32`/`SubInt32`/`MulInt32`, numeric relational comparisons,
and binary bitwise `ToInt32`; cell/unknown numeric coercion falls back with
typed diagnostics, and `PowNumber` remains outside the generated subset.
VM-boundary differential coverage and CodeBlock snapshot fingerprints reject
stale same-owner bytecode before generated execution. Typed fallback diagnostics
now travel through executor results into VM-owned fallback records, and
generated fallback throws synchronize exception roots after no-GC exit. The
current P6 opcode slice carries a generated-effect contract proving no heap
allocation, no runtime/JS call, and no heap/root mutation, with an explicit
register barrier-handoff caveat from the shared register write helper. The
generated runtime/heap boundary now includes planner-side classification for
no-JS-call heap/runtime helpers, executor helper-handoff metadata, VM-side
heap/root/no-GC boundary evidence, and an explicit VM-owned helper execution
path proven with planner-approved destination-only allocation handoffs. The VM
suspends the active no-GC region, dispatches exactly one interpreter instruction
through the runtime boundary, resumes no-GC, and rejects stale helper plans
before helper execution. Helper plans are now artifact-owned metadata,
validated against the artifact bytecode snapshot, rejected if malformed,
recorded through VM-owned generated install, and consumed by the normal tiering
entry path for an installed `NewObject`, `NewArray`, or `TypeOf` helper
artifact. Planner/install now derives operand-role helper metadata from
registered CodeBlock root maps and rejects explicit helper plans that do not
match the derived CodeBlock/root-map proof. `TypeOf` remains a runtime-helper
handoff, not direct generated execution. Before adding strings, objects,
properties, calls, BigInt, or `PowNumber` broadly to generated code, each
opcode's allocation, barrier, root, and exception contract must be proven
through that derived helper path. A JIT slice that bypasses
code-block ownership, cell identity, root visibility, barriers, exceptions,
fallback, or runtime calls is not acceptable, even if it makes a small
generated-code test pass.

## Compatibility Boundary Policy

The Rust design models JSC concepts first, not exact C++ file layout or field
layout. Concepts such as `Vm`, `Heap`, `JsValue`, `JsCell`, `Structure`,
`JsObject`, `Executable`, `CodeBlock`, `CallFrame`, and module records are
semantic contracts.

Exact C++ layout, offset stability, pointer tagging, LLInt/JIT ABI, and public
C API opaque-handle compatibility are separate compatibility modes. A subsystem
may depend on exact layout only if the relevant design section marks that
dependency explicitly.

No implementation may copy C++ layout only because nearby C++ code exposes
offsets. Layout compatibility must be justified by a named boundary: generated
code, mixed C++/Rust operation, public API, debugger/profiler tooling, JIT, or
Wasm entry.

## Unsafe Rust Policy

Unsafe Rust is permitted only at named engine boundaries:

- tagged value representation
- GC allocation and cell initialization
- raw object storage layout
- barrier slots
- FFI handles and callbacks
- generated-code entrypoints
- stack and frame layout
- atomics and fences
- future JIT/Wasm ABI bridges

Unsafe is not a local escape hatch for unresolved ownership. Any unsafe function
or unsafe block must document:

- the representation, aliasing, or lifetime invariant it relies on
- who owns the referenced memory
- which roots or handles keep GC things alive
- whether a write barrier is required
- whether the object may already have escaped
- whether generated code, C API code, or concurrent compiler code can observe
  the layout

Normal runtime code should call safe APIs that enforce rooting, finish-creation,
structure transitions, and write barriers.

## Cross-Component Ownership

Rust ownership and JavaScript ownership are separate. GC-managed cells are owned
by `Heap`, not by `Box`, `Rc`, or ordinary Rust borrowing. Stack and native
references to cells must use handles, roots, or explicitly scoped borrowed
access.

The VM owns the heap and VM-wide roots. The heap owns allocated cells. Cells own
their internal barriered references and storage. Runtime objects own identity
and storage, while structures own shape/prototype/class metadata. Executables
own source-derived code state; linked code blocks own runtime-linked metadata,
constants, profiles, caches, and deferred tiering slots.

Cross-component references must be directional and typed. Backedges, weak
references, alternative code, replacement code, and caches must state whether
they are strong, weak, barriered, rooted, or externally traced.

## Cross-Component Mutation And Barriers

All writes from a GC-owned object to a GC thing must go through an owner-aware
barrier API. The API must require the owning cell or equivalent owner context.
Slot-only mutation is not the default model.

Mutation APIs must distinguish:

- initialization before escape
- normal post-escape mutation
- structure or shape transition
- property and indexed storage mutation
- variable or scope storage mutation
- weak/map/finalization mutation
- concurrent compiler or generated-code mutation

Direct field writes to GC references, structure IDs, object storage pointers, or
cached runtime objects are forbidden outside unsafe layout capsules.
`set_without_barrier`-style operations are initialization-only or explicitly
compiler/JIT-only and must carry proof comments.

## VM

### Proposed Responsibility

`Vm` owns one engine instance's runtime state. It coordinates heap access, root
and handle registration, exception and termination state, VM entry bookkeeping,
canonical runtime structures, string/symbol registries, runtime caches, and
host/client hooks.

The VM is the coordinator for execution and GC. It is not a convenience global.

### Proposed Types

- `Vm`: engine instance and owner of heap-wide runtime state.
- `VmConfig`: configuration selected before VM creation.
- `VmEntryState`: active entry-frame and top-frame bookkeeping.
- `ExceptionState`: pending exception and termination exception state.
- `RuntimeStructures`: canonical structures shared by the VM.
- `RuntimeCaches`: VM-wide caches for structures, executable data, strings, and
  service state.
- `VmServices`: host services, watchdogs, timers, microtask hooks, and
  integration callbacks.
- `HeapAccessToken`: explicit proof that the mutator may allocate or inspect GC
  cells.

### Ownership

`Vm` owns `Heap`, root sets, handle sets, registries, common structures, runtime
caches, and host hooks. GC cells are heap-owned and reached through handles,
roots, or barriers.

Call frames are not owned by `Vm`, but `VmEntryState` records the active top
frame and entry frame.

### Mutation

VM entry and exit mutate top-frame state through scoped guards. Exception state
mutates only through exception APIs. Heap access must be acquired before
allocation or cell inspection. Runtime singletons and cached structures are
stored through barriers.

### Unsafe Boundary

Unsafe is limited to ABI-visible VM field offsets, top-frame and entry-frame
layout, raw pointers used by interpreter/JIT entry, host FFI hooks, and direct
access to GC-owned cells.

### Dependencies

`Vm` depends on `Heap`, `JsValue`, `JsCell`, global objects, code blocks,
executables, interpreter frames, strings, symbols, host APIs, and service hooks.

### Unresolved Questions

- Is the VM single-thread-affine, shareable with locking, or represented as a
  context group plus per-thread entry state?
- Are handles raw slots, generational indices, or another stable reference form?
- Which VM fields must remain layout-compatible with existing LLInt/JIT/API
  code?
- How are host API context groups represented?

## Heap / GC / Roots / Barriers

### Proposed Responsibility

`Heap` owns all GC-managed cell allocation, marking, sweeping, finalization,
auxiliary storage liveness, weak references, roots, handles, conservative roots,
and write/read barrier policy.

### Proposed Types

- `Heap`: owner of JavaScript-managed memory.
- `Subspace`: typed allocation domain.
- `CellArena`: allocation area for cells and cell-sized blocks.
- `GcRef<T>`: typed reference to a heap-owned cell.
- `Handle<T>`: scoped rooted reference usable from Rust code.
- `Root<T>`: long-lived explicit root.
- `Weak<T>`: weak reference cleared by GC.
- `WriteBarrier<T>`: owner-aware barriered field.
- `ValueBarrier`: barriered `JsValue` field.
- `Trace`: trait or method-table contract for visiting children.
- `Tracer`: marking visitor interface.
- `CollectionScope`: active collection state.
- `GcPhase`: allocation, marking, sweeping, finalization, and idle phases.
- `NoGcScope`: proof that an operation cannot trigger collection.
- `CellInit<T>`: unpublished allocation state before finish-creation.

### Ownership

`Heap` owns cells and auxiliary allocations. Rust code may hold rooted handles or
short-lived borrowed cell references tied to a heap access scope. Heap fields
inside cells use `WriteBarrier<T>` or `ValueBarrier`; unbarriered storage is
limited to initialization and verified root paths.

Weak references are owned by weak tables or weak sets and are cleared by the GC,
not ordinary Rust ownership.

### Mutation

Allocation and cell access require mutator heap access. Any reference store from
a GC-owned object to another GC thing must go through a barrier. Initializing an
unpublished cell may use barrier-free writes through `CellInit<T>`. Weak refs
and finalization queues mutate only during defined GC phases or no-GC mutation
windows.

### Unsafe Boundary

Unsafe is limited to allocator layout, moving/non-moving assumptions, raw cell
pointer decoding, conservative stack scanning, auxiliary storage marking,
finalizer callbacks, concurrent marking races, and barrier fast paths.

### Dependencies

`Heap` depends on `Vm`, `JsCell` headers, `JsValue` encoding, object storage,
structures, code blocks, call frames, host roots, and future JIT metadata.

### Unresolved Questions

- Is the collector non-moving by contract?
- Are conservative roots retained?
- What generational or incremental barrier algorithm is used?
- How do Rust lifetimes encode no-GC regions?
- What is the finalization and destructor ordering model?

## JSValue

### Proposed Responsibility

`JsValue` represents JavaScript values as the tagged transport type used by the
runtime, bytecode constants, object storage, frames, public API, and future
JIT-visible APIs.

### Proposed Types

- `JsValue`: `repr(transparent)` encoded value word.
- `EncodedJsValue`: raw ABI/storage representation.
- `ValueKind`: decoded classification for safe queries.
- `NumberValue`: number-specific view.
- `CellValue`: cell-containing view.
- Constructors such as `undefined`, `null`, `from_i32`, `from_double`, and
  `from_cell`.

### Ownership

`JsValue` owns only bits. If it contains a cell pointer, the heap owns the cell
and rooting/barriers determine liveness.

### Mutation

Values are copyable bit payloads. Storing a cell-containing value into
GC-managed memory requires `ValueBarrier`. Decoding to a cell reference requires
heap access or a rooted/validated handle path.

### Unsafe Boundary

Unsafe is limited to bit encoding/decoding, NaN-boxing or split 32/64-bit
representation, cell pointer extraction, concurrent loads from value storage,
and JIT/LLInt tag constants.

### Dependencies

`JsValue` depends on `JsCell`, heap barriers, bytecode constants, call frames,
registers, property storage, and API value conversion.

### Unresolved Questions

- Is the first Rust target `JSVALUE64` only?
- How much C++/JIT binary compatibility is required?
- How are BigInt32, unboxed Wasm values, empty/deleted sentinels, and concurrent
  value loads represented?

## JSCell

### Proposed Responsibility

`JsCell` defines the common GC header and dynamic dispatch/type identity for
every heap-managed JavaScript value.

### Proposed Types

- `JsCellHeader`: `repr(C)` common cell header.
- `JsCell`: erased cell identity.
- `CellType`: runtime type tag.
- `TypeInfo`: object/callability/indexing metadata.
- `CellState`: GC cell-state hint.
- `StructureId`: encoded structure reference.
- `CellLock`: per-cell lock, if retained.
- `TraceCell`: tracing contract for cell payloads.
- `CellVTable` or trait-backed method table: dynamic behavior and tracing.

### Ownership

Every GC cell begins with `JsCellHeader`. A cell references its `Structure`
through `StructureId`; subtype payload follows the header and is heap-owned.

### Mutation

Structure changes go through `set_structure` and barrier/nuking rules. Cell
state changes use atomic or compare-and-swap operations. Indexing-type and
miscellaneous flags cannot be freely assigned after a cell escapes. Cell lock
ordering remains cell lock before structure lock if both locks exist.

### Unsafe Boundary

Unsafe is limited to `repr(C)` header offsets, subtype casts, method-table
dispatch, `StructureId` encode/decode, atomic state transitions, and lock layout
assumptions.

### Dependencies

`JsCell` depends on heap allocation and marking, `Structure`, `JsValue`, type
info, object model, method tables, and interpreter/JIT offset extraction.

### Unresolved Questions

- Should dynamic behavior use Rust traits, explicit method tables, or generated
  tables?
- What is the `StructureId` compression strategy?
- What pinning guarantees are provided?
- Are cell locks embedded in all cells?

## JSObject / Structure / Butterfly

### Proposed Responsibility

This subsystem models object identity, shape, property metadata,
prototype/realm linkage, inline and out-of-line storage, indexed storage,
transitions, and watchpoint invalidation.

### Proposed Types

- `JsObject`: object cell.
- `ObjectHeader`: object-specific header after `JsCellHeader`.
- `Structure`: shape, prototype, class info, transition, and watchpoint cell.
- `StructureId`: encoded reference to a structure.
- `StructureTransition`: add/delete/attribute/prototype transition descriptor.
- `PropertyTable`: property metadata table.
- `PropertyOffset`: inline/out-of-line slot offset.
- `Butterfly`: unsafe out-of-line storage capsule.
- `IndexingHeader`: indexed storage metadata.
- `InlineStorage`: typed inline slot view.
- `OutOfLineStorage`: typed butterfly property view.
- `IndexingMode`: indexed storage representation.
- `WatchpointSet`: structure/prototype/cache invalidation state.

### Ownership

`JsObject` is a `JsCell` with inline storage and optional butterfly storage.
`Structure` is a GC cell shared by many objects. `Butterfly` is auxiliary heap
storage logically owned by one object unless a copy-on-write representation is
explicitly in use. Property tables and rare data are GC-managed or heap-owned
through barriers.

### Mutation

Adding, removing, or changing properties transitions or mutates dictionary
structures, then writes the slot with barriers. Structure and butterfly changes
are coupled. Indexed writes may change indexing mode before writing. Prototype
or realm changes invalidate watchpoints. Concurrent property reads use the
cell/structure locking protocol.

### Unsafe Boundary

Unsafe is limited to object inline-storage offsets, butterfly pointer
arithmetic, negative property-storage indexing, indexing-header placement,
JIT-visible `Structure` fields, property offset math, copy-on-write storage, and
concurrent structure-table materialization.

### Dependencies

This subsystem depends on `JsCell`, `JsValue`, heap barriers, property keys,
global object/realm state, watchpoints, array storage, bytecode inline caches,
and future JIT layout constants.

### Unresolved Questions

- What is the Rust allocation API for `Butterfly`?
- Are structure transitions immutable nodes plus dictionary side state?
- How are watchpoints represented before JIT exists?
- What compatibility is required for current JIT-visible object layout?

## Identifier / String / Symbol / PropertyKey

### Proposed Responsibility

This subsystem owns string interning, runtime string cells, symbol/private-name
identity, and property-key typing. It preserves the distinction between string
identifiers, symbols, private names, and numeric/index property keys.

### Proposed Types

- `AtomTable`: intern table owned by the VM.
- `AtomId`: stable interned string identity.
- `Identifier`: identifier handle for parser/runtime names.
- `JsString`: GC-managed runtime string.
- `RopeString`: deferred concatenation string state.
- `SymbolUid`: stable symbol identity.
- `SymbolCell`: GC-managed symbol object.
- `PrivateName`: private field/name identity.
- `PropertyKey`: enum for string, symbol, private name, and index keys.
- `CacheableIdentifier`: key form usable by caches and inline caches.

### Ownership

`Vm` owns the atom table and common identifiers. `Heap` owns `JsString` and
`SymbolCell`. `Identifier` and `PropertyKey` are small handles to interned
identity, not owners of string storage.

### Mutation

Interning mutates only through the VM/string-table API. Interned names and
symbol identities are immutable after creation. Rope resolution may cache a
flattened string, but that cache must be explicit and GC-aware.

### Unsafe Boundary

Unsafe is limited to pointer-sized key representation if retained for
compatibility, rope layout and concurrent string access, and FFI conversion to
or from `JSStringRef`.

### Dependencies

This subsystem depends on `Vm`, `Heap`, parser, object property storage, module
maps, builtin names, and public API string/value conversion.

### Unresolved Questions

- Does Rust preserve JSC's pointer-sized `PropertyName` ABI?
- Can `Identifier` carry symbol identity, or must symbol-aware paths use
  `PropertyKey`?
- Where does canonical numeric-index parsing live?

## GlobalObject / Realm / Intrinsics

### Proposed Responsibility

The global object subsystem models the realm root: global object identity,
global lexical environment, intrinsic constructors and prototypes, structure
caches, watchpoints, module loader, microtask hooks, and host method table.

### Proposed Types

- `Realm`: logical realm state.
- `GlobalObject`: GC-managed global object and realm root.
- `GlobalThis`: explicit global `this` binding.
- `Intrinsics`: constructor/prototype/function table.
- `IntrinsicSlot<T>`: barriered slot for an intrinsic.
- `LazyIntrinsic<T>`: lazily initialized intrinsic.
- `RealmStructures`: canonical structures for a realm.
- `RealmWatchpoints`: global/prototype/intrinsic watchpoints.
- `HostRealmHooks`: host callbacks for module loading, promises, and API
  integration.

### Ownership

`Vm` owns or indexes realms. `Heap` owns `GlobalObject`, prototypes,
constructors, structures, and lexical environments. `GlobalObject` holds
barriered references to realm-owned intrinsics and host hooks.

### Mutation

Realm initialization is staged and single-owner until publication. Lazy
intrinsics initialize through checked realm APIs. Mutating primordial objects
invalidates relevant watchpoints and structure caches.

### Unsafe Boundary

Unsafe is limited to layout offsets needed by interpreter/JIT/API,
initialization of cyclic intrinsic graphs, and FFI host hook calls that can
reenter the engine.

### Dependencies

This subsystem depends on the object model, structures, GC barriers, scopes,
builtins, modules, promises/microtasks, public API contexts, and exception
state.

### Unresolved Questions

- Is `Realm` a distinct Rust type or a role of `GlobalObject`?
- Which intrinsics are eager versus lazy?
- How are host hooks represented without exposing Rust lifetimes across
  reentrant calls?

## Scope / Environment / SymbolTable

### Proposed Responsibility

This subsystem represents scope chains, lexical/module/global environments,
binding metadata, TDZ state, stable binding storage, and lookup/update rules.

### Proposed Types

- `Scope`: GC-managed scope cell.
- `ScopeKind`: global, lexical, function, module, eval, with, catch, and other
  scope forms.
- `ScopeChain`: parent-linked scope chain view.
- `Environment`: binding storage abstraction.
- `LexicalEnvironment`: lexical binding object.
- `SegmentedEnvironment`: address-stable segmented binding storage.
- `GlobalLexicalEnvironment`: global lexical binding object.
- `ModuleEnvironment`: module binding storage.
- `SymbolTable`: binding metadata table.
- `SymbolTableEntry`: compact or expanded binding metadata.
- `BindingSlot`: barriered storage for binding values.
- `ScopeOffset`: encoded scope slot offset.
- `VarOffset`: global/direct-access variable location.

### Ownership

Scope and environment objects are GC cells. A scope points to its parent with a
write barrier. Environments own binding storage. `SymbolTable` owns binding
metadata and may be shared or cloned by environments.

### Mutation

Binding creation mutates `SymbolTable` before or during environment allocation.
Value stores go through barriered `BindingSlot` APIs. Const/read-only and TDZ
checks live at the environment access boundary. Watchpoint touch/invalidate
behavior is part of binding mutation.

### Unsafe Boundary

Unsafe is limited to address-stable binding storage for direct access, inline
variable storage after cell headers, JIT-visible offsets, and concurrent symbol
table iteration/locking.

### Dependencies

This subsystem depends on identifiers/property keys, GC/write barriers,
bytecode/codegen binding offsets, global object, module records, debugger
metadata, and profiling metadata.

### Unresolved Questions

- Do all environments use segmented storage, or only global/direct-access cases?
- How much watchpoint machinery exists before JIT implementation?
- Are symbol-table entries compact encoded state or explicit Rust enums?

## Parser / Lexer / Source

### Proposed Responsibility

This subsystem owns stable source identity and source slices, tokenizes Latin-1
or UTF-16 input, preserves source positions and directives, and drives a parser
generic over tree construction versus syntax checking. This is a frontend
contract, not an execution sequence.

### Proposed Types

- `SourceProvider`: immutable source text plus origin, URL, taint, and cache
  hooks.
- `SourceCode`: ranged view into a `SourceProvider` with first-line and
  start-column metadata.
- `SourceSpan`, `SourcePosition`, `LineColumn`: source-location currency used
  beyond parsing.
- `SourceEncoding`: Latin-1 or UTF-16 specialization boundary.
- `Lexer<'src, 'arena, E>`: cursor, tokenization state, directive buffers, and
  error buffers.
- `Token`, `TokenKind`, `TokenData`: token type, span, literal, and identifier
  payload.
- `LexerSnapshot`: parser save/restore state for ambiguous grammar.
- `Parser<'src, 'arena, B: TreeBuilder>`: recursive-descent parser over a lexer.
- `TreeBuilder`: shared interface for AST building and syntax checking.
- `ParseMode`, `ScriptMode`, `BuiltinMode`: source grammar configuration.
- `ParserError`: structured syntax and early-error result.

### Ownership

`SourceProvider` owns source storage. `SourceCode` holds a shared provider
handle and range. `Lexer` borrows `SourceCode` and `IdentifierArena`. `Parser`
owns lexer state, parser scopes, labels, declarations, and builder state. Tokens
must not outlive the source and parser arena.

### Mutation

Source text is immutable after construction. Source URL directives and cache
state are controlled side data. Lexer mutation is limited to cursor, line,
token, and temporary buffers. Parser mutation is limited to current token,
savepoints, scope stack, labels, declarations, and feature flags.

### Unsafe Boundary

Optimized source cursor walking and encoding-specialized scanning may use unsafe
internally, but public token/source APIs remain lifetime-checked. Cached
bytecode and source-provider FFI must not expose borrowed raw source pointers.

### Dependencies

This subsystem depends on identifiers/strings, parser arena, parser modes,
source-origin/taint model, syntax-error construction, module metadata, and code
cache hooks.

### Unresolved Questions

- Are source providers GC cells or ref-counted engine objects?
- How does cached bytecode pin source identity?
- How is function-body reparsing represented?
- Where do generated Unicode tables live?
- How much VM/global identifier state may the parser access?

## AST / Parser Arena / Semantic Metadata

### Proposed Responsibility

This subsystem represents parsed syntax and early semantic decisions with arena
lifetime semantics, while preserving the split between AST construction and
AST-less syntax checking.

### Proposed Types

- `ParserArena`: bump arena for parse products and parser-local identifiers.
- `AstRef<T>` or typed arena indices: non-owning handles into `ParserArena`.
- `IdentifierArena`: parser-local identifier cache backed by engine strings.
- `AstRoot`: root handle tying AST lifetime to arena lifetime.
- `Expr`, `Stmt`, `ScopeNode`, `FunctionMetadata`: syntax and function metadata.
- `VariableEnvironment`: var/let/const/import/private-name declarations.
- `EarlySemanticInfo`: strict mode, captures, features, and constant counts.
- `ModuleAnalysis`: import/export/module-record metadata.
- `AstBuilder`: `TreeBuilder` implementation that allocates nodes.
- `SyntaxBuilder`: `TreeBuilder` implementation that returns compact syntax
  facts.

### Ownership

`ParserArena` owns all AST nodes and parser-local identifiers. AST handles
borrow or index into the arena. Function metadata references source spans,
parameter metadata, declaration environments, and body roots. Module analysis is
an owned result derived from the AST.

### Mutation

AST nodes and semantic metadata are mutable only during parse/build. After
parse, AST products are frozen for analysis/code generation. Later passes attach
derived results separately unless a field is explicitly marked as a post-parse
annotation.

### Unsafe Boundary

Arena allocation may use unsafe internally if using typed bump allocation, but
exposed handles must prevent use after arena drop. Raw node downcasts should not
be an API contract; prefer enums or typed handles.

### Dependencies

This subsystem depends on parser/lexer/source spans, identifiers/strings,
variable environments, module analysis, bytecode generation or lowered bytecode
IR, and error reporting.

### Unresolved Questions

- Preserve AST-node-driven codegen or lower to a separate IR?
- Should AST representation use enums, trait objects, or generated node types?
- What is the destructor policy for arena contents?
- How are capture analysis and sloppy hoisting encoded without hidden parser
  side effects?

## Bytecode Schema

### Proposed Responsibility

The bytecode schema is the single source of truth for opcodes, operands, widths,
metadata, checkpoints, and verifier rules used by generation, linking,
interpretation, profiling, and future JIT tiers.

### Proposed Types

- `Opcode`: generated opcode enum preserving schema order.
- `OperandKind`, `OperandSpec`: generated operand layout descriptors.
- `VirtualRegister`: local/argument/header/constant register encoding.
- `BytecodeIndex`, `CallSiteIndex`: typed byte offsets and call-site
  identifiers.
- `TypedInstruction`: safe generated instruction view.
- `InstructionBuilder`: mutable emission and label-patching interface.
- `PackedInstructionStream`: frozen byte representation.
- `MetadataTable` and `UnlinkedMetadataTable`: per-opcode metadata storage.
- `CheckpointSpec`: generated checkpoint layout for exception/OSR-visible
  points.
- `BytecodeVerifier`: validates widths, targets, operands, and metadata
  alignment.

### Ownership

The bytecode generator owns `InstructionBuilder` until finalization.
`UnlinkedCodeBlock` owns frozen instructions, unlinked metadata, constants, jump
tables, and handler tables. `CodeBlock` owns linked metadata and runtime-facing
profiles/caches.

### Mutation

Labels, jumps, temporary register allocation, and metadata mutate before
finalization. Frozen unlinked bytecode is immutable. Runtime feedback, inline
caches, profiling, and patchable call data belong to linked `CodeBlock`
metadata, not the unlinked schema.

### Unsafe Boundary

Unsafe is limited to packed instruction decoding, metadata alignment, raw byte
offsets, and JIT/LLInt ABI layout. Prefer a generated typed Rust instruction
layer and confine packed-layout operations to one module.

### Dependencies

This subsystem depends on virtual registers, `JsValue`, GC/write barriers for
constants and metadata, unlinked/linked code blocks, interpreter dispatch,
exception handlers, and call/link metadata.

### Unresolved Questions

- Typed IR before packing, or direct packed emission?
- What is the cached-bytecode format?
- What endian and alignment guarantees are required?
- How does checkpoint metadata map to Rust frames?
- How much future JIT layout compatibility is required immediately?

## UnlinkedCodeBlock / CodeBlock / Executable

### Proposed Responsibility

This subsystem preserves JSC's separation between source-derived reusable code,
runtime-linked code, and executable objects that own specialization,
installation, replacement, and entrypoint state.

### Proposed Types

- `UnlinkedCodeBlock`: frozen bytecode plus source-derived metadata.
- `UnlinkedFunctionCodeBlock`, `UnlinkedProgramCodeBlock`,
  `UnlinkedEvalCodeBlock`, `UnlinkedModuleCodeBlock`: code-kind
  specializations.
- `UnlinkedFunctionExecutable`: function source metadata and lazily generated
  unlinked call/construct code.
- `ExecutableBase`: common executable cell with entrypoint slots.
- `ScriptExecutable`: source-backed executable with parse/codegen feature
  metadata.
- `FunctionExecutable`: linked function executable with call/construct
  `CodeBlock`s.
- `CodeBlock`: runtime-linked bytecode, constants, scopes, metadata, handlers,
  profiles, and caches.
- `LinkContext`: global object, scope, specialization, and VM state needed to
  link.
- `CodeCache`: source/unlinked-code reuse boundary.
- `DeferredTieringSlots`: reserved JIT/tiering attachment points.

### Ownership

Unlinked code owns frozen frontend products. Executables hold GC-traced
references to unlinked executables/code and installed `CodeBlock`s. `CodeBlock`
holds the linked global object, scope, constants, metadata, runtime profiles,
and deferred tiering slots. `JsFunction` points to an executable and captured
scope.

### Mutation

Unlinked code mutates only during generation/finalization and cache
bookkeeping. Executables may install, clear, or replace code blocks through
VM/GC-aware APIs. `CodeBlock` may mutate inline caches, profiles, alternatives,
watchpoints, and metadata using write barriers.

### Unsafe Boundary

Unsafe is limited to GC allocation/tracing, write barriers, raw entrypoint
pointers, code replacement, cached-bytecode decode, and future JIT patching.

### Dependencies

This subsystem depends on bytecode schema, parser/source metadata, VM, heap,
rooting, global object, scopes, values, functions/calls, exceptions, debugger,
and profiler.

### Unresolved Questions

- Are unlinked artifacts GC cells or non-GC-owned objects?
- What is the cache serialization shape?
- What are code jettisoning semantics?
- What synchronization is required for concurrent compilation or cache lookup?
- How are disabled/deferred JIT fields represented?

## Interpreter / Register / CallFrame / VM Entry

### Proposed Responsibility

This subsystem defines VM entry, dispatch, register access, frame layout, stack
walking, and unwinding contracts so interpreter execution and later native
entrypoints can attach without changing ownership boundaries.

The interpreter is the reference execution engine. It is the semantic oracle for
bytecode, values, calls, scopes, exceptions, runtime calls, builtins, and control
flow. JIT-enabled execution must be compared against interpreter-only execution
until the relevant opcode family and runtime boundary are proven equivalent.

### Proposed Types

- `Interpreter`: dispatch and VM execution services.
- `VmEntryScope`: RAII-style VM entry guard for top frame/global/trap state.
- `EntryFrame`: host-to-VM frame record.
- `ProtoCallFrame`: pre-entry call frame description.
- `CallFrameLayout`: stable slot contract for caller, return PC, code block,
  callee, argc, this, args, and locals.
- `CallFrame<'vm>`: safe active-frame view.
- `FrameCursor`: constrained stack-walking view.
- `Register`: encoded JS value or frame/code pointer slot.
- `RegisterFile` or `StackSegment`: VM-owned stack storage.
- `DispatchPC`: typed bytecode program counter.
- `ExecutionResult<T>`: return value plus pending-exception awareness.

### Ownership

`Vm` owns stack storage and interpreter state. Entry scopes borrow the VM and
restore prior top-frame state on exit. Call frames are stack regions, not heap
owners. Active frame views borrow stack storage. Code blocks and values
referenced by frames must be rooted or conservatively visible to GC.

### Mutation

Only the active interpreter or entry path mutates registers in an active frame.
`topCallFrame`, `topEntryFrame`, trap state, and pending exception are VM-owned
mutable state. Stack walking reads frames through `FrameCursor`.

### Unsafe Boundary

Unsafe is limited to raw stack layout, frame-pointer casts, tagged return PCs,
ABI entry thunks, register unions, pointer tagging, and stack-capacity checks.
Safe Rust APIs should encapsulate all slot indexing and frame construction.

### Dependencies

This subsystem depends on bytecode schema, `CodeBlock`, `Executable`,
`JsValue`, GC rooting, function call setup, exceptions/unwind, debugger/profiler
hooks, and VM traps.

### Unresolved Questions

- C-loop dispatch or generated dispatch?
- What compatibility target is required for future JIT frames?
- What is the rooting discipline for frame-held values?
- Is tail-call frame reuse supported?
- How is OSR/checkpoint state represented?

## Functions / Calls / Constructors

### Proposed Responsibility

This subsystem represents callable and constructable entities, call metadata,
argument passing, `this` and `new.target` semantics, native function boundaries,
call-link profiling, and constructor allocation behavior.

### Proposed Types

- `JsFunction`: function object with executable and captured scope edges.
- `ExecutableBase`, `FunctionExecutable`, `NativeExecutable`: call/construct
  entry ownership.
- `CallData`: none/native/JS call target metadata.
- `ConstructData`: constructability and constructor entry metadata.
- `ArgList` and `MarkedArgList`: rooted borrowed argument storage.
- `CallMode`: regular, tail, construct, and varargs call forms.
- `ConstructorKind`, `ConstructAbility`, `ThisMode`: semantic function
  classification.
- `CallLinkInfo`: per-call-site runtime feedback and linked target state.
- `AllocationProfile`: constructor-created object structure/prototype cache.
- `HostFunction`: ABI wrapper for embedder/native callbacks.

### Ownership

`JsFunction` is a GC cell with executable and scope edges. Function rare data
owns lazy name/length/prototype/allocation-profile state. Executables own or
reference code blocks. Call-link metadata is owned by `CodeBlock`. Argument
lists borrow rooted stack or temporary storage.

### Mutation

Lazy function properties and rare data mutate through VM/write-barrier APIs.
Executable preparation may link or install code. Call sites may update
profiling, last-seen callee, monomorphic/polymorphic state, and varargs maxima.
Constructor profiles mutate only under structure/watchpoint rules.

### Unsafe Boundary

Unsafe is limited to native callback ABI, varargs frame materialization, direct
entrypoint invocation, call-link patching, pointer-tagged executable/rare-data
storage, and host exception handoff.

### Dependencies

This subsystem depends on object model, structures/prototypes, scopes,
environments, executables/code blocks, interpreter/VM entry, exceptions, GC
barriers, and global object.

### Unresolved Questions

- Trait-based callable abstraction or enum-based dispatch?
- What is the host callback API shape?
- How do proxy, bound, and remote functions integrate?
- Who owns allocation profile/watchpoint state?
- Where is class-constructor and construct-error enforcement handled?

## Exceptions

### Proposed Responsibility

Exceptions are modeled as VM state, not Rust panics. This subsystem owns thrown
values and stack traces, verifies throw propagation, and maps bytecode/native
frames to handlers during unwind.

### Proposed Types

- `Exception`: GC cell containing thrown `JsValue`, captured stack, and
  inspector-notified bit.
- `PendingException`: VM slot for current exception plus trap integration.
- `ThrowScope` and `ExceptionScope`: scoped permission and verification for
  throwing APIs.
- `JsResult<T>`: Rust-facing result type that cooperates with VM pending
  exception.
- `HandlerInfo` and `UnlinkedHandlerInfo`: linked/unlinked catch/finally tables.
- `CatchInfo`: resolved handler target and frame state.
- `Unwinder`: frame-walking handler lookup and debugger notification.
- `ErrorFactory`: constructors for TypeError, RangeError, TDZ, stack overflow,
  and other engine errors.
- `TerminationException`: distinguished exception that cannot be casually
  cleared.

### Ownership

`Vm` owns the current pending exception handle. `Exception` owns the thrown
value through a write barrier and owns captured stack frames. `CodeBlock` and
`UnlinkedCodeBlock` own handler tables. `ThrowScope` borrows the VM and does not
own the exception.

### Mutation

Throwing sets VM pending exception and trap state. Returning from throwing APIs
must preserve or explicitly clear pending exception. Catch/unwind transfers
control to the handler and clears or exposes the exception according to handler
semantics. Termination exceptions are protected from normal clearing.

### Unsafe Boundary

Unsafe is limited to unwinding over raw call frames and entry records, stack
capture, host callback exception handoff, and ABI paths that return an encoded
value while also setting VM exception state. Rust panics must not be used for
JavaScript control flow.

### Dependencies

This subsystem depends on VM traps, call frames, code-block handler tables,
bytecode indices/checkpoints, values, GC, error objects, debugger/inspector, and
host API.

### Unresolved Questions

- How strict should `JsResult<T>` be versus implicit VM pending state?
- What is the Rust equivalent of exception-scope verification?
- What are stack trace capture cost and ownership rules?
- How does async stack trace integration work?
- What are termination/watchdog semantics?

## Builtins

### Proposed Responsibility

Builtins are privileged engine code with stable private names, builtin indexes,
generated metadata, and controlled access to intrinsics.

### Proposed Types

- `BuiltinRegistry`: registry for builtin code and metadata.
- `BuiltinId`: generated builtin index.
- `BuiltinSource`: JS-authored or IR-authored builtin source.
- `BuiltinExecutableCache`: lazy executable cache.
- `BuiltinNames`: public, private, and well-known builtin names.
- `BuiltinPrivateName`: private builtin key.
- `BuiltinIntrinsic`: privileged intrinsic operation exposed to builtins.
- `BuiltinVisibility`: public, private, implementation-only, or inline-only
  visibility.

### Ownership

`Vm` owns builtin name tables and shared builtin source metadata. Executable
caches own or weakly retain unlinked builtin executables. Realms own
instantiated builtin functions and prototypes.

### Mutation

Builtin name tables are immutable after VM initialization except explicit
external-name registration. Builtin executables are created lazily and cached.
Builtin private names are not exposed through public property enumeration.

### Unsafe Boundary

Unsafe is limited to generated builtin tables, parser shortcuts or metadata
hand-computation, private-name lookup for builtin syntax, and future
ABI-visible builtin intrinsic call stubs.

### Dependencies

This subsystem depends on parser/source providers, identifiers/symbols,
unlinked executables, bytecode intrinsics, global object intrinsics, functions,
and VM caches.

### Unresolved Questions

- Keep JS-authored builtin source, lower builtins to Rust IR, or support both?
- How should validation compare generated metadata against parser behavior?
- Which builtin intrinsics may depend on host hooks?

## Modules / Host Loading

### Proposed Responsibility

Modules are a host-integrated typed state machine covering registry keys,
fetch/instantiate/link/evaluate, import attributes, dynamic import, top-level
await, namespace objects, and cached failures.

### Proposed Types

- `ModuleLoader`: realm-owned module loading coordinator.
- `ModuleKey`: resolved specifier plus module type/import attributes.
- `ModuleType`: JavaScript, JSON, Wasm, synthetic, or host-defined type.
- `ModuleRegistry`: map from `ModuleKey` to registry entries.
- `ModuleRegistryEntry`: status, record, promises, and cached errors.
- `ModuleStatus`: new, fetching, fetched, linking, linked, evaluating,
  evaluated, and error states.
- `ModuleRecord`: common module record abstraction.
- `SourceTextModuleRecord`: JavaScript source-text module.
- `CyclicModuleRecord`: status machine for cyclic modules and top-level await.
- `SyntheticModuleRecord`: host-created module.
- `ModuleRequest`: import request and attributes.
- `ModuleGraphLoad`: iterative graph-loading state.
- `HostModuleLoader`: host hook trait or ABI table.

### Ownership

A realm/global object owns its module loader. The loader owns registry entries
keyed by module identity and type. Registry entries keep module records,
promises, fetchers, and cached errors alive. Module records own import/export
tables, requested modules, environments, namespace objects, and async/TLA state.

### Mutation

Registry status transitions are explicit and monotonic except well-defined error
states. Graph algorithms should be iterative, not recursive. Resolution caches
store successful resolutions separately from registry failure state. Host
callbacks may complete asynchronously and reenter the engine.

### Unsafe Boundary

Unsafe is limited to FFI/host payloads, promise/microtask integration,
WebAssembly or JSON module handoff, and host-provided opaque fetcher data.

### Dependencies

This subsystem depends on identifiers, source providers, parser module
analysis, promises/microtasks, global object host hooks, module environments,
executable/code objects, and exception state.

### Unresolved Questions

- How are import attributes represented in `ModuleKey`?
- Is host loading a Rust trait, a C ABI table, or both?
- How are dynamic import and top-level await scheduled without baking in a
  specific event loop?

## Public Embedding API

### Proposed Responsibility

The embedding API provides a stable opaque boundary for context groups/VMs,
global contexts/realms, values, objects, classes, callbacks, exceptions,
protection/rooting, and script/module entry points.

### Proposed Types

- `ApiContextGroup`: opaque VM/context-group handle.
- `ApiGlobalContext`: opaque global context/realm handle.
- `ApiValueRef`: opaque value handle.
- `ApiObjectRef`: opaque object handle.
- `ApiStringRef`: opaque string handle.
- `ApiClass`: host-defined class metadata.
- `ApiCallbackObject`: host callback object data.
- `ProtectedValue`: protected/rooted API value.
- `ApiExceptionResult`: exception out-parameter bridge.
- `ApiLock`: API lock/entry discipline.

### Ownership

API handles are opaque references to VM, global object, value, object, string,
or class state. Context groups retain VM-like state. Global contexts retain
realm/global-object state. `ProtectedValue` roots GC values until balanced
unprotect/release.

### Mutation

Every API entry acquires or checks the API lock and establishes an entry scope.
Host callbacks may reenter. Exception out-parameters mirror pending-exception
state. Class/private-data callbacks must obey finalization restrictions.

### Unsafe Boundary

Unsafe is limited to C ABI handle casts, `JSValueRef` representation, callback
trampolines, class private data, finalizer calls, and cross-context-group
misuse.

### Dependencies

This subsystem depends on VM lifetime, GC rooting/protection, `JsValue`
representation, global object/realm, object model, strings, exceptions, host
callbacks, and modules for import APIs.

### Unresolved Questions

- Preserve the existing C API exactly with a Rust implementation, or define a
  Rust-native API plus compatibility shim?
- Is `JSValueRef` bit-compatible on 64-bit?
- How are locking and reentrancy enforced for embedders using multiple threads?

## Baseline JIT And Optimizing Tier Integration

### Proposed Responsibility

The first JIT requirement is a real baseline tier. It may be narrow, but it must
install through VM/code-block/tiering ownership, prove bytecode eligibility from
the owning `CodeBlock`, enter generated code for eligible bytecode, fall back to
the interpreter for unsupported bytecode, and share the interpreter's value
representation, frame model, root visibility, runtime-call path, exception path,
and GC/barrier contracts.

DFG, FTL, B3, and other optimizing tiers remain deferred. The skeleton reserves
the full execution abstraction:

```text
Executable -> CodeBlock -> entrypoint/profile/metadata/tiering state
```

`CodeBlock`-equivalent state must have reserved slots for JIT type, entrypoint,
inline caches, profiling data, OSR/tier-up state, watchpoint dependencies,
exception metadata, and code-liveness tracing. Interpreter semantics must not
depend on those fields being populated.

### Proposed Types

- `JitType`: interpreter thunk, baseline, DFG, FTL, or none.
- `Entrypoint`: abstract execution entry.
- `BaselineCodeRef`: installed baseline generated-code reference.
- `BaselineGeneratedCodeArtifact`: non-native typed generated-code body and
  proofs used before real executable memory exists.
- `JitCodeRef`: optimizing-tier compiled-code reference.
- `BaselineEligibility`: bytecode subset and safety decision for baseline entry.
- `BaselineFallbackSnapshot`: frame/register/bytecode-index state needed to
  resume in the interpreter.
- `InlineCacheSlot`: property/call cache attachment point.
- `TieringState`: counters, OSR metadata, and tiering policy.
- `WatchpointDependency`: optimized-code invalidation dependency.
- `CompilationPlan`: future concurrent compilation job.
- `CodeLiveness`: GC tracing and liveness state for code.

### Ownership

`CodeBlock` owns or references baseline and later JIT side data. Baseline code
does not own semantic state; it owns executable code plus metadata that maps
generated execution back to the shared frame, root, exception, and fallback
contracts. Compilation plans may hold weak or externally traced references to
code blocks and runtime objects. Watchpoint dependencies must be traced or
invalidated according to GC and structure rules.

### Mutation

Baseline side data mutates through VM-owned code-block APIs that state whether
the mutation installs, rejects, defers, invalidates, or falls back from generated
code. Installation must carry both an ABI proof and a bytecode-eligibility proof
for the same code-block owner before generated entry can be considered. Later
JIT side data follows the same ownership rule and must state whether an
operation is generated-code, compiler-thread, or main-thread visible. No
subsystem may bypass these APIs to simplify early execution.

### Unsafe Boundary

Unsafe is limited to generated code pointers, executable memory, patching,
register/stack ABI, barrier emission, code memory allocation, OSR entry/exit,
and compiler-thread interaction with GC-visible state.

### Dependencies

This subsystem depends on bytecode schema, code blocks, call frames, object
layout, value representation, GC barriers, watchpoints, profiling, and
exception metadata.

### Unresolved Questions

- Which backend produces the first baseline generated code?
- What is the shape of profiling and inline-cache side data?
- What concurrency model is used for compilation plans/worklists?
- Which layout offsets are reserved abstractly versus exactly?
- Which opcode family is the first baseline eligibility slice after frame/root
  ABI and install policy are stable?

## Wasm Integration Points - Deferred

### Proposed Responsibility

Wasm implementation is deferred. The module loader, host object model,
executable/code abstraction, call boundary, memory/table/global ownership model,
and public API surface must reserve Wasm extension points.

### Proposed Types

- `WasmModuleInfo`: parsed module information.
- `WasmModuleRecord`: module-system integration record.
- `WasmInstanceObject`: runtime instance object.
- `WasmMemoryObject`: memory wrapper.
- `WasmTableObject`: table wrapper.
- `WasmGlobalObject`: global wrapper.
- `WasmCalleeGroup`: compiled callee set.
- `JsToWasmBridge`: JavaScript-to-Wasm call bridge.
- `WasmToJsBridge`: Wasm-to-JavaScript call bridge.
- `WasmCompilationPlan`: future validation/compilation job.

### Ownership

Wasm module information may be non-GC engine data. Instance objects and public
wrappers are GC cells. Callee groups and compilation plans must participate in
GC liveness if they reference JS objects, code blocks, or instances.

### Mutation

Wasm states mutate through validation, instantiation, linking, memory/table
growth, imports/exports, and compilation-plan completion. While deferred, these
states may be unsupported placeholders, but the module/API/object contracts must
reserve their attachment points.

### Unsafe Boundary

Unsafe is limited to Wasm instance layout, memory/table raw access, generated
entrypoints, JS/Wasm bridge ABI, signal/fault handling, and compilation-plan
interaction with code memory.

### Dependencies

This subsystem depends on module loader, public API, functions/calls, object
model, GC, executable/code abstraction, JIT integration points, and host hooks.

### Unresolved Questions

- How do Wasm source types fail while implementation is deferred?
- Are Wasm instances GC cells with custom trailing layout or abstract host
  objects first?
- How do Wasm compilation plans relate to future JIT worklists?

## Rules For Future Agent Work

Agents may fill in implementation only behind documented contracts. Before
changing a public skeleton type, an agent must update:

- owned-state contract
- mutation permissions
- barrier/rooting requirements
- unsafe justification
- deferred JIT/Wasm/API impact
- unresolved-decision list

Temporary APIs that bypass GC, barriers, handles, exceptions, or layout
contracts are prohibited unless the skeleton marks them as deliberate
scaffolding with removal criteria.

Good agent tasks after this skeleton are bounded design expansions, for example:

- expand `Heap / GC / Roots / Barriers` into a dedicated ownership model
- define the `JsValue` representation decision tree
- design `Structure` transitions and watchpoint state
- design parser arena lifetimes and AST handle types
- design the bytecode schema generator contract

Bad agent tasks remain:

- make a JavaScript program execute end-to-end
- port a C++ subsystem line by line
- add broad `Rc<RefCell<_>>` usage to bypass ownership questions
- introduce undocumented unsafe code
- remove deferred JIT/Wasm/API hooks to simplify local implementation
