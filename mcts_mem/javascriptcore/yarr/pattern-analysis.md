- Pattern parsing and analysis are parameterized by input width and regexp options before bytecode or JIT generation.
- YARR owns regexp flag and option validation early enough to report syntax errors before runtime matching.
- Special pattern classes such as atoms, string-list alternations, entire-input matches, and trim-like forms are identified explicitly instead of inferred from a nullable atom string.
- Parser delegate requirements are checked by C++ constraints rather than a comment-only contract.

## Facts

- 2016-10-25 (e234d68f) rationale: Flag parsing belongs at the regexp/pattern boundary because both literals and constructors must reject invalid combinations consistently. (code)
- 2016-03-08 (1d5ebf95) rationale: Pattern construction records Unicode mode in YARR options before syntax checking so escape validation can distinguish legacy and Unicode grammar. (code)
- 2017-12-04 rationale: Sticky anchoring is represented as a YARR pattern option rather than a caller-side start-index convention. (code)
- 2018-07-03 (6ca10d5a) rationale: Special atom detection lets simple substring matches bypass the general regexp executor while preserving the same RegExp object surface. (code)
- 2023-03-03 (270c8244) pitfall: RegExp syntax checking through a delegate must reject constructs as the parser sees them; postponing validity checks can accept patterns that later code generation cannot represent. (code)
- 2023-03-14 rationale: UnicodeSets mode is represented as a compile mode alongside Legacy and Unicode so parser, class construction, and codegen share one mode decision. (code)
- 2026-03-27 (d1ae3398) rationale: Function-body reparsing can skip Yarr::checkSyntax for RegExp literals because the enclosing script parse already validated them, and this removes roughly 45% of checkSyntax calls in web-tooling-benchmark. (sourced)

## Moves

- 2007-11-07 (7f105b01) replaced [[regexp-flags-as-int]]: RegExp constructor changed from accepting an integer flags bitmask to accepting a UString of flag characters, eliminating duplicated flag-parsing code scattered across callers. (code)
- 2010-01-09 (3d2dfd7e) removed: The m_flags UString field was never read after construction — flag information is reconstructed on demand from the numeric m_flagBits bitfield — so storing the original flags string was pure redundant allocation per RegExp instance. (code)
- 2011-11-14 (baec49ed) replaced [[yarr-parser-uchar-only-pattern-buffer]]: Yarr::parse now dispatches on pattern.is8Bit() and instantiates Parser with LChar or UChar so the parser can read the stored string width instead of always taking pattern.characters16(). (code)
- 2019-03-11 (71aac694) replaced [[regexp-flag-bitmask]]: Moving flag parsing from runtime/ to yarr/ as OptionSet<Flags> enables early (parse-time) SyntaxError detection for invalid RegExp flags, which the old RegExpFlags int bitmask in runtime/ could not surface until bytecode emit time. (code)
- 2024-10-15 (5d4d9792) replaced [[substring-global-atom-regexp-engine-counting]]: For substring global atom regular expressions with one-character patterns, direct span scanning replaces repeated RegExpGlobalData::performMatch calls while preserving the substring match cache. (code)
- 2025-02-20 (be68a2f3) replaced [[yarr-parser-comment-delegate-contract]]: The parser delegate contract was moved from an advisory comment to a C++ concept so missing callback methods become compile-time type errors. (code)
- 2025-02-28 (04edf771) replaced [[yarr-atom-regexp-fast-path]]: A nullable atom string could only represent literal fixed-character regexps, so Yarr changed the special-case representation to an enum that can also carry anchored whitespace trim patterns. (code)
- 2026-03-29 (3591ed5f) replaced [[html-pattern-double-regexp-compilation]]: HTML pattern matching needed raw-pattern validation plus anchored matching without compiling both the raw and anchored regular expressions, because anchoring can make an invalid raw pattern valid. (code)
