- WatchpointsOnStructureStubInfo owned the Bag of StructureTransitionStructureStubClearingWatchpoint and AdaptiveValueStructureStubClearingWatchpoint variants.
- Each clearing watchpoint stored a WatchpointsOnStructureStubInfo* holder and invalidated m_holder->stub() when its condition failed or fired.
- PolymorphicAccessJITStubRoutine stored the holder as a unique_ptr and installed it through setWatchpoints().

## Moves

- 2024-05-18 (1ece8d08) replaced by [[inline-cache]]: Structure/adaptive clearing watchpoints now fire a WatchpointSet instead of invalidating a PolymorphicAccessJITStubRoutine directly, so the watchpoint target is no longer a stub-routine pointer. (code)
