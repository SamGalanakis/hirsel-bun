# Eval Notes

This file is no longer a product-spec test plan.

The old rewrite-eval checklist targeted a different Hirsel shape. The current repo is:

- a backend-served Datastar UI
- a thin Tauri wrapper
- one project with one shepherd, one canvas, and many visible threads
- Docker + Nix scope execution for coding turns

Use these checks for the current repo:

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
bash -n dev.sh
bun run vite:build
bun run test
```

For architecture context, see `docs/architecture.html`.
