#!/usr/bin/env bash
# Layer A: Playwright browser smoke (mock Tauri IPC). Fast, no .exe.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FRONTEND="$ROOT/frontend"

# Drop yesterday's e2e screenshots / Playwright reports before generating new ones.
sh "$ROOT/scripts/clean-e2e-artifacts.sh"

cd "$FRONTEND"

# Ensure lockfile picks up @playwright/test (and friends).
npm install
npm run build
# Default: system Chrome via channel:"chrome" (no CDN download).
# Optional bundled browser: PLAYWRIGHT_CHROMIUM=1 npx playwright install chromium
if [[ "${PLAYWRIGHT_CHROMIUM:-}" == "1" ]]; then
  npx playwright install chromium
fi
npx playwright test "$@"
