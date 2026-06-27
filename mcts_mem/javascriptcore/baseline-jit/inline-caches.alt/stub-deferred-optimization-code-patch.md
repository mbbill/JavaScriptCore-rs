- The first IC use defers optimization by patching the call address to a second stub.

## Moves

- 2009-07-31 (4f8fbdbc) replaced by [[inline-caches]]: On ASSEMBLER_WX_EXCLUSIVE builds (W^X memory) the first-call code-patch (ctiPatchCallByReturnAddress to a _second stub) requires making executable memory writable, which is expensive; a data flag in StructureStubInfo eliminates the patch on first call and improves WX-exclusive performance by 2-2.5%. (sourced)
