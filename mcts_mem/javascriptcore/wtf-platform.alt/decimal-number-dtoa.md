- Number-to-string conversion is implemented by an in-tree DecimalNumber helper.
- UString formatting calls DecimalNumber paths for exponential, fixed, and generic double conversion.
- JavaScript numeric printing does not depend on an external dtoa library.

## Moves

- 2011-09-02 (5a9627ad) replaced by [[wtf-platform]]: Old in-tree DecimalNumber/numberToString replaced by google double-conversion library (code.google.com/p/double-conversion) because the new library is faster for number-to-string conversion. (sourced)
