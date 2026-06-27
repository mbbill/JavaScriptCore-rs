- Noncopyability is expressed by inheriting an empty Noncopyable base class.
- Client classes may combine the noncopyable base with other empty bases such as fast-allocation helpers.
- Object layout is left to C++ empty-base rules.

## Moves

- 2010-09-27 (3af4b634) replaced by [[wtf-platform]]: The Itanium C++ ABI forbids two empty base classes of the same type at the same offset, so inheriting both Noncopyable and FastAllocBase could silently inflate object sizes (String grew by sizeof(void*)); a macro avoids any base-class footprint entirely. (sourced)
