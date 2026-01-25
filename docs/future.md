# Future Ideas

## GUI: Custom font
Consider replacing system fonts with a dedicated typeface (Inter, IBM Plex Sans, Geist). ET Book (https://edwardtufte.github.io/et-book/) was considered but is a poor fit — serif fonts don't suit dense developer tool UIs, limited weight coverage, and fine serifs render poorly at small sizes.

## Sprites.dev Support (Removed)

Sprite runner support was removed because:
- Requires publicly-accessible coordinator URL (workers can't connect to localhost)
- Fly.io provides similar ephemeral VM functionality with better integration
- Adds maintenance burden for a rarely-used runner type

If sprites.dev support is reconsidered in the future, it would require:
1. Remote orchestrator mode (coordinator on Fly.io or similar)
2. Or Tailscale integration for private connectivity
3. Restore files from git history:
   - `src-tauri/src/core/runner/sprite.rs`
   - `src-tauri/src/core/snapshot/sprite_checkpoint.rs`

## Migrate from git2 to gix (gitoxide)

The project currently uses [git2-rs](https://github.com/rust-lang/git2-rs) for git operations (`src/core/git.rs`). git2 wraps libgit2 (C library) and depends on OpenSSL via libssh2, causing cross-compilation issues on macOS (building x86_64 from aarch64 runners fails due to OpenSSL).

[gix (gitoxide)](https://github.com/GitoxideLabs/gitoxide) is a pure Rust git implementation that would eliminate these issues:
- No C dependencies or OpenSSL
- No cross-compilation problems
- Cargo itself is [migrating to gix](https://github.com/rust-lang/cargo/pull/13592)

Migration scope:
- Refactor `src/core/git.rs` (~1100 lines) to use gix API
- Verify all operations: clone, push, fetch, branches, worktrees, diffs, SSH auth
- gix API differs from git2 (see [migration guide](https://docs.rs/gix/latest/gix/?search=git2))

Current workaround: Use `macos-13` (Intel) runner for x86_64 builds instead of cross-compiling from `macos-latest` (M1).
