- Temporal ICU bridge helpers encoded failures as sentinels or asserted successful ICU calls.
- Callers interpreted INT32_MIN, -1, or unchecked return values alongside real calendar data.

## Moves

- 2026-06-04 (178eab31) replaced by [[temporal]]: Temporal's ICU bridge replaced unchecked/sentinel-based helpers with explicit optional and TemporalResult propagation because each ICU call carrying UErrorCode can fail and callers need to surface a controlled Temporal error instead of asserting or interpreting sentinel values. (sourced)
