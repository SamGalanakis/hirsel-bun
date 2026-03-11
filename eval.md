# Eval Specification

> **HISTORICAL:** This evaluation spec was for the original Rust/Tauri rewrite.
> The frontend has since migrated from Alpine.js to SolidJS.
> See `docs/architecture.html` for current architecture.

Comprehensive verification of the Tauri + Rust hirsel rewrite.

## Reference Implementation

The reference implementation is at `references/hirsel/`. Compare against:

- `references/hirsel/src/hirsel/cli.py` - All CLI commands
- `references/hirsel/src/hirsel/state.py` - SQLite state management
- `references/hirsel/src/hirsel/schema.sql` - Database schema
- `references/hirsel/tests/` - Test patterns to replicate

---

## Test Suite

### 1. Project Structure

Verify required files exist:

```bash
# Must exist
test -f src-tauri/Cargo.toml
test -f src-tauri/src/main.rs
test -f src-tauri/src/lib.rs
test -f src-tauri/tauri.conf.json
test -f src/index.html
test -f src/styles/main.css
test -f package.json
```

Check Cargo.toml has required dependencies:
- [ ] `tauri`
- [ ] `clap`
- [ ] `rusqlite`
- [ ] `serde`
- [ ] `agent-client-protocol`

Check package.json has required dependencies:
- [ ] `@tauri-apps/api`
- [ ] `alpinejs`
- [ ] `basecoat-css`
- [ ] `tailwindcss`

### 2. Rust Build

```bash
cd src-tauri && cargo build --release 2>&1 | tee build.log
```

- [ ] Exit code 0
- [ ] Binary exists at `target/release/hirsel`
- [ ] **No critical warnings** - check build.log for:
  - No `error[E...]`
  - No `warning: unused` on public APIs
  - No `warning: deprecated`
  - Minor warnings (e.g., unused variables in dev) are acceptable

### 3. Frontend Build

```bash
npm install && npm run build
```

- [ ] Exit code 0
- [ ] CSS compiled successfully
- [ ] No TypeScript errors

### 4. CLI Command Tests

Test each CLI command works. Use a temp directory for test runs.

#### Version & Help
```bash
./hirsel --version
# Expected: prints version string, exit 0

./hirsel --help
# Expected: shows command list, exit 0

./hirsel man
# Expected: shows manual with sheep ASCII art, exit 0
```

#### Run Management Commands
```bash
# Create a test project
mkdir -p /tmp/hirsel-test && cd /tmp/hirsel-test
git init && echo "test" > file.txt && git add . && git commit -m "init"

# Test 'runs' (should be empty initially)
./hirsel runs
# Expected: empty list or "no runs", exit 0

./hirsel runs --json
# Expected: valid JSON output, exit 0
```

#### Config Commands
```bash
./hirsel config
# Expected: shows current config or agent selection, exit 0

./hirsel templates
# Expected: lists available templates, exit 0

./hirsel completions bash
# Expected: outputs bash completion script, exit 0
```

### 5. State Management Tests

Test SQLite state operations match the schema:

```bash
# Create a run and verify state database
./hirsel go test-run spec.md --workers 1 --time-limit 1m

# Verify database created
test -f ~/.hirsel/runs/test-run/state.db

# Verify tables exist (using sqlite3)
sqlite3 ~/.hirsel/runs/test-run/state.db ".tables"
# Expected: state tasks workers evals messages history

# Clean up
./hirsel delete test-run
```

### 6. E2E Test: Calculator Scenario

Run the full e2e test with the calculator scenario. This tests:
- Run creation
- Worker spawning
- Task management
- ACP communication
- Eval execution
- Run completion

```bash
# Setup test scenario (use local tests folder)
cp -r tests/scenarios/calculator /tmp/calc-test
cd /tmp/calc-test/project
git init && git add . && git commit -m "init"

# Run hirsel
./hirsel go calc-e2e ../spec.md --workers 1 --time-limit 5m

# Wait for completion or timeout
# Poll status until done or timeout
for i in {1..60}; do
  status=$(./hirsel view calc-e2e --json 2>/dev/null | jq -r '.status')
  if [ "$status" = "done" ] || [ "$status" = "delivered" ]; then
    break
  fi
  sleep 5
done

# Verify results
./hirsel view calc-e2e
# Expected: status is 'done' or 'delivered'

# Run eval on the result
cd /tmp/calc-test/project
pytest -v
# Expected: all tests pass (multiply, divide functions added)

# Clean up
./hirsel delete calc-e2e
```

### 7. Desktop App Launch Test

```bash
# Start the app in background
timeout 10 cargo tauri dev &
APP_PID=$!
sleep 5

# Verify process is running
ps -p $APP_PID > /dev/null
# Expected: process exists

# Check no panics in stderr
# (capture stderr during startup)

# Clean up
kill $APP_PID 2>/dev/null
```

### 8. Theme Verification

Check CSS variables for correct hirsel theme:

```bash
grep -E "#d4a574|#7d9970|#1a1a1a" src/styles/main.css
```

- [ ] Amber accent: `#d4a574` (NOT red)
- [ ] Sage green: `#7d9970`
- [ ] Dark background: `#1a1a1a`

### 9. Basecoat UI Integration

```bash
# Check Basecoat is imported
grep -E "@import.*basecoat|basecoat-css" src/styles/main.css src/index.html

# Check Alpine.js is included
grep -E "alpinejs|x-data|x-init" src/index.html
```

---

## Pass Criteria

**Call `eval_pass`** if ALL of the following:

1. ✅ Project structure complete (all required files exist)
2. ✅ Rust builds without errors and no critical warnings
3. ✅ Frontend builds without errors
4. ✅ `hirsel --help` shows all commands
5. ✅ `hirsel --version` works
6. ✅ `hirsel runs` works (even if empty)
7. ✅ Desktop app launches without crash
8. ✅ Theme colors are correct (amber, sage, not red)
9. ✅ Basecoat UI + Alpine.js integrated

**Bonus (not required for pass):**
- E2E calculator test completes successfully
- All CLI commands functional
- Multi-worker support works

---

## Fail Criteria

**Call `eval_fail`** with specific feedback if:

| Issue | Feedback |
|-------|----------|
| Cargo build fails | Include compiler error output |
| Frontend build fails | Include npm/build error |
| CLI missing commands | List which commands are missing |
| App crashes on launch | Include panic/error message |
| Wrong theme colors | Specify which colors are wrong |
| Missing core files | List what's missing |
| Wrong frontend framework | Specify what was found (e.g., React instead of Alpine) |
| State schema mismatch | Show diff from expected schema |

---

## Test Environment Setup

Before running tests:

```bash
# Ensure clean state
rm -rf ~/.hirsel/runs/test-*
rm -rf /tmp/hirsel-test

# Required tools
which cargo rustc sqlite3 jq npm

# Build first
cd src-tauri && cargo build --release
cd .. && npm install
```

---

## Notes

- This is a Rust rewrite, not a TypeScript port
- Tauri v2 required
- Frontend: Alpine.js + Tailwind + Basecoat (no React/Vue/Svelte)
- Reference `references/hirsel/` for expected behavior
- The calculator e2e test is the gold standard for functionality
- Feature parity with Python CLI is the goal
