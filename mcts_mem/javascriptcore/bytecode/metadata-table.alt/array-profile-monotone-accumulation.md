- m_observedArrayModes always accumulates via |=
- no pruning once polymorphic

## Moves

- 2013-07-25 (6dc567a6) replaced by [[metadata-table]]: When an ArrayProfile goes polymorphic (two or more array mode bits set) for the first time, forcibly monomorphizing it to the latest-seen structure (controlled by m_didPerformFirstRunPruning) eliminates unnecessary Arrayify nodes and makes loops effect-free; measured 5% speedup on Kraken/imaging-gaussian-blur with FTL enabled. (sourced)
