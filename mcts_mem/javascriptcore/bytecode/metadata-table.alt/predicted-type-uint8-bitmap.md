- typedef uint8_t PredictedType
- PredictNone=0x00,PredictCell=0x01,PredictArray=0x03,PredictInt32=0x04,PredictDouble=0x08,PredictNumber=0x0c,PredictBoolean=0x10,PredictTop=0x1f,StrongPredictionTag=0x80
- inline predictionToString() returning fixed string constants

## Moves

- 2011-09-14 (59ad8d44) replaced by [[metadata-table]]: The uint8 PredictedType representation could only hold 5 type bits plus a strong-prediction tag; adding JSFinalObject, ObjectOther, ObjectUnknown, String, CellOther, and Other distinctions required 15 value bits plus the tag, necessitating expansion to uint16. (code)
