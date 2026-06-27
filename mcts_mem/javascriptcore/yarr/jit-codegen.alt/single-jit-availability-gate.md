- RegExp JIT compilation called JSGlobalData::canUseJIT(); disabling or not building the JS language JIT also disabled YARR JIT.
- JSGlobalData stored one m_canUseJIT flag computed from executableAllocator validity, Options::useJIT, and the JavaScriptCoreUseJIT CF preference/environment setting.

## Moves

- 2012-05-01 (5c6e9dc3) replaced by [[jit-codegen]]: Need to split canUseRegExpJIT out of canUseJIT. (sourced)
