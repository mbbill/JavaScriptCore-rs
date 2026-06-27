- UString m_flags member in RegExp
- const UString& flags() const accessor in RegExp
- m_flags(flags) initializer in RegExp constructor taking flags parameter

## Moves

- 2010-01-09 (3d2dfd7e) removed: The m_flags UString field was never read after construction — flag information is reconstructed on demand from the numeric m_flagBits bitfield — so storing the original flags string was pure redundant allocation per RegExp instance. (sourced)
