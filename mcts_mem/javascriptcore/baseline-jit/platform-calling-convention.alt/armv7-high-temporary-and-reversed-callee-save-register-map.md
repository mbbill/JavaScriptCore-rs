- ARMv7 maps common temporaries to higher registers and keeps a target-specific reversed callee-save order.

## Moves

- 2022-03-29 (26b13a47) replaced by [[platform-calling-convention]]: ARMv7 maps lower-order temporaries to r4/r5 and orders regCS0/regCS1 as r10/r11 so Thumb-2 can use shorter encodings for common temporaries and LLInt callee-save code no longer needs target-specific reversed-order handling. (code)
