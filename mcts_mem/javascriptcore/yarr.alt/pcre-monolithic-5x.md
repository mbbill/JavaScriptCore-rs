- pcre/pcre.c (monolithic source)
- pcre/get.c
- pcre/internal.h
- pcre/maketables.c (old)
- pcre/dftables.c (old)

## Moves

- 2005-09-09 (9b1f5040) replaced by [[yarr]]: PCRE vendored library updated from ~5.x single-file layout to PCRE 6.1 modular multi-file layout including Unicode character property support (ucptable.c, ucp_findchar.c); the upgrade preserves Apple's UTF-16 patch applied on pcre-6-1-branch, gaining new PCRE 6.1 features and bug fixes that the old monolithic pcre.c could not provide. (sourced)
