#!/usr/bin/env bash
# U2: local signed install — app stays current tauri.conf version; fixture claims higher
# version but serves real signed Stitch_0.1.2 NSIS bytes (+ matching .sig).
# Proves download + signature verify + NSIS start. Does NOT change production downloads.
# After run: rebuild with tauri.webdriver.json before other desktop e2e.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/../../.." && pwd)"
E2E="$ROOT/e2e"
FIX="$E2E/fixtures/u2-update"
DL="$REPO/rust/data/downloads"
PORT="${STITCH_FAKE_UPDATE_PORT:-18765}"
PID_FILE="${TMPDIR:-/tmp}/stitch-u2-update-${PORT}.pid"
SETUP_NAME="Stitch_0.1.2_x64-setup.exe"
SETUP="$DL/$SETUP_NAME"
SIG="$DL/${SETUP_NAME}.sig"

cleanup() {
  if [[ -f "$PID_FILE" ]]; then
    kill "$(cat "$PID_FILE")" 2>/dev/null || true
    rm -f "$PID_FILE"
  fi
}
trap cleanup EXIT

if [[ ! -f "$SETUP" || ! -f "$SIG" ]]; then
  echo "error: need signed $SETUP_NAME (+ .sig) under rust/data/downloads/" >&2
  echo "       build+sign once, then sh scripts/publish-update-manifest.sh" >&2
  exit 1
fi

sh "$ROOT/scripts/clean-e2e-artifacts.sh"

mkdir -p "$FIX"
cp -f "$SETUP" "$FIX/$SETUP_NAME"
cp -f "$SIG" "$FIX/${SETUP_NAME}.sig"

export STITCH_U2_FIX="$(
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$FIX"; else printf '%s' "$FIX"; fi
)"
export STITCH_U2_SETUP_NAME="$SETUP_NAME"
export STITCH_U2_PORT="$PORT"

python <<'PY'
import json, os
from datetime import datetime, timezone
from pathlib import Path

fix = Path(os.environ["STITCH_U2_FIX"])
name = os.environ["STITCH_U2_SETUP_NAME"]
port = os.environ["STITCH_U2_PORT"]
sig = (fix / f"{name}.sig").read_text(encoding="utf-8").strip().replace("\r", "").replace("\n", "")
doc = {
    "version": "0.1.3",
    "notes": "e2e U2 local signed bytes of 0.1.2 NSIS — do not publish",
    "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {
        "windows-x86_64": {
            "signature": sig,
            "url": f"http://127.0.0.1:{port}/{name}",
        }
    },
}
out = fix / "stitch-update.json"
out.write_text(json.dumps(doc, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"OK: fixture {out} → {doc['platforms']['windows-x86_64']['url']}")
PY

# Free / start static server on fixture dir
if command -v curl >/dev/null 2>&1 && curl -fsS --max-time 1 "http://127.0.0.1:${PORT}/stitch-update.json" >/dev/null 2>&1; then
  echo "warn: port ${PORT} already serving; restarting from U2 fixture" >&2
  if [[ -f "$PID_FILE" ]]; then
    kill "$(cat "$PID_FILE")" 2>/dev/null || true
    rm -f "$PID_FILE"
  fi
  # Best-effort: stop any python http.server bound to this port (Git Bash / Windows).
  if command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -Command \
      "Get-NetTCPConnection -LocalPort ${PORT} -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }" \
      >/dev/null 2>&1 || true
  fi
  sleep 0.5
fi

(
  cd "$FIX"
  python -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1 &
  echo $! >"$PID_FILE"
)
sleep 0.5
curl -fsS --max-time 3 "http://127.0.0.1:${PORT}/stitch-update.json" | head -c 120 >/dev/null

cd "$ROOT"
sh scripts/build-ui.sh

cd "$REPO/rust"
(
  export TAURI_CONFIG
  TAURI_CONFIG="$(node -e "const fs=require('fs'); process.stdout.write(fs.readFileSync(process.argv[1],'utf8'))" "$ROOT/e2e/tauri.updater-u2.json")"
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
# Skip nested clean-artifacts (already cleaned); run WDIO directly.
npx wdio run wdio.conf.ts --spec ./specs/updater-install.spec.ts

echo "U2 local signed install PASS (discover + install click / session exit)."
echo "Rebuild with tauri.webdriver.json (no fake endpoint) before other e2e:"
echo "  TAURI_CONFIG=\$(cat e2e/tauri.webdriver.json) cargo build -p stitch-desktop --features webdriver"
