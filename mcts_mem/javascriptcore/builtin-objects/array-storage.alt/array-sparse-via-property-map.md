- Sparse array indices were represented as generic properties keyed by stringified identifiers.
- Indexed lookup and update paid string conversion and generic property-map costs.

## Moves

- 2007-10-21 (0cf6079d) replaced by [[array-storage]]: Sparse array indices beyond sparseArrayCutoff were stored in the generic string-keyed PropertyMap requiring Identifier string conversion on every get/put; replaced with a dedicated HashMap<unsigned,JSValue*> keyed by integer index, yielding a 10% SunSpider speedup. (sourced)
