- GTK JSRunLoopTimer owns a GLib GSource directly.
- Timer priority, naming, scheduling, and destruction are encoded in the GTK-specific timer implementation.
- The generic WTF RunLoop timer is not the owner for GTK JavaScript run-loop timers.

## Moves

- 2017-04-11 (3a455e09) replaced by [[threading]]: GTK JSRunLoopTimer moved to WTF::RunLoop::Timer while only Cocoa kept the platform-specific timer because Cocoa needs to retarget timers to the WebThread run loop. (sourced)
