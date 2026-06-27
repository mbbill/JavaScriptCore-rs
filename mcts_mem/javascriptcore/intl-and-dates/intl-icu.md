- Intl objects store ICU handles or ICU-derived locale identifiers on each object instance.
- ICU C APIs, not ICU C++ builders, are the boundary for locale canonicalization, pattern generation, collation, segmentation, and formatting.
- Per-VM IntlCache stores bounded canonical-locale and date-time-pattern-generator caches.
- ICU data is converted into ECMAScript-visible options at object initialization or resolvedOptions time, not reinterpreted by callers.

## Facts

- 2015-12-23 (e20ee4f8) rationale: Intl.DateTimeFormat initialization stores resolved locale, calendar, numbering system, time zone, hour12, date/time fields, and a UDateFormat for formatting and resolvedOptions. (code)
- 2020-05-06 (e1048fb5) rationale: Intl.Locale uses ICU's C API even though ICU's C++ LocaleBuilder would make construction easier, so lazy getters and cached immutable locale IDs compensate at the JSC boundary. (sourced)
- 2021-08-21 (b2eeba9a) rationale: Intl.Locale hourCycles asks ICU for the best pattern for skeleton `j` and reads the hour-field symbol instead of maintaining a locale table. (code)
- 2026-05-26 (99cab5c0) measurement: Canonicalized locale ID caching leaves first lookup and failures on ICU while memoizing repeated successful small ASCII tags per VM. (sourced)
