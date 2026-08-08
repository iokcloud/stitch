#!/usr/bin/env bash
# Stitch Desktop auto-acceptance (project-local — do not replace with external npx Skill packs).
#
# Pattern aligned with Cursor create-verification-skill:
#   drive real harness → capture evidence → print PASS/FAIL report → exit code.
#
# Usage (Git Bash, from crate root or repo root):
#   bash rust/crates/stitch-desktop/scripts/accept.sh
#   bash rust/crates/stitch-desktop/scripts/accept.sh --layers A
#   bash rust/crates/stitch-desktop/scripts/accept.sh --layers A,B
#   bash rust/crates/stitch-desktop/scripts/accept.sh --layers A,mature
#   bash rust/crates/stitch-desktop/scripts/accept.sh --layers A,B,mature
#   bash rust/crates/stitch-desktop/scripts/accept.sh --layers updater  # U0 prod latest
#   sh scripts/accept.sh --layers A,updater
#
# Agent MUST run this before claiming Stitch UI/desktop work done.
# Default delivery gate is A,B (Playwright + real exe). Do not ship on A alone.
# Paste the printed 验收 block into the user reply. For Layer V, Read the listed PNGs.
# U1 discover (local fake): sh scripts/smoke-updater-discover.sh (not in default layers).
# U2 local signed install: sh scripts/smoke-updater-u2.sh (not in default layers; rebuild webdriver after).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPORT_DIR="$ROOT/e2e/artifacts"
REPORT="$REPORT_DIR/ACCEPTANCE-REPORT.md"
LAYERS="A,B"
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --layers)
      LAYERS="${2:-A}"
      shift 2
      ;;
    --layers=*)
      LAYERS="${1#*=}"
      shift
      ;;
    *)
      EXTRA_ARGS+=("$1")
      shift
      ;;
  esac
done

IFS=',' read -r -a LAYER_LIST <<< "$LAYERS"
NEED_A=0
NEED_B=0
NEED_MATURE=0
NEED_UPDATER=0
for L in "${LAYER_LIST[@]}"; do
  case "$(echo "$L" | tr '[:lower:]' '[:upper:]' | xargs)" in
    A|UI) NEED_A=1 ;;
    B|DESKTOP) NEED_B=1 ;;
    MATURE|M) NEED_MATURE=1 ;;
    UPDATER|U0) NEED_UPDATER=1 ;;
    ALL)
      NEED_A=1
      NEED_B=1
      NEED_MATURE=1
      NEED_UPDATER=1
      ;;
    *)
      echo "accept.sh: unknown layer '$L' (use A, B, mature, updater, or all)" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$REPORT_DIR"
STARTED_AT="$(date -Iseconds 2>/dev/null || date)"
A_LINE="skipped"
B_LINE="skipped"
MATURE_LINE="skipped"
UPDATER_LINE="skipped"
A_RC=0
B_RC=0
MATURE_RC=0
UPDATER_RC=0
VERDICT="PASS"

run_a() {
  echo "==> Layer A (smoke-ui / Playwright)"
  set +e
  sh "$ROOT/scripts/smoke-ui.sh" "${EXTRA_ARGS[@]}"
  A_RC=$?
  set -e
  if [[ $A_RC -eq 0 ]]; then
    A_LINE="PASS (exit 0)"
  else
    A_LINE="FAIL (exit $A_RC)"
    VERDICT="FAIL"
  fi
}

run_b() {
  echo "==> Layer B (smoke-desktop / real exe)"
  echo "    covers: settings↔chat · workspace · account/sediment（默认不改主题、测完不弹资源管理器）"
  echo "    not included: theme-smoke / mature / agent-rich / updater（需显式加跑）"
  set +e
  sh "$ROOT/scripts/smoke-desktop.sh"
  B_RC=$?
  set -e
  if [[ $B_RC -eq 0 ]]; then
    B_LINE="PASS (desktop-smoke: nav + workspace + account)"
  else
    B_LINE="FAIL (exit $B_RC)"
    VERDICT="FAIL"
  fi
}

run_mature() {
  echo "==> Mature scenes (WDIO + DeepSeek; needs webdriver exe + key)"
  local e2e="$ROOT/e2e"
  if [[ ! -f "$e2e/package.json" ]]; then
    MATURE_LINE="FAIL (e2e/package.json missing)"
    VERDICT="FAIL"
    MATURE_RC=1
    return
  fi
  (
    cd "$e2e"
    npm install --silent >/dev/null 2>&1 || npm install
  )
  set +e
  (
    cd "$e2e"
    npm run mature-debug-recover &&
      npm run mature-checkpoint-resume &&
      npm run mature-merge-ready &&
      npm run mature-scope-lock
  )
  MATURE_RC=$?
  set -e
  if [[ $MATURE_RC -eq 0 ]]; then
    MATURE_LINE="PASS (debug-recover + checkpoint-resume + merge-ready + scope-lock)"
  else
    MATURE_LINE="FAIL (exit $MATURE_RC)"
    VERDICT="FAIL"
  fi
}

