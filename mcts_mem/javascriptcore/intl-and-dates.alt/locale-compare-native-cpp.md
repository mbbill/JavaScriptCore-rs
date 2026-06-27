- String.prototype.localeCompare was a native C++ function under the Intl-enabled build.
- The C++ implementation could not directly use the Collator JS API or the default prototype instance.

## Moves

- 2015-12-23 (e1037229) replaced by [[intl-and-dates]]: When INTL is enabled, localeCompare is implemented as a JS builtin that delegates to Intl.Collator, because the builtin can call the Collator JS API directly and share the prototype instance for the no-argument fast path, while the C++ implementation could not access Intl.Collator. (sourced)
