- Wasm parser and validation helpers return Expected-style values with cold error construction rather than synchronizing success with side-channel error strings.
- Function-body helpers own block-signature, lane-index, name, and feature-validation details that are only needed while parsing wasm bodies. (`FunctionParser`)
- Block signatures are normalized to inline single-result types or module function-signature references before validation consumers use them.
- Name-section UTF-8 validation scans bytes directly and keeps validated UTF-8 bytes rather than converting through a discarded UTF-16 buffer.
- Parser diagnostics, debugger names, and library metadata are escaped or validated at the boundary where external tools consume them.

## Facts

- 2016-12-15 (1ed42e5a) rationale: the Expected-based error path is intentionally marked NEVER_INLINE and guarded with UNLIKELY so success paths avoid string formatting and register allocation can keep error handling cold. (sourced)
- 2018-02-02 (a9603d33) pitfall: custom sections must not become the previous section used for WebAssembly known-section ordering; track previousKnownSection separately, decode section byte 0 as Custom, and reject nonzero unknown section ids before parsing the section body. (code)
- 2023-01-26 (3e28bdb0) rationale: indexed reference-type block signatures have to be created after the type section is read rather than pre-populated in TypeInformation, so the attempted representation made BlockSignature a RefPtr to keep synthesized signatures alive during validation. (sourced)
- 2023-11-28 (9a518a55) pitfall: Wasm block signatures must be normalized to FunctionSignature pointers: one-byte reference result signatures are parsed into synthetic one-result function signatures, and type-index block signatures are rejected unless they expand to a function type. (code)
- 2025-11-13 (4868790b) rationale: moving block-signature and lane-index parsing helpers from ParserBase to FunctionParser reduces the surface affected by future parser-validation elision because those helpers are only used while parsing function bodies. (sourced)
- 2026-04-12 (cfed70b0) rationale: Wasm name parsing validates UTF-8 with checkUTF8WithoutUTF16Length because it only needs byte validity before copying the UTF-8 bytes into Name, not a computed UTF-16 length. (code)
- 2026-04-15 (de5a7d70) pitfall: Wasm library names derived from source URLs and name sections must be XML-escaped for &, <, >, and quote before being written into qXfer:libraries XML; the debug name is cached on ModuleDebugInfo so repeated library-list reads reuse it. (code)

## Moves

- 2024-04-28 (f5b6bcdb) replaced [[wasm-name-utf8-validate-by-utf16-conversion]]: Wasm name validation uses checkUTF8 because it is more efficient than converting non-ASCII UTF-8 into a UTF-16 buffer and then discarding the buffer. (code)
