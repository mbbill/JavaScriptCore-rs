- Module namespace property access disables normal property caching.
- Each namespace get resolves the export through the module record.
- The value path uses a target module environment property slot after export resolution.

## Moves

- 2017-02-21 (24801203) replaced by [[modules]]: Module namespace property access moved from per-access resolveExport with caching disabled to cached export resolutions plus a namespace-cell-guarded IC that loads directly from the module environment. (code)
