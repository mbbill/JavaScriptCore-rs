# JavaScriptCore Rust Rewrite

This directory contains the breadth-first Rust project for the JavaScriptCore
rewrite.

The crate remains architecture-first. Some executable VM behavior now exists,
but implementation work must still be scheduled by dependency and subsystem
priority rather than by chasing a tiny local path.

Start with the coordination notes in `docs/`, especially:

- `docs/ai-agent-workflow.md`
- `docs/000-jsc-responsibility-map.md`
- `docs/001-rust-design-skeleton.md`
- `docs/002-bfs-rewrite-plan.md`
- `docs/progress.md`
