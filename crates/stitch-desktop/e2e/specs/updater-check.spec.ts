/**
 * Real updater check against production stitch-update.json.
 * Expects tauri.conf pubkey configured and host manifest version == app version.
 */
import { findVisibleUiLeak } from "../helpers/ui-hygiene";

describe("Stitch updater (production endpoint)", () => {
  it("settings → 通用 → 检查更新 → 已是最新版本", async () => {
    await browser.waitUntil(
      async () => {
        const title = await browser.getTitle();
        return title.toLowerCase().includes("stitch");
      },
      { timeout: 30_000, interval: 500 },
    );

    await browser.waitUntil(
      async () => {
        const settings = await $('[data-testid="settings-view"]');
        const chat = await $('[data-testid="chat-view"]');
        return (await settings.isExisting()) || (await chat.isExisting());
      },
      { timeout: 60_000, interval: 500 },
    );

    await browser.waitUntil(async () => !(await $("#app-loader").isExisting()), {
      timeout: 20_000,
    });

    const readView = async () => {
      const text = await $('[data-testid="diag-view"]').getText();
      const m = text.match(/view=(\w+)/);
      return m?.[1] ?? "";
    };

    if ((await readView()) === "chat") {
      await $('[data-testid="open-settings"]').click();
      await browser.waitUntil(async () => (await readView()) === "settings", {
        timeout: 10_000,
      });
    }

    await $('[data-testid="settings-tab-system"]').click();
    await $('[data-testid="settings-system"]').waitForExist({ timeout: 5_000 });
    await $('[data-testid="check-update"]').click();

    await browser.waitUntil(
      async () => {
        const el = await $('[data-testid="settings-footer-status"]');
        if (!(await el.isExisting())) return false;
        const t = await el.getText();
        return t.includes("已是最新版本") || t.includes("发现新版本") || t.includes("检查更新失败");
      },
      {
        timeout: 45_000,
        timeoutMsg: "check_update never settled",
        interval: 500,
      },
    );

    const status = await $('[data-testid="settings-footer-status"]').getText();
    if (status.includes("检查更新失败") || status.includes("尚未配置")) {
      throw new Error(`updater check failed: ${status}`);
    }
    // Same version on host → latest; if somehow host is higher, still proves endpoint+pubkey work.
    expect(
      status.includes("已是最新版本") || status.includes("发现新版本"),
    ).toBe(true);

    expect(await findVisibleUiLeak()).toBeNull();
  });
});
