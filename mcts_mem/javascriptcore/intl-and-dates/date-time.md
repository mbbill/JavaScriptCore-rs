- Date values are stored as double millisecond timestamps and expanded lazily into Gregorian calendar fields.
- Date caches are VM-owned and reset together with timezone and parsed-string cache state.
- Local-time conversion routes through platform-specific safe primitives while retaining ECMAScript Date's double timestamp model.

## Facts

- 2009-10-27 (8a1d0723) measurement: A shared DateInstanceCache produced about 0.5% SunSpider speedup, concentrated in date-format-tofte.js. (sourced)
- 2009-11-10 (bbfd4169) pitfall: DateInstanceCache must be reset with timezone cache state after system timezone changes, or cached GregorianDateTime values can use a stale timezone. (code)

## Moves

- 2002-10-30 (0c946c0c) replaced [[posix-time-functions]]: POSIX gmtime/localtime hit the disk by lstat()ing /etc/localtime on every call, causing unacceptable I/O during JavaScript execution; Core Foundation time APIs bypass this. (sourced)
- 2003-09-18 (a480b3c3) dropped: CF-based time functions — The CF-based overrides of gmtime/localtime/mktime/timegm/time were introduced because the OS-native libc implementations hit the filesystem on every call; once the OS fixed this performance problem, the CF workaround was no longer needed and was removed. (sourced)
- 2009-10-27 (8a1d0723) replaced [[date-instance-per-instance-gregorian-cache]]: Per-instance heap-allocated Cache was allocated lazily on first access and owned by DateInstance (delete in destructor); replaced by a 64-entry fixed-size hash table in JSGlobalData shared across all DateInstance objects because benchmark patterns access many distinct DateInstance objects with the same ms value, so a cross-instance cache hits where per-instance caches cold-miss. SunSpider reports ~0.5% speedup. (sourced)
- 2020-12-15 (5f5df0eb) replaced [[vm-date-cache-fields-plus-free-date-math-functions]]: Date cache state was moved behind a DateCache class so time-zone, date-instance, offset, and parsed-string caches can be reset and used as one owned mechanism rather than VM fields plus free functions. (code)
