/**
 * Shared WDIO helpers for real-exe chat walkthroughs.
 */
import fs from "node:fs";
import path from "node:path";

export async function shot(outDir: string, name: string) {
  fs.mkdirSync(outDir, { recursive: true });
  const file = path.join(outDir, `${name}.png`);
  await browser.saveScreenshot(file);
  return file;
}

export async function chatStats() {
  return browser.execute(() => {
    const log = document.querySelector('[role="log"]');
    const bubbles = [...(log?.querySelectorAll(".message") ?? [])];
    const users = bubbles.filter((b) => !!b.querySelector(".message-user"));
    const assistants = bubbles.filter(
      (b) => !!b.querySelector(".msg-assistant") || !!b.querySelector(".msg-role"),
    );
    const lastA = assistants.at(-1)?.textContent?.replace(/\s+/g, " ").trim() ?? "";
    const lastU = users.at(-1)?.textContent?.replace(/\s+/g, " ").trim() ?? "";
    const logText = (log?.textContent || "").replace(/\s+/g, " ").trim();
    return {
      userCount: users.length,
      assistantCount: assistants.length,
      lastUser: lastU.slice(0, 200),
      lastAssistant: lastA.slice(0, 400),
      logText: logText.slice(0, 1200),
      hasStopped: logText.includes("已停止生成"),
      bodyHasSveltekit: (document.body?.innerText ?? "").includes("__sveltekit_"),
    };
  });
}

export async function ensureChat() {
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
        "chat-core-human",
      );
    });
  }

  await $('[data-testid="chat-view"]').waitForExist({ timeout: 15_000 });
}

/**
 * 完整启动等待（含 boot-error 表面检查）——各 spec 的本地 waitBooted
 * 副本统一收敛到此处（审查发现 8 处复制且已发散）。
 */
export async function waitBooted() {
  await browser.waitUntil(
    async () => {
      const title = await browser.getTitle();
      return title.toLowerCase().includes("stitch");
    },
    {
      timeout: 30_000,
      timeoutMsg: "window title never contained Stitch (wrong driver/port?)",
      interval: 500,
    },
  );
  await browser.waitUntil(
    async () => {
      const settings = await $('[data-testid="settings-view"]');
      const chat = await $('[data-testid="chat-view"]');
      const bootError = await $('[data-testid="boot-error"]');
      return (
        (await settings.isExisting()) ||
        (await chat.isExisting()) ||
        (await bootError.isExisting())
      );
    },
    {
      timeout: 60_000,
      timeoutMsg: "neither settings, chat, nor boot-error appeared after launch",
      interval: 500,
    },
  );
  const bootError = await $('[data-testid="boot-error"]');
  if (await bootError.isExisting()) {
    const text = await bootError.getText().catch(() => "");
    throw new Error(`app reached boot-error surface: ${text}`);
  }
  await browser.waitUntil(async () => !(await $("#app-loader").isExisting()), {
    timeout: 20_000,
    timeoutMsg: "#app-loader still present after main view mounted",
  });
  await $('[data-testid="diag-view"]').waitForExist({ timeout: 10_000 });
}

export async function bootChat() {
  await waitBooted();
  await ensureChat();
}

export async function waitIdle(timeout = 180_000) {
  await browser.waitUntil(
    async () => {
      const allow = await $('[data-testid="confirm-allow"]');
      if (await allow.isExisting()) {
        // Prefer session-allow so multi-step coding tasks do not remount the dialog.
        const session = await $('[data-testid="confirm-session-allow"]');
        if (await session.isExisting()) {
          const on = await session.isSelected();
          if (!on) await session.click();
        }
        await allow.click();
        return false;
      }
      // Scope to the composer button: the compact bar keeps its own
      // aria-label="停止生成" button in the DOM at all times.
      const stop = await $('[data-testid="chat-send"][aria-label="停止生成"]');
      const send = await $('[data-testid="chat-send"][aria-label="发送"]');
      if (await stop.isExisting()) return false;
      return await send.isExisting();
    },
    { timeout, timeoutMsg: "generation did not finish", interval: 400 },
  );
}

