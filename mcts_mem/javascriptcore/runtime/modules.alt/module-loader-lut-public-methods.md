- Module-loader helper functions are installed from a generated lookup table as ordinary function properties.
- Loader helper calls go through method properties such as `this.resolve` rather than private link-time constants.
- Error stacks can observe internal module-loader helper names.

## Moves

- 2022-08-21 (254430a0) replaced by [[modules]]: Internal module-loader functions were marked private so they would not be exposed in Error stacks. (sourced)
