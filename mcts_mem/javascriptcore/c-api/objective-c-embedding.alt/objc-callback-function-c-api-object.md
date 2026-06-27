- Objective-C callback functions are exposed as generic C API objects with custom class callbacks.
- JavaScript observes those callbacks as objects rather than as function cells.

## Moves

- 2013-03-14 (df22c6d6) replaced by [[objective-c-embedding]]: Implementing ObjCCallbackFunction as a JSClassRef C-API object gave it typeof 'object' instead of 'function', and Function.prototype.toString failed; subclassing JSCallbackFunction (a JSCell) gives the correct JS type and prototype chain membership. (sourced)
