import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import { shot } from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(__dirname, "../artifacts/desktop-smoke");

async function waitBooted() {
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

async function readView() {
  const text = await $('[data-testid="diag-view"]').getText();
  const m = text.match(/view=(\w+)/);
  return m?.[1] ?? "";
}

async function openSettings() {
  if ((await readView()) === "settings") return;
  await $('[data-testid="open-settings"]').click();
  await browser.waitUntil(async () => (await readView()) === "settings", {
    timeout: 10_000,
    timeoutMsg: "open-settings did not switch to settings",
  });
}

async function enterChat() {
  if ((await readView()) === "chat") return;
  const goChat = await $('[data-testid="settings-go-chat"]');
  const backChat = await $('[data-testid="settings-back-chat"]');
  if (await goChat.isExisting()) {
    await goChat.click();
  } else if (await backChat.isExisting()) {
    await backChat.click();
  } else {
    await browser.execute(() => {
      (
        window as unknown as { __stitchShowChat?: (r?: string) => void }
      ).__stitchShowChat?.("wdio-hook");
    });
  }
  await browser.waitUntil(async () => (await readView()) === "chat", {
    timeout: 10_000,
    timeoutMsg: "enter-chat did not reach view=chat — see diag-banner",
  });
  await $('[data-testid="chat-view"]').waitForExist({ timeout: 5_000 });
}

describe("Stitch desktop smoke", () => {
  it("boots and can navigate settings ↔ chat", async () => {
    await waitBooted();
    await openSettings();
    expect(await readView()).toBe("settings");
    await shot(OUT, "01-settings");
    await enterChat();
    await shot(OUT, "02-chat");
    expect(await findVisibleUiLeak()).toBeNull();
  });

  it("workspace tree + account red-dot / sediment target", async () => {
    await waitBooted();
    await enterChat();

    await $('[data-testid="workspace-panel"]').waitForExist({ timeout: 10_000 });
    await $('[data-testid="workspace-list"]').waitForExist({ timeout: 5_000 });
    const groups = await $$('[data-testid="workspace-group"]');
    expect(groups.length).toBeGreaterThan(0);
    await shot(OUT, "10-workspace-tree");

    const gearDot = await $('[data-testid="settings-account-dot"]');
    const hasGearDot = await gearDot.isExisting();

    await openSettings();
    await $('[data-testid="settings-tab-account"]').click();
    await $('[data-testid="settings-sediment-visibility"]').waitForExist({ timeout: 5_000 });
    await $('[data-testid="sediment-vis-explore"]').waitForExist({ timeout: 5_000 });
    await $('[data-testid="sediment-vis-personal"]').waitForExist({ timeout: 5_000 });
    await shot(OUT, "11-account-sediment");

    const navDot = await $('[data-testid="settings-account-dot"]');
    const connectBanner = await $('[data-testid="account-connect-banner"]');
    if (hasGearDot) {
      expect(await navDot.isExisting()).toBe(true);
      expect(await connectBanner.isExisting()).toBe(true);
      expect(
        await $('[data-testid="settings-tab-account"]').getAttribute("class"),
      ).toContain("settings-nav-item-active");
    } else {
      // Token already on this machine: red-dot off; sediment target still present.
      expect(await navDot.isExisting()).toBe(false);
      expect(await connectBanner.isExisting()).toBe(false);
    }

    await enterChat();
    const treeStill = await $('[data-testid="workspace-panel"]');
    expect(await treeStill.isExisting()).toBe(true);
    expect(await findVisibleUiLeak()).toBeNull();
  });
});
