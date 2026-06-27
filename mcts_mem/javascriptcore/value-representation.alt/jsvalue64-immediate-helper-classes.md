- JSVALUE64 kept JSImmediate and JSNumberCell helper classes for tag constants and constructors.
- Some JSValue constructors were split through JSNumberCell friendship.
- Empty implementation files remained for helper classes with no live behavior.

## Moves

- 2011-04-11 (4c7f9e1b) removed: JSImmediate and JSNumberCell on JSVALUE64 contained only uncalled/dead methods and JSValue constructors split across unnecessary layers; collapsing them into JSValue.h and JSValueInlineMethods.h removes indirection while keeping JSVALUE32_64 and JSVALUE64 implementations unified in one header. (sourced)
