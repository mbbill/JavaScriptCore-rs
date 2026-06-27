- Each `JSManagedValue` stores its owner weakly at construction time.
- Managed-value liveness is tracked by the managed value itself instead of the virtual machine's external object graph.

## Moves

- 2013-03-22 (8cd345a1) replaced by [[objective-c-embedding]]: The owner-embedded JSManagedValue API duplicated ownership tracking that JSVirtualMachine already provides; consolidating into JSVirtualMachine addManagedReference:withOwner: is the single authoritative mechanism for keeping managed references alive. (sourced)
