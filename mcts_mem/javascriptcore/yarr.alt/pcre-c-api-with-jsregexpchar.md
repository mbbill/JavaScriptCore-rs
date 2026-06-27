- JSRegExpChar typedef (unsigned short)
- int options bitfield passed to jsRegExpCompile
- pcre_malloc/pcre_free function pointers
- FastMallocPCRE.cpp shim wrapping fastMalloc for C code
- pcre_maketables returning malloc'd block freed by caller
- .c source files (pcre_compile.c, pcre_exec.c, pcre_maketables.c, etc.)

## Moves

- 2007-11-11 (74afd231) replaced by [[yarr]]: The PCRE C-language layer used a custom JSRegExpChar (unsigned short) type and a combined int options bitfield; converting to C++ with UChar and typed enum parameters eliminated the type mismatch with WebKit's UTF-16 strings and enabled C++ new/delete, removing the pcre_malloc/free indirection and FastMallocPCRE.cpp shim. (code)
