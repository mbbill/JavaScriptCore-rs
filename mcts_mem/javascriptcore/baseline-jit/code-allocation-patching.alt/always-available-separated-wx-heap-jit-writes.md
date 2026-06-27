- Separated W^X heap support is compiled as available without a feature gate.

## Moves

- 2016-03-16 (4905f9cd) replaced by [[code-allocation-patching]]: Separated W^X heap support was put back behind ENABLE_SEPARATED_WX_HEAP and disabled in feature defines because the ungated version caused crashes on ARM. (sourced)
