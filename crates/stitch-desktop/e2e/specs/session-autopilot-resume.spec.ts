/**
 * Autopilot S1: persist after tool turn → drop memory → follow-up restores from disk.
 * Artifacts → e2e/artifacts/session-autopilot-resume/
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import {
  activeSessionId,
  bootChat,
  chatStats,
  clickSend,
  fillChat,
  newSession,
  setPlanMode,
  setWorkDir,
  shot,
  waitIdle,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/session-autopilot-resume");
const MARKER = "autopilot-resume-ok";

async function dropAgentMemory(id: string) {
  await browser.execute(async (sid) => {
    const w = window as unknown as {
      __stitchDropAgentMemory?: (id: string) => Promise<void>;
    };
    if (!w.__stitchDropAgentMemory) throw new Error("no __stitchDropAgentMemory hook");
    await w.__stitchDropAgentMemory(sid);
  }, id);
}

describe("Session Autopilot resume (disk authority)", () => {
  it("persists tool turn then restores after memory drop", async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });

    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-autopilot-"));
    fs.writeFileSync(path.join(outDir, "sandbox-path.txt"), sandbox, "utf8");
    fs.writeFileSync(path.join(sandbox, "probe.txt"), MARKER, "utf8");

    await bootChat();
    await setPlanMode(false);
    await newSession();
    await setWorkDir(sandbox);
    await browser.pause(400);

    const sid = await activeSessionId();
    expect(sid).toBeTruthy();
    fs.writeFileSync(path.join(outDir, "session-id.txt"), sid!, "utf8");

    await fillChat(
      `用 list_directory 列出当前工作目录（.），在回复里原样包含标记 ${MARKER}`,
    );
    await clickSend();
    await waitIdle(180_000);
    await shot(outDir, "01-after-tool");

    const sessionDir = path.join(sandbox, ".stitch", "sessions", sid!);
    const jsonl = path.join(sessionDir, "messages.jsonl");
    const manifest = path.join(sessionDir, "manifest.json");
    expect(fs.existsSync(jsonl)).toBe(true);
    expect(fs.existsSync(manifest)).toBe(true);
    const man = JSON.parse(fs.readFileSync(manifest, "utf8")) as {
      session_id: string;
      msg_count: number;
    };
    expect(man.session_id).toBe(sid);
    expect(man.msg_count).toBeGreaterThan(1);

    await dropAgentMemory(sid!);
    await browser.pause(200);

    await fillChat(`刚才列表里有没有 probe.txt？只需简短回答，并再次写出 ${MARKER}`);
    await clickSend();
    await waitIdle(180_000);
    await shot(outDir, "02-after-resume");

    const stats = await chatStats();
    expect(stats.hasStopped).toBe(false);
    expect(stats.logText.toLowerCase()).not.toContain("api error 400");
    expect(stats.logText.toLowerCase()).not.toContain("400");
    expect(stats.lastAssistant.includes(MARKER) || stats.logText.includes(MARKER)).toBe(
      true,
    );
    expect(await findVisibleUiLeak()).toBeNull();

    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      [
        `session=${sid}`,
        `jsonl=${jsonl}`,
        `msg_count=${man.msg_count}`,
        `lastAssistant=${stats.lastAssistant.slice(0, 300)}`,
        "PASS",
      ].join("\n"),
      "utf8",
    );
  });
});
