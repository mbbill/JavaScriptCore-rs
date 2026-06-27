- Runtime support owns realm/global state, host-visible job scheduling, module linkage, and VM coordination decisions that are not specific to a compiler tier, object layout, or public API.
- A `JSGlobalObject` roots each realm's cached structures, constructors, global properties, watchpoints, and lazy feature state.
- Runtime execution assumes explicit VM/lock ownership at API and callback boundaries, with selected per-thread state used only where a VM or thread identity must be recovered cheaply.
- Promise jobs, module jobs, watchdog traps, and stop-the-world coordination are VM-level services rather than ad-hoc queues owned by individual builtins or opcodes.

- [[global-object]]
- [[locking-and-threads]]
- [[modules]]
- [[promises-and-microtasks]]
- [[watchdog-and-vm-coordination]]
