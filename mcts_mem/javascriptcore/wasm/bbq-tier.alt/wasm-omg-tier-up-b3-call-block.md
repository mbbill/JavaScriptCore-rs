- BBQ tier-up checks emit their rare tier-up call as an explicit B3 branch and CCall block inside each function.
- Each generated function owns the out-of-line tier-up call sequence.

## Moves

- 2017-06-06 (184a9951) replaced by [[bbq-tier]]: BBQ tier-up checks switched from an explicit B3 branch and CCall block to a patchpoint plus shared thunk so out-of-line call code is generated once instead of in each function. (code)
