#!/usr/bin/env bash
# Build SvelteKit (adapter-static) UI into frontend/build
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/frontend"
if [[ ! -d node_modules ]]; then
  npm install
fi
npm run build
# Force Cargo/tauri-build to re-embed frontendDist even when only UI changed.
# Without this, `cargo build` may reuse a stale embedded webview bundle.
touch "$ROOT/build.rs"
echo "OK: frontend/build ready (build.rs touched for re-embed)"
