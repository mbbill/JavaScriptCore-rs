- JSC carries a JavaScript-specific PCRE source fork.
- PCRE source files compile and execute regexp patterns even after YARR becomes the main engine.
- PCRE remains present as dead or fallback code after the YARR transition.

## Moves

- 2011-02-10 (5f1c8764) removed: PCRE source removed from tree because Yarr had already been adopted as the JSC regex engine, leaving PCRE dead code that was still being built; sourced from bug 54188 and 'Remove PCRE source from trunk' message. (sourced)
