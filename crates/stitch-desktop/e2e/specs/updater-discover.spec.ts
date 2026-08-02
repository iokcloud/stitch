/**
 * Real updater check against local fake stitch-update.json (version > app).
 * Requires: smoke-updater-discover.sh (or serve fixture + build with tauri.updater-discover.json).
 * Does NOT click install (dummy signature cannot complete U2).
 */
import { findVisibleUiLeak } from "../helpers/ui-hygiene";

describe("Stitch updater (local fake higher version)", () => {
  it("settings → 通用 → 检查更新 → 发现新版本", async () => {
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
        return (
          t.includes("发现新版本") ||
          t.includes("已是最新版本") ||
          t.includes("检查更新失败")
        );
      },
      {
        timeout: 45_000,
        timeoutMsg: "check_update never settled",
        interval: 500,
      },
    );

    const status = await $('[data-testid="settings-footer-status"]').getText();
    if (status.includes("检查更新失败") || status.includes("尚未配置")) {
      throw new Error(`updater discover failed: ${status}`);
    }
    if (!status.includes("发现新版本")) {
      throw new Error(
        `expected 发现新版本 (local fake higher version); got: ${status}. ` +
          "Rebuild with e2e/tauri.updater-discover.json and keep fake server on :18765.",
      );
    }

    const btn = await $('[data-testid="check-update"]').getText();
    expect(btn.includes("安装更新")).toBe(true);

    expect(await findVisibleUiLeak()).toBeNull();
  });
});
