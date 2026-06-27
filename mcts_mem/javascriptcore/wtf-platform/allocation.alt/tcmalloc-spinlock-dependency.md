- JSC allocation and GC infrastructure stores TCMalloc spinlock objects directly.
- Static spinlocks use TCMalloc initializer macros.
- Instance locks require explicit TCMalloc spinlock initialization in constructors.

## Moves

- 2015-03-13 (541755c0) replaced by [[allocation]]: WebKit no longer uses TCMalloc and can replace its spinlock dependency with a WTF::SpinLock built on WTF::Atomic. (sourced)
