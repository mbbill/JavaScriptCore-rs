- JSString and JSRopeString use compact shared layouts where rope state is encoded in pointer bits rather than vtable shape.
- Rope concatenation and substring operations preserve fibers until a caller requires contiguous characters.
- Rope resolution is lazy and mutates the string cell into resolved storage after flattening.
- String storage is allocated under string/primitive caging rules before entering JSString or rope resolution.

## Facts

- 2011-11-14 (16010626) rationale: Rope resolution preserves 8-bit ropes by carrying an is8Bit flag across fibers and resolving all-Latin-1 ropes into LChar buffers. (code)
- 2017-08-31 (2c525d35) rationale: Moving string backing storage into a dedicated String Gigacage lets JSString creation and rope resolution enforce caged storage at the boundary. (code)

## Moves

- 2011-10-19 (c172563e) replaced [[malloc-ropeimpl-strings]]: The new GC-backed rope representation was chosen because it gave a ~1% SunSpider speedup and removed one cause for strings having C++ destructors. (sourced)
- 2014-07-22 (5aae0c1f) replaced [[eager-substring-stringimpl]]: jsSubstring became lazy by representing a substring as a special JSRopeString case instead of immediately creating a substring StringImpl. (code)
- 2016-02-18 (fcee787a) replaced [[jsrope-substring-copy-resolution]]: Resolving substring ropes by sharing the parent StringImpl was chosen over copying bytes, trading possible parent-string lifetime extension for less GC and lower peak memory on large diff-viewer pages. (sourced)
- 2019-03-01 (1bbd6bf9) replaced [[jsstring-explicit-length-flags-layout]]: sizeof(JSString) reduced 24->16 and sizeof(JSRopeString) 48->32 by eliminating redundant length/flags fields from JSString (queried from StringImpl instead) and compressing JSRopeString's three fiber pointers + length + is8Bit flag into 48-bit-address-exploiting split storage, fitting both into GC heap cell atoms to cut per-instance allocation by 16 bytes. (sourced)
- 2019-05-10 (509328f0) replaced [[substring-sharing-impl-return]]: StringImpl::createSubstringSharingImpl kept the owner StringImpl alive for the full lifetime of any substring, inflating memory; JSRopeString avoids this by creating a fresh StringImpl only on resolution, and after JSRopeString was shrunk to 32 bytes it became cheap enough to prefer unconditionally. (sourced)
- 2024-12-16 (f941f5eb) replaced [[rope-resolution-stack-limit-guarded-recursion]]: Rope resolution adopted signature-matched MUST_TAIL_CALL recursion instead of carrying a soft stack limit because the fast path's recursive calls are intended to be tail calls. (sourced)
- 2026-02-06 (93f2fd68) replaced [[recursive-rope-substring-descent]]: Rope substring extraction now uses a bounded loop and returns nullptr after a depth limit so jsSubstring falls back to resolveRope, flattening degenerate ropes instead of repeatedly traversing them. (code)
