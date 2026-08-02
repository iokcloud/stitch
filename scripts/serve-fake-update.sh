#!/usr/bin/env bash
# Serve e2e/fixtures/fake-update on 127.0.0.1:18765 for U1 updater discover.
# Foreground. Prefer smoke-updater-discover.sh which starts/stops this for you.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/e2e/fixtures/fake-update"
PORT="${STITCH_FAKE_UPDATE_PORT:-18765}"

if [[ ! -f "$DIR/stitch-update.json" ]]; then
  echo "error: missing $DIR/stitch-update.json" >&2
  exit 1
fi

cd "$DIR"
echo "Serving fake update manifest at http://127.0.0.1:${PORT}/stitch-update.json"
exec python -m http.server "$PORT" --bind 127.0.0.1
