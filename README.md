# JavaScriptCore Rust Skeleton

This directory contains the breadth-first Rust project skeleton for the
JavaScriptCore rewrite.

The crate is intentionally contract-first. Its modules define the major engine
responsibilities, ownership boundaries, mutation rules, and unsafe boundaries
before implementation work begins. It must not grow a small executable
JavaScript path until the surrounding module contracts are stable.

Start with the design notes in `docs/`, especially:

- `docs/ai-agent-workflow.md`
- `docs/000-jsc-responsibility-map.md`
- `docs/001-rust-design-skeleton.md`
