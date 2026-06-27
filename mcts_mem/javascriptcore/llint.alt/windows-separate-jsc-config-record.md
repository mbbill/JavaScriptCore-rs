- Windows LLInt and C++ code address JSC configuration through a separate JSC/WTF config path.
- Configuration offsets are not unified with the freezable g_config record shared by other targets.

## Moves

- 2024-08-05 (f0f17a7e) replaced by [[llint]]: JSC stopped maintaining a Windows-only standalone JSC/WTF config path and made LLInt and C++ code address JSC configuration through the unified g_config offsets. (code)
