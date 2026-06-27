- `JSContext` stores its pending exception as a retained `JSValue` Objective-C property.
- The exception wrapper participates in Objective-C retain cycles with its context.

## Moves

- 2013-01-31 (6db3421f) replaced by [[objective-c-embedding]]: Storing the exception as a retained `JSValue*` ObjC property created a strong reference cycle because JSValue holds a strong reference back to its owning JSContext; replacing with a JSC::Strong<JSC::JSObject> GC handle breaks the cycle since it does not participate in ObjC ARC/retain. (sourced)
