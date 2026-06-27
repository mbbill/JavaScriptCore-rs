- Regexp matching returns compact offsets or spans first and lazily materializes match arrays or subpattern strings only when observed.
- Output-vector storage lives inline, cached, or RegExp-owned on normal VM-thread paths rather than being heap-allocated for every match.
- The engine maintains direct and counting paths for atom or global matches when full capture materialization is unnecessary.

## Facts

- 2005-10-11 (a02dfe3c) rationale: Centralizing all RegExp matching through performMatch fixes 11 JavaScriptCore tests for RegExp.$_, RegExp.lastMatch, RegExp.lastParen, RegExp.leftContext, RegExp.rightContext properties that were not populated under the old registerRegexp/setSubPatterns protocol. (sourced)
- 2007-10-31 (d6aa380c) pitfall: Raw caller-owned output vectors made deletion and lifetime part of every match call path. (code)
- 2008-09-09 rationale: Lazy RegExpMatchesArray materialization keeps capture substrings out of the hot path when code only tests whether a match occurred. (code)
- 2010-01-07 measurement: Inline output-vector storage removed a per-match heap allocation and produced a reported speedup on SunSpider string-unpack-code. (sourced)
- 2014-05-23 (836527ff) rationale: Cached and direct match paths target repeated simple regexp use where the cost of materializing captures or arrays dominates useful work. (sourced)
- 2018-07-03 (6ca10d5a) rationale: Global atom counting can compute result counts without constructing the full match array when the caller only needs a count. (code)

## Moves

- 2007-10-31 (d6aa380c) replaced [[regexp-match-returns-ustring]]: Returning a UString from RegExp::match and performMatch forced needless string allocation for every match; returning position+length as ints avoids the allocation and eliminates an intermediate UString copy on each call path. (code)
- 2008-05-25 (632b6d94) replaced [[regexp-matches-array-eager-fill]]: Many callers of RegExp exec/match only test the array for nullness and never access its contents; eager population wastes string allocation and put() calls on every match; lazy fill avoids all of this work for the common case. (sourced)
- 2009-07-04 (390e99e8) replaced [[regexp-match-ovector-heap-allocated]]: Replacing OwnArrayPtr<int> (heap allocation per match) with Vector<int,32> (32-int inline buffer, reused across calls) eliminated the per-match heap allocation, yielding ~5% speedup on SunSpider string-unpack-code and 0.3% overall. (sourced)
- 2010-06-16 (cabdfe60) replaced [[yarr-matchbegin-stack-slot]]: matchBegin was stored in a dedicated call-stack slot (push/pop at disjunction boundary, +1 frameSize) but the output array passed into the JIT stub already provides a suitable temporary slot at index 0; storing directly there eliminates the extra stack allocation. (code)
- 2010-08-25 (a4062f12) dropped: single-entry regexp match cache: The single-entry cache (m_lastMatchString, m_lastMatchStart, m_lastOVector) was added to speed up Dromaeo, but Dromaeo was later modified to use somewhat random regular expressions that do not repeat identical matches, making the cache unhelpful while adding overhead on every non-matching call. (sourced)
- 2011-05-25 (28ba6e56) replaced [[regexp-eager-compile]]: RegExp construction now only validates the pattern and extracts numSubpatterns; JIT/bytecode codegen is deferred to the first match() call, reducing construction cost for regexps that are created but never executed. (sourced)
- 2012-01-11 (30377e04) replaced [[regexp-match-array-subclassdata-private-copy]]: RegExp match arrays only need the input string, subexpression count, and active output vector, so storing that snapshot inline avoids allocating and copying a whole RegExpConstructorPrivate object. (code)
- 2012-03-21 (432a7802) replaced [[regexp-matches-array-copied-ovector]]: RegExpMatchesArray stopped copying the ovector because sub-pattern results are often only used for grouping and never accessed, making allocation, construction, and destruction of every matches array more expensive. (sourced)
- 2026-02-03 (e9964c3c) replaced [[regexp-global-data-offset-vector]]: RegExp can determine the needed offset-vector size, so normal VM-thread matching reuses a vector owned by the RegExp instead of allocating or moving per-match vectors, while concurrent matching still receives caller-provided storage. (code)
