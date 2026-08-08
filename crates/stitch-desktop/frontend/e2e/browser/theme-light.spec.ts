import { expect, test } from "@playwright/test";
import path from "node:path";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

const outDir = path.resolve("e2e/artifacts/theme-light");

test.describe("Light theme visual capture", () => {
  test("chat + settings in light mode", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.addInitScript(() => {
      localStorage.setItem("stitch-theme", "light");
    });
    await page.goto("/");

    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await assertUiHygiene(page);

    const contrast = await page.evaluate(() => {
      const body = getComputedStyle(document.body);
      const root = getComputedStyle(document.documentElement);
      return {
        color: body.color,
        background: body.backgroundColor,
        fgVar: root.getPropertyValue("--color-foreground").trim(),
        bgVar: root.getPropertyValue("--color-background").trim(),
        borderVar: root.getPropertyValue("--color-border").trim(),
        mutedVar: root.getPropertyValue("--color-muted").trim(),
      };
    });
    // Light theme: dark neutral ink + non-white page chrome
    // （2026-08-03 浅色中性化：#1a2b4c slate 蓝墨 → #1a1a1e 中性墨）
    expect(contrast.fgVar.toLowerCase()).toMatch(/^#1a2b4c$|^#0[0-9a-f]{5}$|^#1a1a1e$/);
    expect(contrast.bgVar.toLowerCase()).not.toBe("#ffffff");
    const m = contrast.color.match(/rgb\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
    expect(m).toBeTruthy();
    const r = Number(m![1]);
    const g = Number(m![2]);
    const b = Number(m![3]);
    expect(r + g + b).toBeLessThan(240);

    // Seed bubbles so light chat chrome is visible (SessionsStore shape)
    await page.evaluate(() => {
      const id = "theme-light-seed";
      const now = Date.now();
      const payload = {
        current: id,
        sessions: {
          [id]: {
            id,
            title: "浅色主题预览",
            createdAt: now,
            updatedAt: now,
            messages: [
              {
                id: "u1",
                type: "message",
                role: "user",
                content: "用一句话说明工作目录用途。",
              },
              {
                id: "a1",
                type: "message",
                role: "assistant",
                content:
                  "工作目录是 Stitch 读写文件与执行命令的根路径，请先确认再让我改代码。",
              },
            ],
          },
        },
      };
      localStorage.setItem("stitch-theme", "light");
      localStorage.setItem("stitch-sessions", JSON.stringify(payload));
    });
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText("工作目录是 Stitch")).toBeVisible({ timeout: 5_000 });

    await page.screenshot({
      path: path.join(outDir, "01-chat-light.png"),
      fullPage: true,
    });

    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.screenshot({
      path: path.join(outDir, "02-settings-light.png"),
      fullPage: true,
    });
  });
});
