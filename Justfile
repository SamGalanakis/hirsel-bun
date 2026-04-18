set shell := ["bash", "-euo", "pipefail", "-c"]

repo_root := justfile_directory()
bin := repo_root + "/target/debug/hirsel"
log_file := repo_root + "/dev.log"
env_loader := repo_root + "/scripts/load-hirsel-env.sh"
tmp_dir := repo_root + "/.tmp"
port := env_var_or_default("HIRSEL_PORT", "8484")
dev_root_default := repo_root + "/.hirsel-dev"

default:
    @just --list

# Build the hirsel binary.
build profile="dev":
    #!/usr/bin/env bash
    source {{ env_loader }}
    hirsel_load_dotenv {{ repo_root }}
    export TMPDIR="${TMPDIR:-{{ tmp_dir }}}"
    mkdir -p "$TMPDIR"
    cargo build -p hirsel-cli{{ if profile == "dev" { "" } else { " --profile " + profile } }}

# Build the web UI.
build-web:
    cd {{ repo_root }}/webui && bun run build

# Build everything (binary + web UI).
build-all: build build-web

# Run the daemon after building.
serve *args: build build-web
    #!/usr/bin/env bash
    source {{ env_loader }}
    hirsel_load_dotenv {{ repo_root }}
    export HIRSEL_ROOT="${HIRSEL_ROOT:-{{ dev_root_default }}}"
    mkdir -p "$HIRSEL_ROOT"
    exec {{ bin }} serve --port {{ port }} {{ args }}

# Dev mode: build and run with live output.
dev:
    #!/usr/bin/env bash
    source {{ env_loader }}
    hirsel_load_dotenv {{ repo_root }}
    export TMPDIR="${TMPDIR:-{{ tmp_dir }}}"
    mkdir -p "$TMPDIR"
    export HIRSEL_ROOT="${HIRSEL_ROOT:-{{ dev_root_default }}}"
    export RUST_LOG="${RUST_LOG:-hirsel=info}"
    mkdir -p "$HIRSEL_ROOT"

    echo "Building hirsel..."
    cargo build -p hirsel-cli 2>&1 | tee {{ log_file }}

    backend_pid=""
    vite_pid=""
    cleanup() {
        local code=$?
        trap - EXIT INT TERM
        if [[ -n "${vite_pid:-}" ]] && kill -0 "$vite_pid" 2>/dev/null; then
            kill "$vite_pid" 2>/dev/null || true
        fi
        if [[ -n "${backend_pid:-}" ]] && kill -0 "$backend_pid" 2>/dev/null; then
            kill "$backend_pid" 2>/dev/null || true
        fi
        wait "${vite_pid:-}" 2>/dev/null || true
        wait "${backend_pid:-}" 2>/dev/null || true
        exit "$code"
    }
    trap cleanup EXIT INT TERM

    echo
    echo "Starting hirsel dev stack"
    echo "  Root: $HIRSEL_ROOT"
    echo "  API:  http://127.0.0.1:{{ port }}"
    echo "  UI:   http://127.0.0.1:5199"
    echo

    {{ bin }} serve --port {{ port }} 2>&1 | tee -a {{ log_file }} &
    backend_pid=$!

    (
        cd {{ repo_root }}/webui
        HIRSEL_PORT={{ port }} bun run dev -- --host 127.0.0.1
    ) 2>&1 | tee -a {{ log_file }} &
    vite_pid=$!

    wait -n "$backend_pid" "$vite_pid"

# Stop any running hirsel processes.
stop:
    #!/usr/bin/env bash
    set +e
    pkill -f "target/debug/hirsel serve" 2>/dev/null || true
    echo "Stopped."

# Reset the local Hirsel database under HIRSEL_ROOT.
reset-db:
    #!/usr/bin/env bash
    source {{ env_loader }}
    hirsel_load_dotenv {{ repo_root }}
    export HIRSEL_ROOT="${HIRSEL_ROOT:-{{ dev_root_default }}}"
    db_path="$HIRSEL_ROOT/hirsel.surrealkv"
    echo "Resetting Hirsel DB: $db_path"
    rm -rf "$db_path"
    echo "Done."

# Run cargo check.
check:
    cargo check --workspace

# Run cargo clippy.
clippy:
    cargo clippy --workspace

# Run frontend lint.
lint-web:
    cd {{ repo_root }}/webui && bun run lint
