- Temporal calendar identity was represented by TemporalCalendar JS objects and LazyProperty fields on Temporal objects.
- PlainDate carried a lazy calendar object rather than a compact CalendarID.

## Moves

- 2026-05-26 (b35d67a6) replaced by [[temporal]]: Migrate calendar representation from JS/LazyProperty calendar objects to CalendarID fields to eliminate repeated String round-trips and enable compile-time calendar dispatch via CalendarICUBridge. (sourced)
