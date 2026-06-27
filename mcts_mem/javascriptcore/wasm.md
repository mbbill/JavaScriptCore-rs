- Wasm keeps a validated, embedder-neutral module representation whose metadata can be shared across instances and compilation work. (`ModuleInformation`)
- Function bodies are parsed through a shared function-parser abstraction that feeds validation, interpreters, and JIT backends from the binary stream.
- Streaming compilation parses by section and function units and accepts incremental byte delivery.
- Optional names, source bytes, and opcode origins are retained only where stack traces, diagnostics, profiling, or debugger-visible execution need them.
- Debugger/profiler integration identifies Wasm frames by function index or name and maps sampled PCs back to wasm opcode/source offsets.
- LLDB-facing Wasm debugging uses a synthetic module/instance address space and a GDB/RWI packet layer.
- Feature use is tracked per function with flags for SIMD, exceptions, atomics, and related proposals.

## Facts

- 2016-08-15 (68f6d162) rationale: the first parse/compile pipeline split module scanning from per-function decoding because the design was intended to make as much compilation work as possible independently threadable later (sourced).
- 2016-12-20 (045d5b2a) rationale: VM-lifetime signature indices avoid per-module pointer identity and make duplicate type-section signatures comparable with one integer check (sourced).
- 2017-04-05 (5c40c80b) statement: names are stored as UTF-8 byte vectors and decoded to thread-local Strings on demand; most modules have few, mostly ASCII, names (sourced).
- 2018-08-28 (5a417755) pitfall: the streaming parser may not have the complete byte buffer when it detects an error, so streaming errors do not report total byte size (sourced).
- 2019-12-05 (da014010) measurement: merging serial validation into the concurrent compile pass produced a 1.5x compile-time speedup on the ZenGarden WebAssembly demo (sourced).
- 2021-12-12 (f3993d57) rationale: sampling-profiler origin maps encode the Wasm opcode location in CodeOrigin::bytecodeIndex and keep the map with the live Wasm callee registry so sampled frames resolve back to wasm bytecode (code).
- 2025-09-26 (e03c1022) rationale: LLDB-facing addresses use a synthetic 64-bit address space whose top bits distinguish instance linear memory from module bytecode and whose remaining bits carry a module/instance id plus offset (code).
- 2025-10-24 (e116bf50) statement: the RWI debugger uses WorkQueue command handling and stop-the-world pause semantics, leaving simultaneous Web Inspector JavaScript debugging unsupported while LLDB controls Wasm execution (sourced).
- 2026-06-17 (16828c4b) measurement: the single-buffer expression-stack parser layout matched JetStream 3 data showing no stack-argument traffic into else blocks, 87% of ifs without else, and P99 maximum function stack depth of 15 (sourced).

## Moves

- 2017-04-05 (5c40c80b) replaced [[wasm-module-information-owned-decoded-strings]]: ModuleInformation became ThreadSafeRefCounted with raw source bytes and byte-vector names so parsed module metadata could be shared across threads instead of owning JS Strings and ArrayBuffer state tied to one thread. (code)
- 2018-08-28 (5a417755) replaced [[wasm-monolithic-module-parser]]: The monolithic ModuleParser required all wasm bytes to be available before parsing could start; the new streaming parser accepts bytes incrementally via addBytes(), using a state machine with Section as the unit of incrementalism and Function as a finer unit inside the Code section, enabling concurrent compilation while parsing continues. (code)
- 2019-12-05 (da014010) replaced [[wasm-two-pass-validate-then-compile]]: Merged serial validation pass into the concurrent bytecode-generation pass so that all functions are validated and compiled in one concurrent traversal instead of a serial validate step followed by a concurrent compile step, yielding a 1.5x compile-time speedup on ZenGarden. (sourced)
- 2023-03-01 (43f182b3) replaced [[wasm-per-function-simd-flag]]: A single SIMD-only per-function flag could not represent the additional exception and atomic feature predicates needed to switch behavior by wasm function feature. (code)
- 2026-01-15 (5ad2efd1) replaced [[wasm-block-signature-as-function-signature]]: Block signatures that are just a single result type no longer need synthetic FunctionSignature/TypeDefinition allocation or lookup because BlockSignature can now store either a module FunctionSignature pointer or the inline result Type directly. (code)
- 2026-06-17 (16828c4b) replaced [[wasm-function-parser-per-control-expression-stacks]]: The function parser replaced per-control-block expression-stack vectors with one contiguous expression stack plus begin offsets to avoid keeping N live vectors and copying/swapping stack slices at every nested control boundary. (sourced)
