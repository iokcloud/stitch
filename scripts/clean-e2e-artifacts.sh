#!/usr/bin/env bash
# Prune Stitch e2e screenshots / temp reports older than MAX_AGE (default 1 hour).
# Keeps the latest run for Layer V; frees disk from stale shots & example dumps.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIRS=(
  "$ROOT/e2e/artifacts"
  "$ROOT/frontend/e2e/artifacts"
  "$ROOT/frontend/test-results"
  "$ROOT/frontend/playwright-report"
)

# Default: drop files older than 1 hour.
MAX_AGE_SEC="${STITCH_ARTIFACT_MAX_AGE_SEC:-3600}"
# Optional: STITCH_ARTIFACT_CLEAN_ALL=1 wipes trees entirely.
CLEAN_ALL="${STITCH_ARTIFACT_CLEAN_ALL:-0}"

now="$(date +%s)"
cutoff=$((now - MAX_AGE_SEC))

removed=0
bytes=0

prune_tree() {
  local dir="$1"
  [[ -d "$dir" ]] || return 0

  if [[ "$CLEAN_ALL" == "1" ]]; then
    local sz
    sz="$(du -sb "$dir" 2>/dev/null | awk '{print $1}')" || sz=0
    rm -rf "$dir"
    mkdir -p "$dir"
    removed=$((removed + 1))
    bytes=$((bytes + sz))
    echo "clean-e2e-artifacts: wiped $dir"
    return 0
  fi

  while IFS= read -r -d '' f; do
    local m sz
    m="$(stat -c %Y "$f" 2>/dev/null || stat -f %m "$f" 2>/dev/null || echo 0)"
    if [[ "$m" -lt "$cutoff" ]]; then
      sz="$(stat -c %s "$f" 2>/dev/null || stat -f %z "$f" 2>/dev/null || echo 0)"
      rm -f "$f"
      removed=$((removed + 1))
      bytes=$((bytes + sz))
    fi
  done < <(find "$dir" -type f -print0 2>/dev/null)

  find "$dir" -mindepth 1 -type d -empty -delete 2>/dev/null || true
}

for d in "${DIRS[@]}"; do
  prune_tree "$d"
done

kb=$((bytes / 1024))
echo "clean-e2e-artifacts: removed $removed file(s), ~${kb} KiB (max_age_sec=$MAX_AGE_SEC cutoff=$cutoff)"
