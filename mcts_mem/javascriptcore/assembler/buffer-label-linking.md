- Assemblers emit into growable buffer storage and LinkBuffer owns or copies that storage when producing executable code.
- Labels, jumps, and calls are typed offset tokens until finalization; they do not become raw executable addresses while bytes are still movable.
- LinkBuffer owns post-emission metadata that must survive MacroAssembler lifetime, including comments, labels, relocation state, and finalization bookkeeping.
- Buffer storage is optimized for short-lived assembler instances: small inline buffers, owned raw buffers, and thread-local reusable AssemblerData reduce allocation churn.

## Facts

- 2008-12-04 (8ef40511) measurement: switching from a shared 1MB AssemblerBuffer singleton to per-instance 256-byte inline buffers that grow dynamically produced a 1% SunSpider progression. (sourced)
- 2014-07-15 (9e4a9a39) measurement: making raw assembler data movable cut AssemblerBuffer::grow to an 8-Thumb-instruction path, halved ARMv7 time spent in Assembler, reduced CSS JIT compilation time by about 20%, and had no measurable x86_64 difference. (sourced)
- 2016-03-26 (42a6dd80) rationale: per-instruction LocalWriter keeps the buffer pointer and index local while emitting bytes, avoiding byte-by-byte AssemblerBuffer writes that the compiler could not prove non-aliasing. (sourced)
- 2016-03-26 (42a6dd80) measurement: LocalWriter-based x86 assembler emission reduced binary size by 66k and produced a small SunSpider speed-up. (sourced)
- 2018-08-19 (43663faf) pitfall: AssemblerBuffer and X86Assembler raw reinterpret_cast loads/stores operated on potentially unaligned emission bytes; WTF::unalignedLoad/unalignedStore avoids C++ UB while preserving efficient x86 moves. (code)
- 2020-05-31 (c1df8dc6) rationale: a thread-local AssemblerData cache reduces AssemblerBuffer allocation churn, with storage destroyed on another thread cached by the destroying thread. (sourced)
- 2026-01-16 (018769c9) pitfall: named offlineasm LabelReference values that resolve to globally defined labels must lower through the global/external label-reference macro, not the local-label macro, or x86 references an undefined local symbol form. (sourced)

## Moves

- 2011-05-01 (b2d50241) replaced [[per-assembler-jmpsrc-jmpdst-types]]: Per-assembler JmpSrc/JmpDst classes predated the MacroAssembler abstraction; having them per-assembler caused code duplication, prevented AssemblerBuffer from providing a richer shared label type, and their semantic meaning was already undermined (JmpSrc overloaded for Call, JmpDst for data labels; ARMv7 JmpSrc carrying extra jump-type/condition data that could not fit cleanly in the base). (sourced)
- 2020-06-02 (946309b4) replaced [[threadspecific-manual-construction-and-clear-hooks]]: Manual placement construction and thread-stopping clear hooks were removed because ThreadSpecific<T> constructs T on operator*/operator-> and runs destructors when the thread goes away. (sourced)
- 2021-02-02 (48f29377) replaced [[raw-assembler-label-offset]]: AssemblerLabel now hides its offset behind accessors so ARM64E can store m_offset as a pointer-authenticated tagged integer while non-ARM64E keeps the raw uint32_t layout. (code)
- 2022-09-06 (ad2e13b2) replaced [[single-jit-comment-per-address]]: WASM BBQ disassembly needs to annotate one machine-code address with multiple compiler events rather than rejecting duplicate comments. (code)
