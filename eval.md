# Eval Notes

This file is no longer a product-spec test plan.

The old rewrite-eval checklist was for a different Hirsel shape: Alpine.js, broad CLI workflows, scenario fixtures, and a route/board-first UI. The current codebase is a SolidJS + Tauri client around a backend-first runtime with project focus views, route work trees, and route-scoped worker concerns.

Use these checks for the current repo instead:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
bunx tsc --noEmit
bun run vite:build
```

For architecture context, see `docs/architecture.html`.
