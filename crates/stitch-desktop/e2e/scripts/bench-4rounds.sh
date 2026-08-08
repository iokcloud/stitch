#!/usr/bin/env bash
# 桌面自动化 benchmark 多轮复跑：每轮存档 REPORT.md + 控制台日志，最后汇总通过率。
# 用法: bash e2e/scripts/bench-4rounds.sh [轮数]   （默认 4，从 1 开始）
set -euo pipefail
cd "$(dirname "$0")/.."
ROUNDS="${1:-4}"
START="${2:-1}"
OUT="artifacts/desktop-benchmark"
# 注意：spec 的 before() 会整删 $OUT——存档必须放兄弟目录，否则下一轮开跑即被清掉
ARCHIVE="artifacts/desktop-benchmark-archive"
SUMMARY="$ARCHIVE/AGGREGATE.md"
mkdir -p "$ARCHIVE"

for i in $(seq "$START" "$ROUNDS"); do
  echo "=== round $i/$ROUNDS start $(date +%H:%M:%S) ==="
  npm run benchmark-desktop > "artifacts/desktop-benchmark-round$i.log" 2>&1 || {
    echo "round $i FAILED (exit $?)"; tail -20 "artifacts/desktop-benchmark-round$i.log"; continue; }
  cp "$OUT/REPORT.md" "$ARCHIVE/REPORT-round$i.md"
  echo "=== round $i/$ROUNDS done $(date +%H:%M:%S) ==="
done

# 汇总
echo "# 多轮汇总" > "$SUMMARY"
echo "" >> "$SUMMARY"
for i in $(seq 1 "$ROUNDS"); do
  f="$ARCHIVE/REPORT-round$i.md"
  [ -f "$f" ] || { echo "| round$i | 未跑成 | - | - |" >> "$SUMMARY"; continue; }
  row=$(grep '^| T' "$f" | sed 's/^/| /')
  rate=$(grep '^\- 通过率' "$f")
  echo "## Round $i  $rate" >> "$SUMMARY"
  echo "" >> "$SUMMARY"
  echo "| 任务 | 结果 | 耗时(s) | 失败原因 |" >> "$SUMMARY"
  echo "|---|---|---|---|" >> "$SUMMARY"
  echo "$row" >> "$SUMMARY"
  echo "" >> "$SUMMARY"
done
echo "汇总: $SUMMARY"
