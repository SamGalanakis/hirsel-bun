# Future Ideas

## GUI: Custom Font

Consider replacing system fonts with a dedicated typeface (Inter, IBM Plex Sans, Geist). ET Book was considered but rejected—serif fonts don't suit dense developer UIs, and fine serifs render poorly at small sizes.

## Migrate git2 to gix

git2-rs wraps libgit2 (C) and depends on OpenSSL, causing macOS cross-compilation issues. [gix](https://github.com/GitoxideLabs/gitoxide) is pure Rust with no C dependencies.

**Scope:** Refactor `src/core/git.rs` (~1100 lines) to use gix API.

**Current workaround:** Use `macos-13` (Intel) runner for x86_64 builds.
