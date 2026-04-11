# Eval Notes

This file is no longer a product-spec test plan.

The old rewrite-eval checklist targeted a different Hirsel shape. The current repo is:

- a backend-served Datastar UI
- a thin Tauri wrapper
- one project with one shepherd, one canvas, and many visible threads
- Docker + Nix scope execution for coding turns

Use these checks for the current repo:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
just --unstable --fmt --check
bun run vite:build
bun run test
```

For architecture context, see `docs/architecture.html`.
