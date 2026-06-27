- PCRE compile and execute paths receive ASCII-truncated pattern and subject buffers.
- PCRE_UTF8 is not enabled.
- Match offsets remain PCRE byte offsets with no bidirectional UTF-8/UTF-16 translation.

## Moves

- 2003-06-05 (f03fde27) replaced by [[yarr]]: The old code passed UString through .ascii() which truncated all code points above U+00FF to their low byte, corrupting non-ASCII characters in both the pattern and the subject; switching to PCRE_UTF8 mode with explicit UString-to-UTF-8 conversion and bidirectional character/byte offset translation allows correct matching of multibyte characters. (sourced)
