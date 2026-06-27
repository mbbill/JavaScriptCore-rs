- Standard-library algorithms can be authored as self-hosted JS builtins, then parsed into builtin executables with private intrinsics when plain JS cannot express the VM operation.
- Builtin JS source is generated into compact runtime metadata: a combined source provider, builtin indexes, private-name bindings, and executable arrays.
- LLInt and IPInt assembly are authored in offlineasm, a macro-assembly DSL lowered into target-specific assembly or C-loop output.
- Code generation extracts C++ layout, bytecode metadata, and build configuration at build time rather than hardcoding offsets in generated sources.
- Offlineasm target backends hide ABI, object-format, label, relocation, and alignment details behind shared instruction semantics.

## Facts

- 2012-02-21 (c9fc3858) rationale: A modified macro assembly plus offline assembler was chosen because LLInt needed macro-language conveniences and direct access to C++ field offsets and type sizes. (sourced)
- 2012-02-22 (7dc7faa4) rationale: The offline assembler was chosen so LLInt macro assembly could use a Turing-complete macro language and C++ layout information such as offsets and sizes. (sourced)
- 2012-03-04 (3fa9e640) pitfall: When ENABLE(JIT) is false, LLIntOffsetsExtractor has no offset magic values, so offlineasm must treat missing magic values as a classic-interpreter skip rather than fatal extraction failure. (code)
- 2015-04-24 (d42943a4) measurement: The JavaScript sort rewrite was faster or unchanged on tracked benchmarks, including 3x faster random input with a comparator and 4x faster random input on non-array objects, with compact integer arrays regressing 2x. (sourced)
- 2015-05-04 (e0e0dca9) rationale: Array sort's merge comparator order was kept simple instead of adopting timsort-style ordered-run detection. (sourced)
- 2018-10-12 (a166627c) rationale: Separating settings extraction from offset extraction avoids generating offsets for every configuration cross-product. (sourced)
- 2019-01-21 (4c257bce) rationale: All builtin JS functions share one concatenated SourceProvider so each builtin uses a substring SourceCode and avoids per-builtin source-provider allocation. (sourced)
- 2026-01-10 (8e462693) pitfall: Builtin executable metadata validation must compare functionStart as well as name and parameter positions because async builtins report the start of the async keyword. (code)

## Moves

- 2014-02-12 (fa5f5a32) replaced [[cxx-array-prototype-every]]: Array.prototype.every was moved from a hand-written C++ host function to a generated JS builtin function so builtins can be authored in JS while still mimicking host-function behavior at the API boundary. (code)
- 2014-02-15 (bb19dd1f) replaced [[inline-cpp-offlineasm-llint-output]]: Windows LLInt adopted standalone MASM-compatible Intel-syntax assembly output instead of inline C++ assembly so Microsoft assembler builds could process it and the path could support 64-bit. (sourced)
- 2015-04-24 (d42943a4) replaced [[cxx-array-sort-specializations]]: Array.prototype.sort moved from C++ specializations to a JavaScript builtin because JavaScript made the operation simpler and less error-prone while providing memory safety, exception safety, and recursion safety. (sourced)
- 2019-03-11 (12d53564) replaced [[builtin-executables-weak-per-member]]: Each Weak<UnlinkedFunctionExecutable> requires a WeakBlock (256 bytes) for GC bookkeeping; with 203 builtins plus 203 SourceCode members the old design consumed ~4KB of WeakBlocks plus 24*203=4KB of SourceCode fields, whereas a raw pointer array plus finalizeUnconditionally() scan replicates JSWeakSet behavior with no WeakBlock overhead. (sourced)
- 2024-02-16 (8f9efa2d) replaced [[raw-emitted-exported-offlineasm-labels]]: Raw emitted .globl labels bypassed offlineasm alt_entry support, while ordinary offlineasm global labels hid symbols, so exported LLInt entry points needed a DSL case that keeps alt_entry generation and export visibility independent. (code)
- 2024-04-05 (75713dba) replaced [[manual-ipint-alignment-emits]]: Offlineasm labels gained an alignment operand and C++-referenced validation labels because hand-emitted .balign padding could not make the label global/referenced, and the commit message says LTO linkers removed unreferenced labels. (sourced)
- 2026-02-24 (586f364e) replaced [[offlineasm-arm64-pcrtoaddr-single-adr]]: A single arm64 adr lowering is only reliable for local labels; extern or global labels need an adrp/add pair with platform-specific relocation syntax. (sourced)
- 2026-05-31 (17e27ee7) replaced [[builtin-js-custom-at-annotations]]: The custom builtin annotation and BytecodeIntrinsic constant mechanism could not express compile-time flags or simple named constants, so builtin JS sources were routed through a C preprocessor with JSC_BUILTIN_* macros. (sourced)
