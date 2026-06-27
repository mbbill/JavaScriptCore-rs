- TypedArrayMode had explicit resizable and non-resizable enum cases.
- Auto-length and growable-shared views could not be expressed while retaining direct raw-field fast paths for fixed views.

## Moves

- 2022-11-16 (fe4f0a4c) replaced by [[typedarray-backing]]: Growable SharedArrayBuffer views and auto-length views needed mode states beyond the old resizable/non-resizable enum, while non-resizable and fixed-growable views still needed direct raw-field fast paths. (code)
