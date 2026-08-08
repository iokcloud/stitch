#!/usr/bin/env bash
# U1: real exe checks local fake stitch-update.json (version > app) → 发现新版本.
# Does not touch production downloads. Does not exercise install (U2).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
E2E="$ROOT/e2e"

# 前置断言：fake manifest 版本必须高于 app 版本（否则 discover 必然「已是最新」）
APP_VERSION="$(python -c 'import json;print(json.load(open(r"'$ROOT'/tauri.conf.json"))["version"])' 2>/dev/null || echo 0.0.0)"
FAKE_VERSION="$(python -c 'import json;print(json.load(open(r"'$E2E'/fixtures/fake-update/stitch-update.json"))["version"])' 2>/dev/null || echo 0.0.0)"
if [[ "$FAKE_VERSION" < "$APP_VERSION" ]]; then
  echo "FAIL: fake fixture version $FAKE_VERSION < app $APP_VERSION — updater discover 必然失败。请先升级 e2e/fixtures/fake-update/stitch-update.json。" >&2
  exit 1
fi
PORT="${STITCH_FAKE_UPDATE_PORT:-18765}"
PID_FILE="${TMPDIR:-/tmp}/stitch-fake-update-${PORT}.pid"

cleanup() {
  if [[ -f "$PID_FILE" ]]; then
    kill "$(cat "$PID_FILE")" 2>/dev/null || true
    rm -f "$PID_FILE"
  fi
}
trap cleanup EXIT

sh "$ROOT/scripts/clean-e2e-artifacts.sh"

# Free port if leftover
if command -v curl >/dev/null 2>&1; then
  if curl -fsS --max-time 1 "http://127.0.0.1:${PORT}/stitch-update.json" >/dev/null 2>&1; then
    echo "warn: port ${PORT} already serving; reusing" >&2
  else
    (
      cd "$E2E/fixtures/fake-update"
      python -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1 &
      echo $! >"$PID_FILE"
    )
    sleep 0.5
  fi
else
  (
    cd "$E2E/fixtures/fake-update"
    python -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1 &
    echo $! >"$PID_FILE"
  )
  sleep 0.5
fi

curl -fsS --max-time 3 "http://127.0.0.1:${PORT}/stitch-update.json" | head -c 80 >/dev/null

cd "$ROOT"
sh scripts/build-ui.sh

cd "$REPO/rust"
(
  export TAURI_CONFIG
  TAURI_CONFIG="$(node -e "const fs=require('fs'); process.stdout.write(fs.readFileSync(process.argv[1],'utf8'))" "$ROOT/e2e/tauri.updater-discover.json")"
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
npm install --silent >/dev/null 2>&1 || npm install

export STITCH_APP_BINARY="$BIN"
npm run updater-discover

echo "U1 discover PASS. Rebuild with tauri.webdriver.json (no fake endpoint) before other e2e."
