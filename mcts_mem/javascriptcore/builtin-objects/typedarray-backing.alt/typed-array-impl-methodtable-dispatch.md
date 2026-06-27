- Access to a typed-array implementation pointer used a MethodTable slot.
- Every JSCell class carried a dispatch hook used only by typed arrays and DataView.

## Moves

- 2018-07-11 (91823615) replaced by [[typedarray-backing]]: Central JSArrayBufferView dispatch by view type was sufficient without spending a MethodTable slot because getTypedArrayImpl was only overridden by typed arrays and DataView. (sourced)
