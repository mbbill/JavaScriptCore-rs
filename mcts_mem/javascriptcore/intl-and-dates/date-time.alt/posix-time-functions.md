- Date conversion called POSIX gmtime and localtime directly.
- Local timezone lookup could trigger filesystem work during JavaScript execution.

## Moves

- 2002-10-30 (0c946c0c) replaced by [[date-time]]: POSIX gmtime/localtime hit the disk by lstat()ing /etc/localtime on every call, causing unacceptable I/O during JavaScript execution; Core Foundation time APIs bypass this. (sourced)