export async function fillChat(text: string) {
  const input = await $('[data-testid="chat-input"]');
  await input.waitForExist({ timeout: 10_000 });
  await input.click();
  await browser.execute((value) => {
    const el = document.querySelector('[data-testid="chat-input"]') as HTMLTextAreaElement | null;
    if (!el) return;
    el.focus();
    // Native setter so Svelte bind:value + oninput both see the change.
    const desc = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value");
    desc?.set?.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
  await browser.pause(200);
}

export async function clickSend() {
  await $('[data-testid="chat-send"][aria-label="发送"]').click();
  await browser.waitUntil(
    async () => (await $('[data-testid="chat-send"][aria-label="停止生成"]').isExisting()),
    {
      timeout: 20_000,
      timeoutMsg: "send did not start streaming",
    },
  );
}

/** Fill + send + wait until idle + new assistant content. */
export async function sendChat(text: string, idleTimeout = 180_000) {
  const before = await chatStats();
  await fillChat(text);
  await clickSend();
  await waitIdle(idleTimeout);
  await browser.waitUntil(
    async () => {
      const after = await chatStats();
      return (
        after.assistantCount > before.assistantCount || after.lastAssistant !== before.lastAssistant
      );
    },
    { timeout: 30_000, timeoutMsg: "no new assistant bubble after send" },
  );
}

export async function setPlanMode(on: boolean) {
  const checked = await browser.execute(() => {
    const el = document.querySelector(
      '[data-testid="plan-mode-toggle"]',
    ) as HTMLInputElement | null;
    return !!el?.checked;
  });
  if (checked !== on) {
    await $('[data-testid="plan-mode-toggle"]').click();
    await browser.pause(200);
  }
}

export async function newSession() {
  await $('[data-testid="session-new"]').click();
  await browser.pause(250);
}

export async function activeSessionId(): Promise<string | null> {
  return browser.execute(() => {
    const el = document.querySelector(
      '[data-testid="session-row"][data-active="true"]',
    ) as HTMLElement | null;
    return el?.getAttribute("data-session-id") ?? null;
  });
}

export async function switchToSession(id: string) {
  const row = await $(`[data-testid="session-row"][data-session-id="${id}"]`);
  await row.waitForExist({ timeout: 5_000 });
  await row.click();
  await browser.pause(250);
}

export async function planPhase(): Promise<string> {
  return browser.execute(() => {
    const card = document.querySelector('[data-testid="plan-card"]');
    if (!card) return "";
    const phase = card.querySelector(".plan-card-phase");
    return (phase?.textContent || "").trim();
  });
}

export async function setWorkDir(dir: string) {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () =>
          typeof (window as unknown as { __stitchSetWorkDir?: unknown }).__stitchSetWorkDir ===
          "function",
      ),
    { timeout: 15_000, timeoutMsg: "__stitchSetWorkDir hook missing" },
  );
  const result = await browser.execute(async (target) => {
    const w = window as unknown as {
      __stitchSetWorkDir?: (path: string) => Promise<string>;
      __stitchGetWorkDir?: () => string;
    };
    if (!w.__stitchSetWorkDir) return { ok: false, error: "no hook" };
    try {
      const applied = await w.__stitchSetWorkDir(target);
      const shown = w.__stitchGetWorkDir?.() ?? "";
      return { ok: true, applied, shown };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  }, dir);
  if (!result || !(result as { ok: boolean }).ok) {
    throw new Error(`set_work_dir failed: ${JSON.stringify(result)}`);
  }
  return result as { ok: true; applied: string; shown: string };
}

export async function composerHeight(): Promise<number> {
  return browser.execute(() => {
    const el = document.querySelector('[data-testid="chat-input"]') as HTMLTextAreaElement | null;
    return el?.getBoundingClientRect().height ?? 0;
  });
}

/** ToolGroup defaults collapsed — expand so tool-status nodes exist for dumps. */
export async function expandToolGroups(): Promise<void> {
  await browser.execute(() => {
    for (const btn of document.querySelectorAll(
      '[data-testid="tool-group-toggle"][aria-expanded="false"]',
    )) {
      (btn as HTMLButtonElement).click();
    }
  });
  await browser.pause(150);
}

/** Assert chat chrome has no emoji / checkmark glyphs in tool/plan UI. */
export async function assertNoGlyphIcons() {
  await expandToolGroups();
  const hit = await browser.execute(() => {
    const roots = [
      ...document.querySelectorAll('[data-testid="tool-status"]'),
      ...document.querySelectorAll('[data-testid="plan-card"]'),
      ...document.querySelectorAll('[data-testid="stream-rail"]'),
    ];
    const banned = /[✓✔✅❌📁📄■□▶]/;
    for (const el of roots) {
      const t = el.textContent || "";
      if (banned.test(t)) return t.slice(0, 200);
    }
    return null;
  });
  expect(hit).toBeNull();
}

export async function uiSnapshot() {
  await expandToolGroups();
  return browser.execute(() => {
    const log = document.querySelector('[role="log"]');
    const tools = [...document.querySelectorAll('[data-testid="tool-status"]')].map((el) =>
      (el.textContent || "").replace(/\s+/g, " ").trim().slice(0, 160),
    );
    const expandBtns = [...(log?.querySelectorAll(".message-expand") ?? [])].map(
      (b) => (b.getAttribute("aria-label") || b.textContent || "").trim(),
    );
    const lastAssistant =
      [...(log?.querySelectorAll(".msg-assistant") ?? [])]
        .at(-1)
        ?.textContent?.replace(/\s+/g, " ")
        .trim()
        .slice(0, 800) ?? "";
    const lastUserHtml =
      [...(log?.querySelectorAll(".message-user") ?? [])].at(-1)?.innerHTML ?? "";
    return {
      tools,
      expandBtns,
      lastAssistant,
      lastUserHasBr: /<br\s*\/?>/i.test(lastUserHtml) || lastUserHtml.includes("\n"),
      lastUserText: (
        [...(log?.querySelectorAll(".message-user") ?? [])].at(-1)?.textContent || ""
      )
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, 400),
      toolCallCount: document.querySelectorAll('[data-testid="tool-status"]').length,
      planCount: document.querySelectorAll('[data-testid="plan-card"]').length,
    };
  });
}

/**
 * 清 WebView2 持久化的 Stitch localStorage 键（S-016——跨 session 持久，
 * 脏值会让真机测试吃到错误的侧栏 tab/主题/库子页）。spec 的 before 里调用。
 */
export async function clearStitchStorage(): Promise<void> {
  await browser.execute(() => {
    const keys = [
      "stitch-sidebar-tab",
      "stitch-library-kind",
      "stitch-theme",
    ];
    for (const k of keys) localStorage.removeItem(k);
  });
}
