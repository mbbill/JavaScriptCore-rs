- Eval preprocessing used the iterative literal parser for SloppyJSON while strict JSON could use the recursive entry path.
- SloppyJSON property-key and parenthesized-literal allowances were outside the recursive parser's mode system.

## Moves

- 2025-03-03 (65ae8f7a) replaced by [[parser]]: SloppyJSON eval preprocessing moved onto the recursive JSON parser by parameterizing recursive descent with ParserMode so it can accept SloppyJSON property keys and parenthesized literals while preserving strict JSON behavior. (code)
