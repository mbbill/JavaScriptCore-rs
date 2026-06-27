- Bound function objects store each bound argument using the parameter's declared type.
- RefPtr and PassRefPtr bound values are copied and passed through the same type at call time.
- Ownership-sensitive parameters do not have a separate storage/view split.

## Moves

- 2011-12-27 (c8ca0d41) replaced by [[ref-counted-ownership]]: Bound parameters are stored through ParamStorageTraits so RefPtr and PassRefPtr can keep owning RefPtr storage while calls peek as raw pointers to avoid reference-count churn. (code)
