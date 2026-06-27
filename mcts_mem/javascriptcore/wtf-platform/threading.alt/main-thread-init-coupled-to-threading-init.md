- Initializing threading also initializes the WTF main-thread identity.
- All ports treat the thread that initializes threading as the main thread unless platform code special-cases it.
- There is no separate API to bind main-thread identity to the process main thread.

## Moves

- 2010-04-26 (e12b0e8c) replaced by [[threading]]: initializeMainThread was previously called inside initializeThreading on all platforms, conflating two distinct concepts; decoupling them and adding initializeMainThreadToProcessMainThread (Mac-only) allows WebKit2 and WebKit1 to both use the same WebCore with different main-thread identity semantics (either the calling thread or the process main thread). (sourced)
