- VoidPtrPair is a plain two-pointer POD struct returned by value.

## Moves

- 2008-10-30 (2c36e779) replaced by [[platform-calling-convention]]: Linux ABI does not pass POD structs of two pointers in registers; wrapping the struct in a union with a uint64_t member forces the pair into a single register-sized value, matching Darwin and MSVC behavior needed for correct CTI calling convention. (sourced)
