- Locale-sensitive standard-library behavior delegates to Intl objects and ICU rather than duplicating locale rules in the core builtin surface.
- Date remains a legacy double-millisecond object with cached Gregorian expansion, while Temporal uses value-typed ISO and ICU bridge layers.
- Per-VM Intl caches memoize expensive ICU locale and date-time-pattern work with bounded freshness.

## Facts

- 2020-05-06 (e1048fb5) rationale: Intl.Locale wraps ICU uloc.h because JSC sticks to ICU's C API; immutable locale ID data lets methods and getters be lazy and cache results. (sourced)
- 2020-09-16 (00358905) measurement: Caching the last-used UDateTimePatternGenerator per VM improved repeated one-locale toLocaleString/date/date-time microbenchmarks by 4.4894x, 5.3669x, and 5.5388x. (sourced)
- 2026-05-26 (99cab5c0) measurement: ICU locale canonicalization cost about 640 ns per tag and about 37% of new Intl.NumberFormat("en-US"), so IntlCache memoizes repeated successful small ASCII tags per VM. (sourced)

## Moves

- 2015-12-05 (fec33f13) replaced [[native-number-to-locale-string-as-to-string]]: Add toLocaleString in builtin JavaScript that delegates formatting to Intl.NumberFormat. Keep exisiting native implementation for use if INTL flag is disabled. (sourced)
- 2015-12-23 (e1037229) replaced [[locale-compare-native-cpp]]: When INTL is enabled, localeCompare is implemented as a JS builtin that delegates to Intl.Collator, because the builtin can call the Collator JS API directly and share the prototype instance for the no-argument fast path, while the C++ implementation could not access Intl.Collator. (sourced)
