- `JSContext` owns a counted set of protected JS values.
- Clients protect and unprotect values through the context rather than each value wrapper protecting itself.

## Moves

- 2013-01-17 (50834c77) replaced by [[objective-c-embedding]]: JSContext's m_protectCounts/protect/unprotect mechanism existed so the context could unprotect values before going away; once JSValue retains the context (keeping context alive as long as any value lives), the context-side lifecycle management is dead code and JSValue can protect/unprotect itself directly in init/dealloc. (sourced)
