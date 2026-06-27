- A compile-time ENABLE_YARR switch chooses between a PCRE path and a YARR path.
- YARR without JIT uses the YARR interpreter, while YARR JIT can still fall back to PCRE.
- PCRE match output uses the PCRE offset-vector convention with an extra internal slot per subpattern.

## Moves

- 2010-11-21 (d77d2f20) replaced by [[yarr]]: ENABLE_YARR macro and PCRE fallback path were removed; YARR JIT now falls back to the YARR interpreter (not PCRE) when JIT compilation is unsupported, eliminating the PCRE dependency from JSC. (sourced)
