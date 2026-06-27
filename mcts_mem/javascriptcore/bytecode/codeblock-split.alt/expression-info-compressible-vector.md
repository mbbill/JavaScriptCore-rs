- CompressibleVector<ExpressionRangeInfo> m_expressionInfo field
- m_expressionInfo.data() accessor used in expressionRangeForBytecodeOffset and addExpressionInfo
- RareData::m_expressionInfoFatPositions for overflow positions

## Moves

- 2013-09-23 (3df588ee) replaced by [[codeblock-split]]: CompressibleVector<ExpressionRangeInfo> was reverted to plain Vector<ExpressionRangeInfo> because it caused a CodeLoad performance regression that the team could not immediately resolve. (sourced)
