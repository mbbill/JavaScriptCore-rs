- Quiet NaNs were treated as one category for JSValue encoding purposes.
- The representation did not distinguish NaNs already safe for tagging from NaNs that needed purification.
- Tagging could misclassify a double NaN whose payload overlapped the non-double tag space.

## Moves

- 2014-04-16 (b0026adb) replaced by [[tagged-encoding]]: A single quiet-NaN category could not express whether a NaN was safe to encode as a JSValue or needed purification before tagging. (code)
