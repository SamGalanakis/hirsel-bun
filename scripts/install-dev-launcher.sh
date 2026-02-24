#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LAUNCHER_PATH="${HIRSEL_LAUNCHER_PATH:-$HOME/.local/bin/hirsel}"

mkdir -p "$(dirname "$LAUNCHER_PATH")"

cat >"$LAUNCHER_PATH" <<EOF
#!/usr/bin/env bash
set -euo pipefail

repo_root="\${HIRSEL_REPO:-$REPO_ROOT}"
manifest="\$repo_root/src-tauri/Cargo.toml"
bin="\$repo_root/src-tauri/target/debug/hirsel"
dev_script="\$repo_root/dev.sh"

if [[ ! -f "\$manifest" ]]; then
  echo "hirsel launcher error: manifest not found at \$manifest" >&2
  exit 1
fi

if [[ ! -x "\$dev_script" ]]; then
  echo "hirsel launcher error: dev script not executable at \$dev_script" >&2
  exit 1
fi

# Dev-mode passthrough:
# - no args -> equivalent to ./dev.sh
# - only dev flags -> equivalent to ./dev.sh [--mcp] [--profiling]
if [[ \$# -eq 0 ]]; then
  cd "\$repo_root"
  exec "\$dev_script"
fi

dev_flags_only=true
for arg in "\$@"; do
  case "\$arg" in
    --mcp|--profiling) ;;
    *) dev_flags_only=false; break ;;
  esac
done

if [[ "\$dev_flags_only" == true ]]; then
  cd "\$repo_root"
  exec "\$dev_script" "\$@"
fi

if [ -t 1 ]; then
  log="/tmp/hirsel-build-\$\$.log"
  cargo build --manifest-path "\$manifest" >"\$log" 2>&1 &
  pid=\$!

  spin='|/-\\'
  i=0
  while kill -0 "\$pid" 2>/dev/null; do
    c=\${spin:i%4:1}
    printf "\\rBuilding hirsel %s" "\$c"
    sleep 0.1
    i=\$((i + 1))
  done

  if wait "\$pid"; then
    rm -f "\$log"
    printf "\\rBuild complete.      \\n"
  else
    printf "\\rBuild failed.\\n" >&2
    cat "\$log" >&2 || true
    rm -f "\$log"
    exit 1
  fi
else
  cargo build --manifest-path "\$manifest" >/dev/null 2>&1
fi

exec "\$bin" "\$@"
EOF

chmod +x "$LAUNCHER_PATH"
echo "Installed hirsel launcher at: $LAUNCHER_PATH"
