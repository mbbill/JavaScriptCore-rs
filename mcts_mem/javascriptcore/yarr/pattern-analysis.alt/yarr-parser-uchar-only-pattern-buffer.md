- YARR parser reads every pattern through a UChar buffer.
- Parser is parameterized by delegate only, not by character width.
- Even 8-bit patterns are accessed via characters16().

## Moves

- 2011-11-14 (baec49ed) replaced by [[pattern-analysis]]: Yarr::parse now dispatches on pattern.is8Bit() and instantiates Parser with LChar or UChar so the parser can read the stored string width instead of always taking pattern.characters16(). (code)
