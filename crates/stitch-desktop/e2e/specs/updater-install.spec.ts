/**
 * U2: same flow as updater-discover, then click 安装更新 with real local .sig.
 * Session death after install is expected on Windows (NSIS exits the app).
 */
import { findVisibleUiLeak } from "../helpers/ui-hygiene";

describe("Stitch updater U2 (local signed install)", () => {
  it("settings → 通用 → 发现新版本 → 安装更新", async () => {
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
      throw new Error(`updater U2 discover failed: ${status}`);
    }
    if (!status.includes("发现新版本")) {
      throw new Error(
        `expected 发现新版本 (local U2 fixture); got: ${status}. ` +
          "Rebuild with e2e/tauri.updater-u2.json and keep u2 fixture server on :18765.",
      );
    }

    const btn = await $('[data-testid="check-update"]').getText();
    expect(btn.includes("安装更新")).toBe(true);
    expect(await findVisibleUiLeak()).toBeNull();

    // Click install. Windows updater exits the process → WDIO may lose the session.
    try {
      await $('[data-testid="check-update"]').click();
      await browser.waitUntil(
        async () => {
          const el = await $('[data-testid="settings-footer-status"]');
          if (!(await el.isExisting())) return false;
          const t = await el.getText();
          return t.includes("正在安装") || t.includes("安装更新失败") || t.includes("没有可用");
        },
        { timeout: 180_000, interval: 500 },
      );
      const after = await $('[data-testid="settings-footer-status"]').getText();
      if (after.includes("安装更新失败") || after.includes("没有可用")) {
        throw new Error(`updater U2 install failed: ${after}`);
      }
    } catch (e) {
      const msg = String(e ?? "");
      if (/session|disconnected|ECONNREFUSED|no such window|invalid session/i.test(msg)) {
        // Expected when NSIS forces app exit after a verified download.
        return;
      }
      throw e;
    }
  });
});
