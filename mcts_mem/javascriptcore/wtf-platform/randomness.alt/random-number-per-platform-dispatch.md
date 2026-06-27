- The JavaScript random-number helper contains its own platform branches for MSVC, Darwin, Unix, Windows, and fallback generators.
- OS entropy use is duplicated separately from the cryptographic-randomness helper.
- Non-OS randomness and OS randomness live in one broad dispatch routine.

## Moves

- 2011-02-28 (3ffbcd40) replaced by [[randomness]]: randomNumber() previously contained per-platform branches (MSVC rand_s, Darwin arc4random, Unix random(), Windows rand, Mersenne Twister) duplicating logic already in cryptographicallyRandomNumber(); refactoring delegates to that abstraction when USE(OS_RANDOMNESS) is available, leaving only non-OS_RANDOMNESS ports (Mersenne Twister, BREWMP) in the fallback path. (code)
