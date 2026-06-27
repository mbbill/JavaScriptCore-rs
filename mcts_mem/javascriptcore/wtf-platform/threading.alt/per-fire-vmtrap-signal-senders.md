- Each VM trap fire creates a separate signal sender object and thread path.
- VMTraps tracks a set of active signal senders.
- Sender lifetime and VM pointer clearing are synchronized per sender.

## Moves

- 2017-06-29 (b8d026a2) replaced by [[threading]]: A single AutomaticThread signal sender avoids the data races caused by allowing many VMTrap signal sender threads to exist at once and deallocates itself when traps are idle. (sourced)
