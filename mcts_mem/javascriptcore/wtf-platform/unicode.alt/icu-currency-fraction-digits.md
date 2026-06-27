- Currency minor-unit digits are read from ICU default fraction digits.
- ECMA-402 currency formatting inherits CLDR-backed ICU behavior for this table.
- JavaScriptCore carries no inline ISO 4217 minor-unit data source.

## Moves

- 2017-03-15 (0dc886f9) replaced by [[unicode]]: ECMA-402 specifies ISO 4217 as the CurrencyDigits data source, so ICU's CLDR-backed default fraction digits were replaced by an inline ISO 4217 minor-unit table. (sourced)
