- Regexp compilation and execution expose PCRE-style C entry points with pcre-prefixed names.
- The fork retains unused PCRE feature paths such as UTF-8, POSIX, UCP, study, and NO_RECURSE support.
- Allocation routes through PCRE-compatible malloc/free indirection.

## Moves

- 2007-11-04 (0fb893f3) replaced by [[yarr]]: The standard PCRE public API (pcre_compile2/pcre_exec/pcre prefix naming) included unused features (UTF-8, POSIX, UCP, study, etc.) that cost code size and execution overhead; renaming to jsRegExpCompile/jsRegExpExecute and stripping all JS-unused paths gave 0.8% overall / 6.5% regexp SunSpider speedup while intentionally forking from upstream PCRE. (sourced)
