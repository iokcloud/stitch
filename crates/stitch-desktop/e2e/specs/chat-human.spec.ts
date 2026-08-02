/**
 * Human-like chat smoke against the real desktop binary + real LLM config.
 * Screenshots → e2e/artifacts/chat-human/
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/chat-human");

async function shot(name: string) {
  fs.mkdirSync(outDir, { recursive: true });
  const file = path.join(outDir, `${name}.png`);
  await browser.saveScreenshot(file);
  return file;
}

/** MessageBubble uses .message + "You"/"Stitch" labels — not ARIA roles. */
async function chatStats() {
  return browser.execute(() => {
    const log = document.querySelector('[role="log"]');
    const bubbles = [...(log?.querySelectorAll(".message") ?? [])];
    const users = bubbles.filter((b) => !!b.querySelector(".message-user"));
    const assistants = bubbles.filter(
      (b) => !!b.querySelector(".msg-assistant") || !!b.querySelector(".msg-role"),
    );
    const lastA = assistants.at(-1)?.textContent?.replace(/\s+/g, " ").trim() ?? "";
    const lastU = users.at(-1)?.textContent?.replace(/\s+/g, " ").trim() ?? "";
    return {
      userCount: users.length,
      assistantCount: assistants.length,
      lastUser: lastU.slice(0, 200),
      lastAssistant: lastA.slice(0, 400),
      bodyHasSveltekit: (document.body?.innerText ?? "").includes("__sveltekit_"),
    };
  });
}

async function ensureChat() {
  await browser.waitUntil(
    async () => {
      const settings = await $('[data-testid="settings-view"]');
      const chat = await $('[data-testid="chat-view"]');
      return (await settings.isExisting()) || (await chat.isExisting());
    },
    { timeout: 60_000, timeoutMsg: "neither settings nor chat appeared" },
  );

  if (await $('[data-testid="chat-view"]').isExisting()) {
    return;
  }

  const go = await $('[data-testid="settings-go-chat"]');
  const back = await $('[data-testid="settings-back-chat"]');
  if (await go.isExisting()) {
    await go.click();
  } else if (await back.isExisting()) {
    await back.click();
  } else {
    await browser.execute(() => {
      (window as unknown as { __stitchShowChat?: (r?: string) => void }).__stitchShowChat?.(
        "chat-human",
      );
    });
  }

  await $('[data-testid="chat-view"]').waitForExist({ timeout: 15_000 });
}

async function waitIdle(timeout = 120_000) {
  await browser.waitUntil(
    async () => {
      const stop = await $('[data-testid="chat-send"][aria-label="停止生成"]');
      const send = await $('[data-testid="chat-send"][aria-label="发送"]');
      if (await stop.isExisting()) return false;
      return await send.isExisting();
    },
    { timeout, timeoutMsg: "generation did not finish", interval: 500 },
  );
}

async function sendChat(text: string) {
  const input = await $('[data-testid="chat-input"]');
  await input.waitForExist({ timeout: 10_000 });
  await input.click();
  // Clear then type — setValue alone is flaky on WebView2 textareas
  await browser.execute((value) => {
    const el = document.querySelector('[data-testid="chat-input"]') as HTMLTextAreaElement | null;
    if (!el) return;
    el.focus();
    el.value = value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
  await browser.pause(200);
  const before = await chatStats();
  await $('[data-testid="chat-send"][aria-label="发送"]').click();
  await browser.waitUntil(
    async () => (await $('[data-testid="chat-send"][aria-label="停止生成"]').isExisting()),
    {
      timeout: 20_000,
      timeoutMsg: "send did not start streaming",
    },
  );
  await waitIdle();
  await browser.waitUntil(
    async () => {
      const after = await chatStats();
      return after.assistantCount > before.assistantCount || after.lastAssistant !== before.lastAssistant;
    },
    { timeout: 30_000, timeoutMsg: "no new assistant bubble after send" },
  );
}

describe("Stitch chat human walkthrough", () => {
  it("boots, single-turn, multi-turn, plan toggle", async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });

    await browser.waitUntil(
      async () => (await browser.getTitle()).toLowerCase().includes("stitch"),
      { timeout: 30_000, timeoutMsg: "title missing Stitch" },
    );

    await browser.waitUntil(async () => !(await $("#app-loader").isExisting()), {
      timeout: 20_000,
      timeoutMsg: "loader stuck",
    });

    await ensureChat();
    // Fresh session so counts are deterministic
    const newChat = await $('button[aria-label="新建会话"]');
    if (await newChat.isExisting()) {
      await newChat.click();
      await browser.pause(300);
    }
    await shot("01-chat-ready");

    const leak0 = await findVisibleUiLeak();
    expect(leak0).toBeNull();

    await sendChat("用一句话介绍你自己，不要使用表情符号。");
    await shot("02-after-single-turn");

    const after1 = await chatStats();
    expect(after1.bodyHasSveltekit).toBe(false);
    expect(after1.userCount).toBeGreaterThanOrEqual(1);
    expect(after1.assistantCount).toBeGreaterThanOrEqual(1);
    expect(after1.lastAssistant.length).toBeGreaterThan(4);
    expect(after1.lastUser).toMatch(/介绍你自己/);

    await sendChat("请记住暗号：blue7。只回复三个字：已记住。");
    await shot("03-after-codeword-set");
    await sendChat("我的暗号是什么？只回答暗号本身，不要解释。");
    await shot("04-after-codeword-ask");

    const afterMt = await chatStats();
    expect(afterMt.lastAssistant.toLowerCase()).toContain("blue7");

    const plan = await $('[data-testid="plan-mode-toggle"]');
    const checkedBefore = await plan.isSelected().catch(async () => {
      return browser.execute(() => {
        const el = document.querySelector(
          '[data-testid="plan-mode-toggle"]',
        ) as HTMLInputElement | null;
        return !!el?.checked;
      });
    });
    await plan.click();
    await browser.pause(200);
    await shot("05-plan-mode-on");
    const placeholderOn = await $('[data-testid="chat-input"]').getAttribute("placeholder");
    expect(placeholderOn || "").toMatch(/计划/);
    if (!checkedBefore) {
      await plan.click();
      await browser.pause(200);
    }
    await shot("06-done");

    const leak1 = await findVisibleUiLeak();
    expect(leak1).toBeNull();

    const report = [
      "Stitch chat human walkthrough — PASS",
      `single-turn user: ${after1.lastUser}`,
      `single-turn assistant: ${after1.lastAssistant}`,
      `multi-turn last: ${afterMt.lastAssistant}`,
      "hygiene: clean (no __sveltekit_)",
      `artifacts: ${outDir}`,
    ].join("\n");
    fs.writeFileSync(path.join(outDir, "REPORT.txt"), report, "utf8");
  });
});
