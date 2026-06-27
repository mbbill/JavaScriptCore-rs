- Statement execution returns Completion structs by value.
- Each Completion stores a type, value, and target label pointer for abrupt control flow.
- Statement-list dispatch propagates break, continue, return, and throw by moving Completion objects.

## Moves

- 2007-12-20 (a54a94ac) replaced by [[exception-unwind]]: Returning Completion structs (type+value+target pointer) from every execute() call allocated stack space for three fields on every statement dispatch; storing completion type in ExecState and returning JSValue* directly eliminates the struct overhead, giving 2.4% SunSpider speedup (first attempted in 2663, rolled back, re-applied in 2665 with a bug fix). (sourced)
