- Every JSScope stores global data, global object, and global this pointers inline.
- Scope construction passes those three global pointers through each scope object.
- Activation size includes duplicated per-global state.

## Moves

- 2012-08-31 (82c4590f) replaced by [[scope-chain-and-activation]]: Storing m_globalData, m_globalObject, and m_globalThis in every JSScope instance duplicated three pointers across all activation objects; moving them to JSGlobalObject (one copy per global) and deriving them via structure()->globalObject() and MarkedBlock lookup reduced JSActivation from 128-byte to 64-byte size class, halving allocation cost. (sourced)
