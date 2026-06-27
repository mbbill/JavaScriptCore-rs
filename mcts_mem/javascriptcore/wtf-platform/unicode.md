- WTF Unicode exposes string-level case mapping and character classification behind backend-specific inline implementations.
- Unicode backends are self-contained enough for ports to map directly to ICU, Qt, or GLib primitives without sharing mismatched neutral enum headers.
- JSC string code uses WTF's `UChar` typedef and Unicode helpers rather than carrying a KJS-specific character wrapper.
- ECMAScript internationalization data may be inlined when the specification names a data source that differs from the platform ICU behavior.

## Moves

- 2006-04-08 (d97e5da8) replaced [[unicode-case-map-char-level]]: Character-by-character case mapping cannot produce multi-character results required by Unicode special casings (e.g. German ß → SS), so toLowerCase/toUpperCase failed to honor these mappings; string-level ICU u_strToLower/u_strToUpper operates on the whole string and can expand characters. (sourced)
- 2006-12-09 (47982b54) replaced [[wtf-unicode-shared-enum-headers]]: The old design shared neutral enum-only headers (UnicodeCategory.h, UnicodeDirection.h, UnicodeDecomposition.h) across backends, but this prevented Qt4 from mapping its enums directly to QChar values; the new design makes each backend self-contained with enums and all inline implementations in one header, which allows Qt4 to alias e.g. LeftToRight = QChar::DirL and eliminates the separate UnicodeQt4.cpp dispatch file. (code)
- 2008-03-10 (834542d4) replaced [[kjs-uchar-struct]]: KJS::UChar was a custom struct wrapping unsigned short with a .uc member field; replaced with WTF's ::UChar (a typedef for unsigned short from wtf/unicode/Unicode.h) to eliminate redundant type definition and remove all .uc member-access sites across the codebase. (code)
- 2009-05-22 (cfb136b6) replaced [[wtf-unicode-icu-only-backend]]: The GTK port needed to reduce the ICU dependency footprint; adding USE(GLIB_UNICODE) as a third dispatch branch in Unicode.h (alongside Qt4 and ICU) allowed the WTF Unicode layer to be implemented on GLib while text codecs and TextBreakIterator remain ICU-based via the hybrid macro. (sourced)
- 2017-03-15 (0dc886f9) replaced [[icu-currency-fraction-digits]]: ECMA-402 specifies ISO 4217 as the CurrencyDigits data source, so ICU's CLDR-backed default fraction digits were replaced by an inline ISO 4217 minor-unit table. (sourced)
