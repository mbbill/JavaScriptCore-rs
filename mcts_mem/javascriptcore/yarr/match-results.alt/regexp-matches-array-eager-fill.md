- arrayOfMatches() directly allocates ArrayInstance and calls put() for each capture group, index, and input properties before returning

## Moves

- 2008-05-25 (632b6d94) replaced by [[match-results]]: Many callers of RegExp exec/match only test the array for nullness and never access its contents; eager population wastes string allocation and put() calls on every match; lazy fill avoids all of this work for the common case. (sourced)
