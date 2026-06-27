- One DFGDriver compile call parsed, optimized, linked, installed watchpoints, and registered identifiers synchronously.
- Compilation and finalization both ran on the calling thread with no Finalizer abstraction.

## Moves

- 2013-07-25 (76a8f465) replaced by [[tier-up]]: The monolithic DFGDriver compile() function combined all compilation phases and finalization (linking, watchpoint installation, identifier registration) in one synchronous call that must run on the main thread, making it impossible to run the compilation phase concurrently on a background thread while requiring finalization on the main thread. (sourced)
