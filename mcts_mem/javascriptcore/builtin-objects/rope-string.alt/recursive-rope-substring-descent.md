- Substring extraction recursively descended through nested rope fibers when the substring fit one fiber.
- Degenerate ropes could repeatedly traverse without a depth bound before flattening fallback.

## Moves

- 2026-02-06 (93f2fd68) replaced by [[rope-string]]: Rope substring extraction now uses a bounded loop and returns nullptr after a depth limit so jsSubstring falls back to resolveRope, flattening degenerate ropes instead of repeatedly traversing them. (code)
