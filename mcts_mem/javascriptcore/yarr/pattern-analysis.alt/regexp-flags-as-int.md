- RegExp(UString pattern, int flags = None) constructor
- flags() returning m_flags int directly
- public enum { None=0, Global=1, IgnoreCase=2, Multiline=4 }

## Moves

- 2007-11-07 (7f105b01) replaced by [[pattern-analysis]]: RegExp constructor changed from accepting an integer flags bitmask to accepting a UString of flag characters, eliminating duplicated flag-parsing code scattered across callers. (code)
