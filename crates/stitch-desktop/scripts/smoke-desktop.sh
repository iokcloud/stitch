#!/usr/bin/env bash
# Layer B: WebdriverIO + Tauri desktop smoke (real .exe on Windows/Linux).
# Requires: Rust toolchain, npm. Closes any running Stitch first (single-instance).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
E2E="$ROOT/e2e"

# Drop yesterday's WDIO/Playwright artifacts before this run writes new shots.
sh "$ROOT/scripts/clean-e2e-artifacts.sh"

cd "$ROOT"
sh scripts/build-ui.sh

cd "$REPO/rust"
# Enable embedded WebDriver plugin + inline capability (not in production default).
# Use a subshell so TAURI_CONFIG never leaks into the caller environment.
# Read JSON via node fs (Git Bash paths → Windows paths for require are flaky).
(
  export TAURI_CONFIG
  TAURI_CONFIG="$(node -e "const fs=require('fs'); process.stdout.write(fs.readFileSync(process.argv[1],'utf8'))" "$ROOT/e2e/tauri.webdriver.json")"
  cargo build -p stitch-desktop --features webdriver
)

EMBEDDED_PORT="${WDIO_EMBEDDED_PORT:-17445}"
export WDIO_EMBEDDED_PORT="$EMBEDDED_PORT"

if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* || -n "${WINDIR:-}" ]]; then
  BIN="$REPO/rust/target/debug/stitch-desktop.exe"
  # Best-effort: release single-instance lock + free embedded WebDriver port.
  taskkill //F //IM stitch-desktop.exe 2>/dev/null || true
  taskkill //F //IM msedgedriver.exe 2>/dev/null || true
  taskkill //F //IM tauri-driver.exe 2>/dev/null || true
else
  BIN="$REPO/rust/target/debug/stitch-desktop"
  pkill -x stitch-desktop 2>/dev/null || true
fi

if [[ ! -f "$BIN" ]]; then
  echo "error: binary not found: $BIN" >&2
  exit 1
fi

cd "$E2E"
npm install

export STITCH_APP_BINARY="$BIN"
set +e
npm run smoke "$@"
SMOKE_RC=$?
set -e

# Windows: optionally re-open Explorer on target/debug after WDIO.
# Default OFF — do not disturb the user's desktop (set STITCH_KEEP_DEBUG_DIR=1 to enable).
if [[ "${STITCH_KEEP_DEBUG_DIR:-0}" != "0" ]]; then
  if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* || -n "${WINDIR:-}" ]]; then
    DEBUG_DIR="$REPO/rust/target/debug"
    if [[ -d "$DEBUG_DIR" ]]; then
      DEBUG_WIN="$(cd "$DEBUG_DIR" && pwd -W 2>/dev/null || true)"
      if [[ -z "$DEBUG_WIN" ]]; then
        DEBUG_WIN="$(cygpath -w "$DEBUG_DIR" 2>/dev/null || echo "$DEBUG_DIR")"
      fi
      cmd.exe //c start "" explorer.exe "${DEBUG_WIN}" >/dev/null 2>&1 || true
    fi
  fi
fi

exit "$SMOKE_RC"
