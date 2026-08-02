#!/usr/bin/env bash
# Score Stitch chat quality artifacts against e2e/quality/samples.json.
# Default: score existing pre-deploy-review dump (no LLM call).
# Optional: SCORE_RUN=1 also runs pre-deploy-review first.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
E2E="$ROOT/e2e"
SAMPLES="$E2E/quality/samples.json"
REVIEW="$E2E/artifacts/pre-deploy-review/REVIEW.md"
ASSISTANT="$E2E/artifacts/pre-deploy-review/assistant.txt"
TOOLS="$E2E/artifacts/pre-deploy-review/tools.json"

if [[ "${SCORE_RUN:-0}" == "1" ]]; then
  sh "$ROOT/scripts/pre-deploy-review.sh"
fi

if [[ ! -f "$REVIEW" || ! -f "$ASSISTANT" || ! -f "$TOOLS" ]]; then
  echo "error: missing artifacts under e2e/artifacts/pre-deploy-review/" >&2
  echo "run: SCORE_RUN=1 sh scripts/smoke-chat-quality.sh" >&2
  echo "  or: sh scripts/pre-deploy-review.sh" >&2
  exit 1
fi

export E2E_DIR="$E2E"
node <<'NODE'
const fs = require("fs");
const path = require("path");

const e2e = process.env.E2E_DIR;
const review = fs.readFileSync(path.join(e2e, "artifacts/pre-deploy-review/REVIEW.md"), "utf8");
const assistant = fs.readFileSync(path.join(e2e, "artifacts/pre-deploy-review/assistant.txt"), "utf8");
const tools = JSON.parse(fs.readFileSync(path.join(e2e, "artifacts/pre-deploy-review/tools.json"), "utf8"));
const samples = JSON.parse(fs.readFileSync(path.join(e2e, "quality/samples.json"), "utf8"));
const sample = samples.samples.find((s) => s.id === "review-diff");
const must = sample.must;

const names = tools.map((t) => t.name);
const readCount = names.filter((n) => n === "read_file").length;
const checks = [];

function ok(name, pass, detail) {
  checks.push({ name, pass, detail });
}

ok(
  "tools_include_any",
  must.tools_include_any.some((n) => names.includes(n)),
  `tools=${names.join(",")}`,
);

for (const re of must.assistant_regex) {
  ok(`assistant_regex:${re}`, new RegExp(re).test(assistant), `len=${assistant.length}`);
}

ok(
  "max_read_file",
  readCount <= must.max_read_file,
  `read_file_count=${readCount}`,
);

if (must.forbid_unrecovered_missing_path) {
  const unrecovered = tools.some((t, i) => {
    const blob = `${t.headline || ""} ${t.detail || ""}`;
    if (t.name !== "read_file" || !/Missing ['"]path['"]/i.test(blob)) return false;
    return !tools.slice(i + 1).some((later) => later.name === "read_file" && !later.err);
  });
  ok("forbid_unrecovered_missing_path", !unrecovered, unrecovered ? "found" : "none");
}

const failed = checks.filter((c) => !c.pass);
const report = [
  "# Chat quality score — review-diff",
  "",
  `tools: ${names.join(", ")}`,
  `read_file_count: ${readCount}`,
  "",
  ...checks.map((c) => `- [${c.pass ? "x" : " "}] ${c.name} — ${c.detail}`),
  "",
  failed.length === 0 ? "VERDICT: PASS" : `VERDICT: FAIL (${failed.length})`,
  "",
].join("\n");

const outDir = path.join(e2e, "artifacts/chat-quality");
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "SCORE.md"), report, "utf8");
process.stdout.write(report);
process.exit(failed.length === 0 ? 0 : 1);
NODE
