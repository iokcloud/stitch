import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

const outDir = path.resolve("e2e/artifacts/theme-visual");

async function seedChat(page: Page) {
  const id = "theme-visual-seed";
  const now = Date.now();
  await page.evaluate(
    ({ id, now }) => {
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: id,
          sessions: {
            [id]: {
              id,
              title: "界面预览",
              createdAt: now,
              updatedAt: now,
              messages: [
                {
                  id: "u1",
                  type: "message",
                  role: "user",
                  content: "帮我看一下当前工作目录的结构。",
                },
                {
                  id: "p1",
                  type: "plan",
                  planId: "theme-plan",
                  title: "查看目录结构",
                  phase: "approved",
                  steps: [
                    { description: "列出顶层目录", status: "done" },
                    { description: "标出入口文件", status: "in_progress" },
                    { description: "汇总说明", status: "pending" },
                  ],
                },
                {
                  id: "t1",
                  type: "tool",
                  name: "list_dir",
                  done: false,
                  error: false,
                  summary: "运行中…",
                  detail: "",
                  expanded: false,
                },
                {
                  id: "a1",
                  type: "message",
                  role: "assistant",
                  content:
                    "可以。我会先列出顶层目录，再标出入口文件（如 `Cargo.toml`、`src/`）。确认工作目录正确后告诉我下一步。",
                },
              ],
            },
          },
        }),
      );
    },
    { id, now },
  );
}

async function captureTheme(page: Page, theme: "light" | "dark") {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
  await page.addInitScript((t) => {
    localStorage.setItem("stitch-theme", t);
  }, theme);
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await seedChat(page);
  await page.reload();
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
  await expect(page.getByText("帮我看一下当前工作目录")).toBeVisible();
  await assertUiHygiene(page);

  await page.screenshot({
    path: path.join(outDir, `${theme}-01-chat.png`),
    fullPage: true,
  });

  await page.getByTestId("library-tab").click();
  await expect(page.getByTestId("library-panel")).toBeVisible();
  await page.screenshot({
    path: path.join(outDir, `${theme}-02-library.png`),
    fullPage: true,
  });

  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("settings-view")).toBeVisible();
  await page.screenshot({
    path: path.join(outDir, `${theme}-03-settings.png`),
    fullPage: true,
  });
}

test.describe("Theme visual QA", () => {
  test("light surfaces", async ({ page }) => {
    await captureTheme(page, "light");
    const tokens = await page.evaluate(() => {
      const r = getComputedStyle(document.documentElement);
      return {
        bg: r.getPropertyValue("--color-background").trim(),
        rail: r.getPropertyValue("--color-rail").trim(),
        fg: r.getPropertyValue("--color-foreground").trim(),
      };
    });
    expect(tokens.bg.toLowerCase()).toMatch(/^#f4f6f8$/);
    expect(tokens.fg.toLowerCase()).toMatch(/^#0f172a$/);
    // No warm-beige / washed blue-gray regression
    expect(tokens.bg.toLowerCase()).not.toMatch(/ebe7df|e0dbd1|f7f5f1|e8eef5/);
  });

  test("dark surfaces", async ({ page }) => {
    await captureTheme(page, "dark");
  });
});
