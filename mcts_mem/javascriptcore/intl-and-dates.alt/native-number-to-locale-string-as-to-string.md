- Number.prototype.toLocaleString was a native function that converted the number to a plain string.
- Intl.NumberFormat was not used by the Intl-enabled builtin path.

## Moves

- 2015-12-05 (fec33f13) replaced by [[intl-and-dates]]: Add toLocaleString in builtin JavaScript that delegates formatting to Intl.NumberFormat. Keep exisiting native implementation for use if INTL flag is disabled. (sourced)
