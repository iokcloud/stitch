#!/usr/bin/env bash
# Production updater handoff: compare installed Stitch ProductVersion vs production
# stitch-update.json. Does NOT change production.
# Exit: 0 CURRENT · 2 NEED_INSTALL · 3 BEHIND · 1 error
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST_URL="${STITCH_UPDATE_MANIFEST_URL:-https://www.promptstdio.com/downloads/stitch-update.json}"
OUT_DIR="$ROOT/e2e/artifacts/updater-prod"
mkdir -p "$OUT_DIR"
REPORT="$OUT_DIR/REPORT.txt"

echo "==> Fetch production manifest"
TMP_JSON="$OUT_DIR/stitch-update.json"
if ! curl -fsSL --max-time 20 "$MANIFEST_URL" -o "$TMP_JSON"; then
  echo "FAIL: cannot fetch $MANIFEST_URL" >&2
  exit 1
fi

MANIFEST_VER="$(
  python -c "import json,sys; print(json.load(open(sys.argv[1],encoding='utf-8')).get('version','').strip())" "$TMP_JSON"
)"
if [[ -z "$MANIFEST_VER" ]]; then
  echo "FAIL: manifest missing version" >&2
  exit 1
fi
echo "    manifest version: $MANIFEST_VER"

echo "==> Resolve installed Stitch.exe"
export STITCH_PROD_VERIFY_OUT="$OUT_DIR/installed-path.txt"
rm -f "$STITCH_PROD_VERIFY_OUT"
if command -v powershell.exe >/dev/null 2>&1; then
  powershell.exe -NoProfile -Command '
    $out = $env:STITCH_PROD_VERIFY_OUT
    $paths = New-Object System.Collections.Generic.List[string]
    $paths.Add((Join-Path $env:LOCALAPPDATA "Programs\Stitch\Stitch.exe"))
    $paths.Add((Join-Path $env:ProgramFiles "Stitch\Stitch.exe"))
    $pf86 = ${env:ProgramFiles(x86)}
    if ($pf86) { $paths.Add((Join-Path $pf86 "Stitch\Stitch.exe")) }
    foreach ($p in $paths) {
      if ($p -and (Test-Path -LiteralPath $p)) {
        Set-Content -LiteralPath $out -Value $p -Encoding ascii
        break
      }
    }
  ' >/dev/null 2>&1 || true
fi

INSTALLED_EXE=""
if [[ -f "$STITCH_PROD_VERIFY_OUT" ]]; then
  INSTALLED_EXE="$(tr -d '\r' <"$STITCH_PROD_VERIFY_OUT" | head -n 1)"
fi

if [[ -z "$INSTALLED_EXE" || ! -f "$INSTALLED_EXE" ]]; then
  cat >"$REPORT" <<EOF
VERDICT: NEED_INSTALL
manifest: $MANIFEST_VER
installed: (not found)

Hand steps:
1. Install production NSIS from https://www.promptstdio.com/stitch (or keep an older build).
2. Settings → 通用 → 检查更新 → 发现新版本 → 安装更新 → allow restart.
3. Re-run: sh scripts/smoke-updater-prod-verify.sh
EOF
  echo "NEED_INSTALL: no Stitch.exe under Local/Programs or Program Files."
  echo "Report: $REPORT"
  exit 2
fi

echo "    exe: $INSTALLED_EXE"
export STITCH_PROD_VERIFY_EXE="$INSTALLED_EXE"
export STITCH_PROD_VERIFY_VER="$OUT_DIR/installed-ver.txt"
rm -f "$STITCH_PROD_VERIFY_VER"
powershell.exe -NoProfile -Command '
  $p = $env:STITCH_PROD_VERIFY_EXE
  $out = $env:STITCH_PROD_VERIFY_VER
  $v = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($p)
  $s = $v.ProductVersion
  if ([string]::IsNullOrWhiteSpace($s)) { $s = $v.FileVersion }
  $s = ($s -replace "[^0-9.].*", "").Trim()
  Set-Content -LiteralPath $out -Value $s -Encoding ascii
' >/dev/null

INSTALLED_VER="$(tr -d '\r' <"$STITCH_PROD_VERIFY_VER" | head -n 1)"
if [[ -z "$INSTALLED_VER" ]]; then
  echo "FAIL: cannot read ProductVersion from $INSTALLED_EXE" >&2
  exit 1
fi
echo "    installed version: $INSTALLED_VER"

CMP="$(
  python - "$INSTALLED_VER" "$MANIFEST_VER" <<'PY'
import sys

def parts(s: str):
    out = []
    for x in s.split("."):
        digits = "".join(ch for ch in x if ch.isdigit())
        out.append(int(digits or "0"))
    return out

a, b = parts(sys.argv[1]), parts(sys.argv[2])
n = max(len(a), len(b))
a += [0] * (n - len(a))
b += [0] * (n - len(b))
print("lt" if a < b else "gt" if a > b else "eq")
PY
)"

if [[ "$CMP" == "lt" ]]; then
  cat >"$REPORT" <<EOF
VERDICT: BEHIND
manifest: $MANIFEST_VER
installed: $INSTALLED_VER
exe: $INSTALLED_EXE

Hand steps (restart-after-upgrade):
1. Open installed Stitch → 设置 → 通用 → 检查更新
2. Expect 「发现新版本」→ 安装更新 → allow restart
3. Re-run: sh scripts/smoke-updater-prod-verify.sh
4. Expect VERDICT: CURRENT (installed >= $MANIFEST_VER)
EOF
  echo "BEHIND: installed $INSTALLED_VER < manifest $MANIFEST_VER"
  echo "Report: $REPORT"
  exit 3
fi

cat >"$REPORT" <<EOF
VERDICT: CURRENT
manifest: $MANIFEST_VER
installed: $INSTALLED_VER
exe: $INSTALLED_EXE
EOF
echo "PASS: installed $INSTALLED_VER >= manifest $MANIFEST_VER"
echo "Report: $REPORT"
exit 0
