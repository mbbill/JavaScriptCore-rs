- Objective-C external object graph marking does not remember old owners that acquire young managed references.
- Eden collection can miss managed values reachable only through a newly-added native edge.

## Moves

- 2014-04-15 (b217437c) replaced by [[opaque-embedding]]: Objective-C external-object graph marking gained an external remembered set because Eden collection must revisit old native owners that have acquired young managed references. (sourced)
