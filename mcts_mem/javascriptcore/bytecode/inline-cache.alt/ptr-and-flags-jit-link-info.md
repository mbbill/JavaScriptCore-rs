- PtrAndFlags<CodeBlock, HasSeenShouldRepatch> ownerCodeBlock in CallLinkInfo
- PtrAndFlags<Structure, HasSeenShouldRepatch> cachedPrototypeStructure in MethodCallLinkInfo
- isFlagSet/setFlag API on PtrAndFlags<>

## Moves

- 2010-01-18 (f1d57724) replaced by [[inline-cache]]: PtrAndFlags<> hides pointer bits from the OS X Leaks tool (which scans memory for recognizable pointers), breaking leak detection; the replacement uses a plain C++ bitfield member for CallLinkInfo and a sentinel pointer value (MethodCallLinkInfo_seenFlag = (Structure*)1) encoding state in cachedPrototypeStructure for MethodCallLinkInfo. (sourced)
