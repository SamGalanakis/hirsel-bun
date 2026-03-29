# Hirsel Historical Rewrite Spec

This file is intentionally archival.

The original rewrite spec described a different product shape built around runs, tasks, workers, delivery flows, and a much larger terminal/CLI surface. That is not the current application.

Current Hirsel is:

- backend-first
- a thin Tauri shell over a backend-served Datastar UI
- one project with one shepherd conversation, one canvas artifact, and many visible threads
- Docker + Nix scope execution for coding work

Use these files for the current system instead:

- `README.md`
- `docs/architecture.html`
- `docs/debugging.md`
- `deploy/README.md`
- `eval.md`

Keep this file only if you need historical context for the abandoned rewrite direction.
