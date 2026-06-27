- A single VM-owned parser arena was borrowed by parse operations.
- Parsed roots had to detach arena contents from the VM-owned arena after parsing.

## Moves

- 2014-12-03 (579e5edd) replaced by [[arena]]: There's no need to keep a global arena. We can create a new arena each time we parse. (sourced)
