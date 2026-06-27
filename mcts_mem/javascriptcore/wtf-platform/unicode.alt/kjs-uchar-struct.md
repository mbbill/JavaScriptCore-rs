- KJS defines its own UChar struct around an unsigned-short code unit.
- Character users access the code unit through a `.uc` member and wrapper accessors.
- The runtime carries both KJS character wrappers and WTF Unicode character types.

## Moves

- 2008-03-10 (834542d4) replaced by [[unicode]]: KJS::UChar was a custom struct wrapping unsigned short with a .uc member field; replaced with WTF's ::UChar (a typedef for unsigned short from wtf/unicode/Unicode.h) to eliminate redundant type definition and remove all .uc member-access sites across the codebase. (code)
