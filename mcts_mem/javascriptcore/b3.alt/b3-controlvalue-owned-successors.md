- ControlValue was the terminal Value subclass for Jump, Branch, Return, Oops, and Switch.
- ControlValue stored successor blocks and exposed successor accessors.
- SwitchValue was a ControlValue subclass.

## Moves

- 2016-07-19 (53a1e5c7) replaced by [[b3]]: Successor edges moved from ControlValue into BasicBlock so any terminal Value, especially a Patchpoint with effects.terminal, can own arbitrary generated control flow without imposing a special ControlValue subclass. (sourced)
