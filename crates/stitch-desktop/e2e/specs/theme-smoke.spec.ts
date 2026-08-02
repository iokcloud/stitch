/**
 * Optional theme regression — NOT part of default Layer B (`npm run smoke`).
 * Run only when explicitly requested: `STITCH_THEME_SMOKE=1 npm run theme-smoke`
 * Do not run against a user's daily Stitch preference without isolation.
 */
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import { shot } from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(__dirname, "../artifacts/theme-smoke");

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

async function dataTheme() {
  return browser.execute(() => document.documentElement.getAttribute("data-theme"));
}

async function storedTheme() {
  return browser.execute(() => localStorage.getItem("stitch-theme"));
}

async function themeOptionMeta(testid: string) {
  return browser.execute((id) => {
    const el = document.querySelector(`[data-testid="${id}"]`);
    if (!el) return null;
    return {
      text: (el.textContent || "").replace(/\s+/g, " ").trim(),
      selected: el.getAttribute("aria-selected") === "true",
      hasSvg: !!el.querySelector("svg"),
    };
  }, testid);
}

async function pickTheme(
  testid: string,
  expectStored: "light" | "dark" | "system",
  expectResolved?: "light" | "dark",
) {
  await $(`[data-testid="${testid}"]`).click();
  await browser.waitUntil(async () => (await storedTheme()) === expectStored, {
    timeout: 5_000,
    timeoutMsg: `${testid} did not persist stitch-theme=${expectStored}`,
  });
  if (expectResolved) {
    await browser.waitUntil(async () => (await dataTheme()) === expectResolved, {
      timeout: 5_000,
      timeoutMsg: `${testid} did not set data-theme=${expectResolved}`,
    });
  }
  expect(await $(`[data-testid="${testid}"]`).getAttribute("aria-selected")).toBe("true");
}

describe("Stitch theme smoke (optional)", () => {
  before(function () {
    if (process.env.STITCH_THEME_SMOKE !== "1") {
      this.skip();
    }
  });

  it("general settings theme: click 浅色 / 深色 / 跟随系统 + header cycle", async () => {
    await waitBooted();
    await openSettings();

    await $('[data-testid="settings-tab-system"]').click();
    await $('[data-testid="settings-theme"]').waitForExist({ timeout: 5_000 });

    const light = await themeOptionMeta("settings-theme-light");
    const dark = await themeOptionMeta("settings-theme-dark");
    const system = await themeOptionMeta("settings-theme-system");
    expect(light?.text).toContain("浅色");
    expect(dark?.text).toContain("深色");
    expect(system?.text).toContain("跟随系统");
    expect(light?.hasSvg).toBe(true);
    expect(dark?.hasSvg).toBe(true);
    expect(system?.hasSvg).toBe(true);

    await pickTheme("settings-theme-dark", "dark", "dark");
    await shot(OUT, "03-theme-dark");

    await pickTheme("settings-theme-light", "light", "light");
    await shot(OUT, "04-theme-light");

    await pickTheme("settings-theme-system", "system");
    const resolved = await dataTheme();
    expect(resolved === "light" || resolved === "dark").toBe(true);
    await shot(OUT, "05-theme-system");

    await pickTheme("settings-theme-dark", "dark", "dark");
    await pickTheme("settings-theme-system", "system");
    await pickTheme("settings-theme-light", "light", "light");

    await enterChat();
    const toggle = await $('[data-testid="toggle-theme"]');
    expect(await toggle.getAttribute("data-theme-pref")).toBe("light");

    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute("data-theme-pref")) === "dark",
      { timeout: 5_000, timeoutMsg: "header toggle did not reach dark" },
    );
    expect(await dataTheme()).toBe("dark");
    await shot(OUT, "06-header-dark");

    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute("data-theme-pref")) === "system",
      { timeout: 5_000, timeoutMsg: "header toggle did not reach system" },
    );
    await shot(OUT, "07-header-system");

    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute("data-theme-pref")) === "light",
      { timeout: 5_000, timeoutMsg: "header toggle did not reach light" },
    );
    expect(await dataTheme()).toBe("light");
    await shot(OUT, "08-header-light");

    await openSettings();
    await $('[data-testid="settings-tab-system"]').click();
    expect(await $('[data-testid="settings-theme-light"]').getAttribute("aria-selected")).toBe(
      "true",
    );
    await shot(OUT, "09-settings-synced");

    expect(await findVisibleUiLeak()).toBeNull();
  });
});
