- Vector<ExpressionRangeInfo> m_expressionInfo field
- direct append via m_expressionInfo.append(info)
- direct access via Vector<ExpressionRangeInfo>& expressionInfo = m_expressionInfo

## Moves

- 2013-08-23 (da52af11) replaced by [[codeblock-split]]: UnlinkedCodeBlock expression-range data is rarely accessed at runtime; using CompressibleVector (zlib-backed) saves ~200k on Google Maps by compressing cold bytecode metadata that otherwise stays live in memory. (sourced)
