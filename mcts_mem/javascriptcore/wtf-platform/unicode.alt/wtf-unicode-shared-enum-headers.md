- Unicode category, direction, and decomposition enums live in neutral shared headers.
- Backends map from the shared enum values instead of declaring their own inline-compatible values.
- Qt4 cannot directly alias WTF Unicode enums to QChar enum constants.

## Moves

- 2006-12-09 (47982b54) replaced by [[unicode]]: The old design shared neutral enum-only headers (UnicodeCategory.h, UnicodeDirection.h, UnicodeDecomposition.h) across backends, but this prevented Qt4 from mapping its enums directly to QChar values; the new design makes each backend self-contained with enums and all inline implementations in one header, which allows Qt4 to alias e.g. LeftToRight = QChar::DirL and eliminates the separate UnicodeQt4.cpp dispatch file. (code)
