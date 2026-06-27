- `JSValueRef` is a `void*`-style opaque handle.
- The C type system does not distinguish object references from value references at API call sites.

## Moves

- 2006-07-10 (be86fd6e) replaced by [[c-api]]: JSValueRef changed from void* to const struct __JSValue* and JSObjectRef from struct __JSObject* to struct __JSValue* to gain C type-safety: the compiler now rejects passing a JSObjectRef where a JSValueRef is expected and vice versa, and revealed numerous existing bugs in the API implementation. (sourced)
