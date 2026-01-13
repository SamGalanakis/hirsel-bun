# Eval Specification

Verify the Electrobun-based hirsel rewrite.

## Reference Implementation

The reference implementation is at `references/hirsel/`. This is the existing Python version being rewritten. Key files:

- `references/hirsel/src/hirsel/cli.py` - All CLI commands
- `references/hirsel/src/hirsel/live.py` - TUI dashboard (being replaced with native app)
- `references/hirsel/src/hirsel/state.py` - SQLite state management pattern
- `references/hirsel/src/hirsel/schema.sql` - Database schema

Compare the new implementation against these files to verify correctness.

## Checks

### 1. Project Structure
- [ ] `package.json` exists with `electrobun` dependency
- [ ] `tsconfig.json` exists with `"strict": true`
- [ ] `src/bun/index.ts` exists (main process entry)
- [ ] `src/views/main/index.html` exists (webview entry)

### 2. Build Succeeds
```bash
bun install && bun run build
```
- Must exit 0
- Must produce output in `dist/` or `build/`

### 3. CLI Works
```bash
./dist/hirsel --help
```
- Must show command list including: go, view, log, attach, pause, resume, delete, runs
- Must exit 0

```bash
./dist/hirsel --version
```
- Must print a version string
- Must exit 0

### 4. Desktop App Launches
```bash
timeout 10 bun run dev &
sleep 5
# Check if process is running
pgrep -f electrobun || pgrep -f hirsel
```
- Process must start without immediate crash
- No "Error" or "FATAL" in stderr within first 5 seconds

### 5. State Module
```bash
bun test src/core/state.test.ts 2>/dev/null || echo "no tests yet"
```
- If tests exist, they must pass
- `src/core/state.ts` must exist and export a `State` class or equivalent

### 6. Theme Colors
Check `src/shared/theme.ts` or equivalent for:
- Accent color is warm amber/gold (hex like `#d4a574`, NOT red `#ff0000`)
- Success color is sage green (hex like `#7d9970`)
- Background is dark charcoal (hex like `#1a1a1a`)

### 7. Reference Usage
- Code should demonstrate understanding of `references/hirsel/` architecture
- Similar module structure: state, config, git, workers, etc.

## Pass Criteria

**Call `eval_pass`** if:
- Checks 1-4 all pass (structure, build, CLI, app launches)
- Check 5 passes OR state module exists but tests not yet written
- Check 6 shows correct warm theme (not red accent)

**Call `eval_fail`** with specific feedback if:
- Build fails → include error output
- CLI doesn't work → include what's missing
- App crashes on launch → include error
- Wrong theme colors → specify which colors are wrong
- Missing core files → list what's missing

## Notes

- This is a ground-up rewrite in TypeScript, not a direct port
- Electrobun uses Bun runtime, not Node
- **ALWAYS check `references/hirsel/` to understand expected behavior**
- Compare your implementation against the Python reference for correctness
- Full feature parity is the goal, but foundation first
