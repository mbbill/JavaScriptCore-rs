- RegExpMatchesArray stores a copied RegExpResult.
- The RegExpResult copies the input, subpattern count, and entire output vector.
- Lazy property fill still pays the vector copy when the matches array is created.

## Moves

- 2012-03-21 (432a7802) replaced by [[match-results]]: RegExpMatchesArray stopped copying the ovector because sub-pattern results are often only used for grouping and never accessed, making allocation, construction, and destruction of every matches array more expensive. (sourced)
