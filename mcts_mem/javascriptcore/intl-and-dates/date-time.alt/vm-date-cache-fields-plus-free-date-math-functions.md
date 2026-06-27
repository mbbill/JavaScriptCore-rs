- VM stored date-instance, timezone-offset, and parsed-date cache fields directly.
- Date math operated through free functions taking those VM cache structures.

## Moves

- 2020-12-15 (5f5df0eb) replaced by [[date-time]]: Date cache state was moved behind a DateCache class so time-zone, date-instance, offset, and parsed-string caches can be reset and used as one owned mechanism rather than VM fields plus free functions. (code)
