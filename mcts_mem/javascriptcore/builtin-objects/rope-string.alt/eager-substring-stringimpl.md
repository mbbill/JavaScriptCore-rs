- Substring creation immediately produced a substring StringImpl or cached flat string.
- Rope strings had no substring-base and offset representation.

## Moves

- 2014-07-22 (5aae0c1f) replaced by [[rope-string]]: jsSubstring became lazy by representing a substring as a special JSRopeString case instead of immediately creating a substring StringImpl. (code)
