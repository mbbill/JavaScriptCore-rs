- Public execution is exposed through a KJScript global facade.
- The current interpreter context is reached through a static current() accessor.
- Global object access is tied to singleton-style facade state.

## Moves

- 2002-03-22 (9491afaa) replaced by [[entry-api]]: The KJScript class (global facade with static current() context) was replaced by the Interpreter class that takes an explicit global Object in its constructor, enabling multiple independent interpreter instances without relying on a global singleton. (code)
