#!/usr/bin/env bash
# Deploy gate: build webdriver debug exe, send「审查本次改动」, write REVIEW.md.
# Does NOT deploy. Cursor Agent must read e2e/artifacts/pre-deploy-review/REVIEW.md
# then ask the user to confirm before promptstdio-deploy.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
E2E="$ROOT/e2e"

cd "$ROOT"
sh scripts/build-ui.sh

cd "$REPO/rust"
(
  export TAURI_CONFIG
  TAURI_CONFIG="$(node -e "const fs=require('fs'); process.stdout.write(fs.readFileSync(process.argv[1],'utf8'))" "$ROOT/e2e/tauri.webdriver.json")"
  cargo build -p stitch-desktop --features webdriver
)

EMBEDDED_PORT="${WDIO_EMBEDDED_PORT:-17445}"
export WDIO_EMBEDDED_PORT="$EMBEDDED_PORT"

if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* || -n "${WINDIR:-}" ]]; then
  BIN="$REPO/rust/target/debug/stitch-desktop.exe"
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
npm run pre-deploy-review "$@"

echo ""
echo "Review dump: $E2E/artifacts/pre-deploy-review/REVIEW.md"
echo "Next: Agent reads REVIEW.md → fix → user confirms → deploy"
