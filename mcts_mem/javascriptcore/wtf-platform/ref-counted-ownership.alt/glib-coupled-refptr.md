- The platform smart pointer is a GLib-specific `GRefPtr` template.
- Reference and dereference operations are hard-wired to the GLib object system.
- Non-GLib ports need separate smart-pointer machinery for their platform objects.

## Moves

- 2010-08-25 (60d2a851) replaced by [[ref-counted-ownership]]: GRefPtr<T> hard-wired ref/deref to GLib object system, blocking use on non-GLib platforms (EFL, Cairo); PlatformRefPtr<T> separates the smart-pointer template from platform-specific refPlatformPtr/derefPlatformPtr hooks so Cairo and EFL ports can adopt it without a GLib dependency. (sourced)
