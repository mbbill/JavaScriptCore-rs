- Exported LLInt entry points were hand-emitted with raw .globl assembler directives.
- Those raw labels bypassed offlineasm label attributes such as alternate entry handling.

## Moves

- 2024-02-16 (8f9efa2d) replaced by [[builtins-codegen]]: Raw emitted .globl labels bypassed offlineasm alt_entry support, while ordinary offlineasm global labels hid symbols, so exported LLInt entry points needed a DSL case that keeps alt_entry generation and export visibility independent. (code)
