- YARR extracts fixed prefixes and fixed-count character classes into a Boyer-Moore prefilter before running the full matcher.
- Candidate maps are bitmap-backed and can carry a short exact-candidate list instead of one byte per possible character.
- Boyer-Moore anchor selection can sample the subject and prefer rare anchors over common ones.

## Facts

- 2010-11-21 (d77d2f20) rationale: The prefilter is deliberately skipped for short patterns and short subjects because setup and scanning overhead can dominate any skip benefit. (code)
- 2010-11-21 (d77d2f20) rationale: Boyer-Moore lookahead only applies when a fixed-position candidate sequence can be extracted from the pattern; variable-width constructs keep the full matcher as the primary executor. (code)
- 2021-07-30 (946ad004) measurement: Initial Boyer-Moore support was targeted at jQuery TodoMVC regular expressions where repeated full matching over long strings made anchor prefiltering useful. (sourced)
- 2023-02-09 (bb136cc4) pitfall: Recursive collection must preserve nested alternative reachability; treating a ParenthesesSubpattern as a hard stop loses useful anchors in patterns such as /aaa|(bbb|cccc)/. (code)

## Moves

- 2011-05-24 (176cfa67) dropped: begin-characters pre-scan optimization: The begin-characters optimization (pre-scanning for likely match start characters using paired char reads and bitmask comparison) had correctness issues (bug #61129) and was no longer measured as a performance win, so it was disabled pending investigation. (sourced)
- 2021-08-02 (77a1ed01) replaced [[yarr-boyer-moore-byte-vector-maps]]: Bitmap-backed Boyer-Moore candidate maps were chosen because they were neutral on jquery-todomvc-regexp while being 8x smaller than byte vectors. (sourced)
- 2021-08-02 (9c6fee78) replaced [[yarr-fixed-size-literal-boyer-moore-extraction]]: The extractor was changed to use fixed prefixes and fixed-count character classes so regexps such as jQuery TodoMVC patterns could still get Boyer-Moore lookahead when a later term was unsupported or the whole disjunction was not fixed-size. (code)
- 2021-08-07 (28852ca9) replaced [[yarr-boyer-moore-masked-bitmap]]: The masked 128-bit bitmap could say a mask was effective but could not preserve a small exact candidate set for unmasked character search, while the new representation carries up to two exact candidates and invalidates when the set grows larger. (code)
- 2023-02-09 (bb136cc4) replaced [[flat-term-yarr-boyer-moore-collection]]: The recursive collector supports nested disjunctions such as /aaa|(bbb|cccc)/ that the old flat collector explicitly refused at ParenthesesSubpattern terms. (code)
- 2023-02-10 (d43e2a23) replaced [[unweighted-regexp-boyer-moore-anchor-selection]]: Subject sampling was chosen because rare anchors make Boyer-Moore search effective while frequent anchors can make the extra search code slow. (code)
