- next4 lookahead in Lexer
- \u decode in scanRegExp using next1/next2/next3/next4
- record16(convertUnicode(...)) inside scanRegExp escape branch

## Moves

- 2006-11-20 (090b9a95) replaced by [[character-classes]]: Handling \u escapes in the lexer (r17354) broke metacharacter escaping and prevented serialized regexps from preserving unicode escapes; moving translation to the RegExp constructor via sanitizePattern() better matches other browsers' behavior by keeping the raw pattern string intact through the parser. (sourced)
