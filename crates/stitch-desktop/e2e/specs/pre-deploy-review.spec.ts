/**
 * Pre-deploy gate: open debug stitch-desktop.exe, send「审查本次改动」,
 * dump full Agent conclusion for Cursor Agent to act on.
 * Artifacts → e2e/artifacts/pre-deploy-review/
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import {
  bootChat,
  newSession,
  sendChat,
  setPlanMode,
  setWorkDir,
  shot,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/pre-deploy-review");
const repoRoot = path.resolve(__dirname, "../../../../../");

/** ToolGroup defaults collapsed — expand so tool-status nodes exist for dump. */
async function expandToolGroups(): Promise<void> {
  await browser.execute(() => {
    for (const btn of document.querySelectorAll(
      '[data-testid="tool-group-toggle"][aria-expanded="false"]',
    )) {
      (btn as HTMLButtonElement).click();
    }
  });
  await browser.pause(150);
}

async function dumpChat(): Promise<{
  lastAssistant: string;
  logText: string;
  tools: { name: string; headline: string; detail: string; err: boolean }[];
  api400: string | null;
}> {
  await expandToolGroups();
  return browser.execute(() => {
    const log = document.querySelector('[role="log"]');
    const assistants = [...(log?.querySelectorAll(".msg-assistant") ?? [])];
    const lastAssistant = assistants.at(-1)?.textContent?.trim() ?? "";
    const logText = (log?.textContent || "").trim();
    const tools = [...document.querySelectorAll('[data-testid="tool-status"]')].map((el) => {
      const name = el.getAttribute("data-tool") || "";
      // Headline: single-line for assertions; keep first line only (do not mash all whitespace).
      const rawHead =
        el.querySelector(".tool-call-headline")?.textContent?.trim() || "";
      const headline = (rawHead.split(/\r?\n/)[0] || "").slice(0, 240);
      // Detail: preserve newlines for REVIEW.md readability (cap length).
      const rawDetail =
        el.querySelector(".tool-call-detail, .tool-listing, .tool-shell-body")?.textContent || "";
      const detail = rawDetail.replace(/\r\n/g, "\n").trim().slice(0, 1200);
      return {
        name,
        headline,
        detail,
        err: el.classList.contains("is-error"),
      };
    });
    let api400: string | null = null;
    if (/API error 400/i.test(logText)) {
      const m = logText.match(/API error 400[\s\S]{0,240}/i);
      api400 = m?.[0]?.replace(/\s+/g, " ").trim() ?? "API error 400";
    }
    return { lastAssistant, logText, tools, api400 };
  });
}

describe("Stitch pre-deploy review (debug exe)", function () {
  this.timeout(720_000);

  before(async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
  });

  it("debug exe chat: 审查本次改动", async () => {
    await setPlanMode(false);
    await newSession();

    const set = await setWorkDir(repoRoot);
    expect(set.applied.toLowerCase()).toMatch(/promptstdio/);

    await sendChat("审查本次改动", 600_000);
    await shot(outDir, "01-审查本次改动");

    const dump = await dumpChat();
    expect(dump.api400).toBeNull();
    expect(dump.lastAssistant.length).toBeGreaterThan(80);
    expect(await findVisibleUiLeak()).toBeNull();

    const toolNames = dump.tools.map((t) => t.name);
    expect(toolNames).toContain("git_status");
    expect(toolNames.some((n) => n === "git_diff" || n === "run_command")).toBe(true);

    // Unrecovered missing-path reads are a harness failure (quality bar).
    const unrecoveredMissingPath = dump.tools.some(
      (t, i) =>
        t.name === "read_file" &&
        /Missing ['"]path['"]/i.test(`${t.headline} ${t.detail}`) &&
        !dump.tools.slice(i + 1).some((later) => later.name === "read_file" && !later.err),
    );
    expect(unrecoveredMissingPath).toBe(false);

    // Prefer evidence from git over STATUS tourism.
    const readCount = toolNames.filter((n) => n === "read_file").length;
    expect(readCount).toBeLessThan(30);

    const conclusion = dump.lastAssistant;
    expect(/结论/.test(conclusion)).toBe(true);
    expect(/阻塞/.test(conclusion)).toBe(true);
    // S-012: SSE chunk-split must not inject U+FFFD into Chinese replies.
    expect(conclusion.includes("\uFFFD")).toBe(false);
    expect(dump.logText.includes("\uFFFD")).toBe(false);

    const report = [
      "# Pre-deploy review (debug exe)",
      "",
      `workdir: ${set.applied}`,
      `prompt: 审查本次改动`,
      `tools_count: ${dump.tools.length}`,
      `tools: ${toolNames.join(", ")}`,
      `read_file_count: ${readCount}`,
      "",
      "## Tool calls",
      ...dump.tools.map(
        (t, i) =>
          `### ${i + 1}. ${t.name}${t.err ? " (error)" : ""}\n${t.headline}\n${t.detail}\n`,
      ),
      "",
      "## Assistant conclusion (full)",
      "",
      dump.lastAssistant,
      "",
    ].join("\n");

    fs.writeFileSync(path.join(outDir, "REVIEW.md"), report, "utf8");
    fs.writeFileSync(path.join(outDir, "assistant.txt"), dump.lastAssistant, "utf8");
    fs.writeFileSync(
      path.join(outDir, "tools.json"),
      JSON.stringify(dump.tools, null, 2),
      "utf8",
    );
  });
});
