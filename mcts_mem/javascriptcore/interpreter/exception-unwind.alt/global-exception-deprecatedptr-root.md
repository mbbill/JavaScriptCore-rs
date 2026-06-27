- The VM's global exception slot is stored in DeprecatedPtr<Unknown>.
- The slot acts as a GC root by convention rather than by type.
- Writes bypass normal barrier semantics without a root-specific wrapper.

## Moves

- 2011-03-01 (b740d069) replaced by [[exception-unwind]]: The global exception slot in JSGlobalData held a DeprecatedPtr<Unknown> which did not correctly classify the slot as a GC root (exempt from write-barrier requirements); GCRootPtr<T> is introduced as a WriteBarrierBase<T> subclass that uses setWithoutWriteBarrier() unconditionally, making the GC-root nature explicit in the type system while hiding the write-barrier-bypass from non-root slots. (code)
