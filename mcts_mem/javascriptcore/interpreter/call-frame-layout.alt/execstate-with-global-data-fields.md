- ExecState stores global object and global data fields beside its call-frame pointer.
- Global state is retrieved directly from ExecState instead of through the frame's scope chain.

## Moves

- 2008-10-03 (ef3cde6f) replaced by [[call-frame-layout]]: ExecState stored m_globalObject and m_globalData redundantly when the same data is reachable through the call frame's scope chain; removing them makes ExecState a thin call-frame-pointer wrapper, enabling further optimization passes and reducing construction cost. (sourced)
