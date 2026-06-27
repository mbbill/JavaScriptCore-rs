- String slicing returned a sharing StringImpl that retained the full owner string.
- Substring lifetime could keep a much larger parent buffer alive.

## Moves

- 2019-05-10 (509328f0) replaced by [[rope-string]]: StringImpl::createSubstringSharingImpl kept the owner StringImpl alive for the full lifetime of any substring, inflating memory; JSRopeString avoids this by creating a fresh StringImpl only on resolution, and after JSRopeString was shrunk to 32 bytes it became cheap enough to prefer unconditionally. (sourced)
