- Temporal core objects store ISO date/time/exact-time value structs and carry calendar identity separately.
- Non-ISO calendar operations route through CalendarICUBridge keyed by compact CalendarID values.
- Time-zone resolution represents a local date-time as gap, single instant, or fold pair before disambiguation.
- Temporal pure-core and ICU bridge code return TemporalResult values instead of throwing JS exceptions directly.

## Facts

- 2021-09-01 (158e7091) pitfall: Temporal.Duration construction must release throw-scope state when returning tryCreateIfValid because that helper can throw. (code)
- 2026-05-11 (15fb1d89) rationale: Time-zone gap disambiguation stores before and after offsets so earlier, later, and compatible instants can be computed without additional ICU lookups. (code)
- 2026-06-11 (7830cc63) rationale: CalendarICUBridge initializes cache entries under the entry lock and stores UCalendar in unique_ptr, avoiding separate atomic initialized flags and raw pointer lifetime. (code)

## Moves

- 2026-05-26 (b35d67a6) replaced [[temporal-calendar-object-representation]]: Migrate calendar representation from JS/LazyProperty calendar objects to CalendarID fields to eliminate repeated String round-trips and enable compile-time calendar dispatch via CalendarICUBridge. (sourced)
- 2026-06-04 (178eab31) replaced [[temporal-icu-sentinel-and-assert-error-handling]]: Temporal's ICU bridge replaced unchecked/sentinel-based helpers with explicit optional and TemporalResult propagation because each ICU call carrying UErrorCode can fail and callers need to surface a controlled Temporal error instead of asserting or interpreting sentinel values. (sourced)
