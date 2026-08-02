/**
 * Real-exe walkthrough: workspace tree fold + account red-dot / sediment target.
 * Run: cd e2e && npm run smoke -- --spec ./specs/workspace-reddot.spec.ts
 * Or via: sh scripts/smoke-desktop.sh --spec ./specs/workspace-reddot.spec.ts
 */
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import { shot } from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(__dirname, "../artifacts/workspace-reddot");

async function waitBooted() {
  await browser.waitUntil(
    async () => (await browser.getTitle()).toLowerCase().includes("stitch"),
    { timeout: 30_000, timeoutMsg: "title missing Stitch" },
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
    { timeout: 60_000, timeoutMsg: "no main surface after launch" },
  );
  if (await $('[data-testid="boot-error"]').isExisting()) {
    throw new Error(`boot-error: ${await $('[data-testid="boot-error"]').getText()}`);
  }
  await browser.waitUntil(async () => !(await $("#app-loader").isExisting()), {
    timeout: 20_000,
    timeoutMsg: "loader stuck",
  });
  await $('[data-testid="diag-view"]').waitForExist({ timeout: 10_000 });
}

async function readView() {
  const text = await $('[data-testid="diag-view"]').getText();
  const m = text.match(/view=(\w+)/);
  return m?.[1] ?? "";
}

async function enterChat() {
  if ((await readView()) === "chat") return;
  const goChat = await $('[data-testid="settings-go-chat"]');
  const backChat = await $('[data-testid="settings-back-chat"]');
  if (await goChat.isExisting()) await goChat.click();
  else if (await backChat.isExisting()) await backChat.click();
  else {
    await browser.execute(() => {
      (window as unknown as { __stitchShowChat?: (r?: string) => void }).__stitchShowChat?.(
        "workspace-reddot",
      );
    });
  }
  await browser.waitUntil(async () => (await readView()) === "chat", {
    timeout: 10_000,
    timeoutMsg: "enter-chat failed",
  });
  await $('[data-testid="chat-view"]').waitForExist({ timeout: 5_000 });
}

async function openSettings() {
  if ((await readView()) === "settings") return;
  await $('[data-testid="open-settings"]').click();
  await browser.waitUntil(async () => (await readView()) === "settings", {
    timeout: 10_000,
    timeoutMsg: "open-settings failed",
  });
}

describe("workspace tree + red-dot real exe", () => {
  it("folds workspace sessions and shows account red-dot / sediment target", async () => {
    await waitBooted();
    await enterChat();

    // --- Workspace tree ---
    await $('[data-testid="workspace-panel"]').waitForExist({ timeout: 10_000 });
    const group = await $('[data-testid="workspace-group"]');
    await group.waitForExist({ timeout: 5_000 });
    await shot(OUT, "01-tree-open");

    // Current group should start expanded (or expand via chevron)
    let expanded = await group.getAttribute("data-expanded");
    if (expanded !== "1") {
      await $('[data-testid="workspace-collapse"]').click();
      await browser.waitUntil(async () => (await group.getAttribute("data-expanded")) === "1", {
        timeout: 3_000,
        timeoutMsg: "could not expand workspace group",
      });
    }
    await $('[data-testid="session-row"]').waitForExist({ timeout: 5_000 });
    await shot(OUT, "02-sessions-under-workspace");

    await $('[data-testid="workspace-collapse"]').click();
    await browser.waitUntil(async () => (await group.getAttribute("data-expanded")) === "0", {
      timeout: 3_000,
      timeoutMsg: "collapse did not hide sessions",
    });
    expect(await group.$$('[data-testid="session-row"]')).toHaveLength(0);
    await shot(OUT, "03-collapsed");

    await $('[data-testid="workspace-collapse"]').click();
    await browser.waitUntil(async () => (await group.getAttribute("data-expanded")) === "1", {
      timeout: 3_000,
      timeoutMsg: "re-expand failed",
    });
    await $('[data-testid="session-row"]').waitForExist({ timeout: 5_000 });
    await shot(OUT, "04-reexpanded");

    // --- Red-dot / sediment ---
    // Delivery goal: when Token is on this machine, red-dots must be OFF.
    // (If Token missing, still allow the nag path — but prefer connected.)
    const gearDot = await $('[data-testid="settings-account-dot"]');
    const hasGearDot = await gearDot.isExisting();
    await shot(OUT, hasGearDot ? "05-chat-gear-dot-on" : "05-chat-gear-dot-off");

    await openSettings();
    await $('[data-testid="settings-tab-account"]').click();
    await $('[data-testid="settings-sediment-visibility"]').waitForExist({ timeout: 5_000 });
    await $('[data-testid="sediment-vis-explore"]').waitForExist({ timeout: 5_000 });
    await $('[data-testid="sediment-vis-personal"]').waitForExist({ timeout: 5_000 });
    await shot(OUT, "06-account-sediment");

    const navDot = await $('[data-testid="settings-account-dot"]');
    const banner = await $('[data-testid="account-connect-banner"]');
    const chipMeta = await browser.execute(() => {
      const chips = [...document.querySelectorAll('[data-testid^="settings-mcp-"]')];
      return chips.map((c) => (c.textContent || "").replace(/\s+/g, " ").trim()).join(" | ");
    });
    const looksConnected =
      /已保存|可用|已失效/.test(chipMeta) ||
      (await $('[data-testid="api-token-stored"]').isExisting());

    if (looksConnected) {
      expect(hasGearDot).toBe(false);
      expect(await navDot.isExisting()).toBe(false);
      expect(await banner.isExisting()).toBe(false);
      await shot(OUT, "07-connected-no-dot");
    } else if (hasGearDot) {
      expect(await navDot.isExisting()).toBe(true);
      expect(await banner.isExisting()).toBe(true);
      expect(await $('[data-testid="account-connect-web"]').isExisting()).toBe(true);
      await shot(OUT, "07-reddot-connect-banner");
    } else {
      await shot(OUT, "07-connected-no-dot");
    }

    await enterChat();
    expect(await $('[data-testid="workspace-panel"]').isExisting()).toBe(true);
    expect(await findVisibleUiLeak()).toBeNull();
    await shot(OUT, "08-back-chat");
  });
});