run_updater() {
  echo "==> Updater U0 (prod stitch-update.json → 已是最新 / 端点通)"
  local e2e="$ROOT/e2e"
  if [[ ! -f "$e2e/package.json" ]]; then
    UPDATER_LINE="FAIL (e2e/package.json missing)"
    VERDICT="FAIL"
    UPDATER_RC=1
    return
  fi
  (
    cd "$e2e"
    npm install --silent >/dev/null 2>&1 || npm install
  )
  set +e
  (
    cd "$e2e"
    npm run updater-check
  )
  UPDATER_RC=$?
  set -e
  if [[ $UPDATER_RC -eq 0 ]]; then
    UPDATER_LINE="PASS (updater-check / U0)"
  else
    UPDATER_LINE="FAIL (exit $UPDATER_RC)"
    VERDICT="FAIL"
  fi
}

[[ $NEED_A -eq 1 ]] && run_a
[[ $NEED_B -eq 1 ]] && run_b
[[ $NEED_MATURE -eq 1 ]] && run_mature
[[ $NEED_UPDATER -eq 1 ]] && run_updater

SHOTS=()
if [[ -d "$ROOT/frontend/e2e/artifacts/mature-entry" ]]; then
  while IFS= read -r -d '' f; do
    SHOTS+=("$f")
  done < <(find "$ROOT/frontend/e2e/artifacts/mature-entry" -type f -name '*.png' -print0 2>/dev/null || true)
fi
if [[ -d "$ROOT/e2e/artifacts/desktop-smoke" ]]; then
  while IFS= read -r -d '' f; do
    SHOTS+=("$f")
  done < <(find "$ROOT/e2e/artifacts/desktop-smoke" -type f -name '*.png' -print0 2>/dev/null || true)
fi

LAYER_SUMMARY=""
[[ $NEED_A -eq 1 ]] && LAYER_SUMMARY="A"
[[ $NEED_B -eq 1 ]] && LAYER_SUMMARY="${LAYER_SUMMARY:+$LAYER_SUMMARY+}B"
[[ $NEED_MATURE -eq 1 ]] && LAYER_SUMMARY="${LAYER_SUMMARY:+$LAYER_SUMMARY+}mature"
[[ $NEED_UPDATER -eq 1 ]] && LAYER_SUMMARY="${LAYER_SUMMARY:+$LAYER_SUMMARY+}updater"
[[ -z "$LAYER_SUMMARY" ]] && LAYER_SUMMARY="(none)"

{
  echo "# Stitch Desktop ACCEPTANCE REPORT"
  echo
  echo "- started: $STARTED_AT"
  echo "- layers: $LAYERS"
  echo "- verdict: **$VERDICT**"
  echo
  echo "## Results"
  echo
  echo "| Layer | Result |"
  echo "| ----- | ------ |"
  echo "| A (Playwright) | $A_LINE |"
  echo "| B (Desktop exe) | $B_LINE |"
  echo "| Mature e2e | $MATURE_LINE |"
  echo "| Updater U0 | $UPDATER_LINE |"
  echo
  echo "## Layer V shots (Agent must Read if UI chrome changed)"
  echo
  if [[ ${#SHOTS[@]} -eq 0 ]]; then
    echo "_No PNGs found (Layer A mature-entry / Layer B desktop-smoke)._"
  else
    for s in "${SHOTS[@]}"; do
      echo "- \`$s\`"
    done
  fi
  echo
  echo "## 验收块（粘贴到用户回复）"
  echo
  echo '```'
  echo "验收:"
  echo "- [$([ "$VERDICT" = PASS ] && echo x || echo ' ')] 层: $LAYER_SUMMARY"
  echo "- [$([ "$A_RC" -eq 0 ] || [ $NEED_A -eq 0 ] && echo x || echo ' ')] A: $A_LINE"
  if [[ $NEED_B -eq 1 ]]; then
    echo "- [$([ "$B_RC" -eq 0 ] && echo x || echo ' ')] B: $B_LINE"
  fi
  if [[ $NEED_MATURE -eq 1 ]]; then
    echo "- [$([ "$MATURE_RC" -eq 0 ] && echo x || echo ' ')] mature: $MATURE_LINE"
  fi
  if [[ $NEED_UPDATER -eq 1 ]]; then
    echo "- [$([ "$UPDATER_RC" -eq 0 ] && echo x || echo ' ')] updater U0: $UPDATER_LINE"
  fi
  echo "- [ ] Layer V: Agent Read 上方 PNG（场景侧栏/壳改动时必勾）"
  echo "- [x] 报告: $REPORT"
  echo "- [ ] STATUS「当前焦点」已更新"
  echo '```'
} >"$REPORT"

echo
echo "======== ACCEPTANCE $VERDICT ========"
cat "$REPORT"
echo "===================================="

if [[ "$VERDICT" != "PASS" ]]; then
  exit 1
fi
exit 0
