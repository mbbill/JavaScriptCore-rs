- Object construction receives an inline `JSObjectCallbacks` struct copied into each callback object.
- Static properties and functions are not represented as shared class-level tables.
- Class inheritance is expressed through parent callback pointers rather than ref-counted class descriptors.

## Moves

- 2006-07-02 (fe2e681b) replaced by [[c-api]]: JSObjectCallbacks inline struct embedded per-object (stored as m_callbacks copy) could not express static property/function tables shared across objects, forced objects onto the oversized heap, and could not support class-level inheritance chains; JSClassRef is a ref-counted shared class object with HashMap<UString,StaticValueEntry> and HashMap<UString,StaticFunctionEntry> tables, removing the need for a getProperty callback for most lookup cases. Specialized subclasses JSCallbackFunction and JSCallbackConstructor preserve JSFunctionMake/JSConstructorMake without creating a custom class per call. (sourced)
