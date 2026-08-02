#!/usr/bin/env bash
# 从已签名的 NSIS + .sig 生成静态更新清单，写入 rust/data/downloads/
# 用法：
#   sh scripts/publish-update-manifest.sh
# 可选：
#   STITCH_SETUP_EXE       安装包路径
#   STITCH_DOWNLOAD_BASE   清单内 URL 前缀（默认 https://www.promptstdio.com/downloads）
set -euo pipefail

to_py_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s' "$1"
  fi
}

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO_RUST="$(cd "$ROOT/../.." && pwd)"
CONF="$ROOT/tauri.conf.json"
OUT_DIR="$REPO_RUST/data/downloads"
BASE_URL="${STITCH_DOWNLOAD_BASE:-https://www.promptstdio.com/downloads}"

export STITCH_CONF_PY="$(to_py_path "$CONF")"
VERSION="$(python -c 'import json,os; print(json.load(open(os.environ["STITCH_CONF_PY"]))["version"])')"
SETUP_NAME="Stitch_${VERSION}_x64-setup.exe"

if [[ -n "${STITCH_SETUP_EXE:-}" ]]; then
  SETUP="$STITCH_SETUP_EXE"
else
  SETUP="$REPO_RUST/target/release/bundle/nsis/$SETUP_NAME"
fi

SIG="${SETUP}.sig"
if [[ ! -f "$SETUP" ]]; then
  echo "ERROR: missing setup: $SETUP" >&2
  exit 1
fi
if [[ ! -f "$SIG" ]]; then
  echo "ERROR: missing signature: $SIG (build with TAURI_SIGNING_PRIVATE_KEY*)" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
cp -f "$SETUP" "$OUT_DIR/$SETUP_NAME"
cp -f "$SIG" "$OUT_DIR/${SETUP_NAME}.sig"

export STITCH_PUBLISH_VERSION="$VERSION"
export STITCH_PUBLISH_SETUP_NAME="$SETUP_NAME"
export STITCH_PUBLISH_BASE_URL="$BASE_URL"
export STITCH_PUBLISH_SIG="$(to_py_path "$SIG")"
export STITCH_PUBLISH_OUT="$(to_py_path "$OUT_DIR/stitch-update.json")"

python <<'PY'
import json, os
from datetime import datetime, timezone

with open(os.environ["STITCH_PUBLISH_SIG"], encoding="utf-8") as f:
    signature = f.read().strip().replace("\r", "").replace("\n", "")

name = os.environ["STITCH_PUBLISH_SETUP_NAME"]
base = os.environ["STITCH_PUBLISH_BASE_URL"].rstrip("/")
doc = {
    "version": os.environ["STITCH_PUBLISH_VERSION"],
    "notes": "",
    "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {
        "windows-x86_64": {
            "signature": signature,
            "url": f"{base}/{name}",
        }
    },
}
out = os.environ["STITCH_PUBLISH_OUT"]
with open(out, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, ensure_ascii=False, indent=2)
    f.write("\n")
print(f"OK: {out}")
print(f"    version={doc['version']}")
print(f"    url={doc['platforms']['windows-x86_64']['url']}")
PY

echo "Next: sync $OUT_DIR/{stitch-update.json,$SETUP_NAME} to production downloads."
