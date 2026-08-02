import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Command palette: fuzzy match / recency ordering / extra actions / keyboard flow.

async function boot(page: Page) {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
}

async function openPalette(page: Page) {
  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("command-palette")).toBeVisible();
}

test.describe("command palette", () => {
  test("fuzzy matches titles and keyword aliases", async ({ page }) => {
    await boot(page);
    await openPalette(page);
    const input = page.getByTestId("palette-input");

    // English alias hits 打开设置 (keyword "settings").
    await input.fill("settings");
    await expect(
      page.getByTestId("palette-item").filter({ hasText: "打开设置" }),
    ).toBeVisible();

    // Alias hits 切换主题 (keyword "theme").
    await input.fill("theme");
    await expect(
      page.getByTestId("palette-item").filter({ hasText: "切换主题" }),
    ).toBeVisible();

    // Subsequence of the Chinese title still matches 新建会话.
    await input.fill("新会");
    await expect(
      page.getByTestId("palette-item").filter({ hasText: "新建会话" }),
    ).toBeVisible();

    // No match → empty state.
    await input.fill("zzzz");
    await expect(page.getByTestId("palette-empty")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("keyboard: arrows + Enter runs the active command", async ({ page }) => {
    await boot(page);
    await openPalette(page);
    // Default order: 新建会话 first, ArrowDown → 打开设置.
    await expect(page.getByTestId("palette-item").first()).toHaveText(/新建会话/);
    await page.getByTestId("palette-input").press("ArrowDown");
    await page.getByTestId("palette-input").press("Enter");
    await expect(page.getByTestId("settings-view")).toBeVisible();
  });

  test("recently used commands float to the top", async ({ page }) => {
    await boot(page);
    await openPalette(page);
    await page.getByTestId("palette-item").filter({ hasText: "切换主题" }).click();
    await openPalette(page);
    await expect(page.getByTestId("palette-item").first()).toHaveText(/切换主题/);
    await assertUiHygiene(page);
  });

  test("settings deep links open the requested tab", async ({ page }) => {
    await boot(page);
    await openPalette(page);
    await page
      .getByTestId("palette-item")
      .filter({ hasText: "设置：账号" })
      .click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await expect(page.getByTestId("settings-tab-account")).toHaveClass(
      /settings-nav-item-active/,
    );
  });

  test("delete current session switches to the next one", async ({ page }) => {
    await boot(page);
    await page.evaluate(() => {
      const now = Date.now();
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: "ses-a",
          sessions: {
            "ses-a": {
              id: "ses-a",
              title: "甲会话",
              createdAt: now,
              updatedAt: now,
              messages: [{ id: "a-u1", type: "message", role: "user", content: "甲的问题" }],
            },
            "ses-b": {
              id: "ses-b",
              title: "乙会话",
              createdAt: now,
              updatedAt: now - 1000,
              messages: [{ id: "b-u1", type: "message", role: "user", content: "乙的问题" }],
            },
          },
        }),
      );
    });
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(
      page.getByTestId("chat-view").getByText("甲的问题", { exact: true }),
    ).toBeVisible();

    await openPalette(page);
    await page
      .getByTestId("palette-item")
      .filter({ hasText: "删除当前会话" })
      .click();

    await expect(
      page.getByTestId("chat-view").getByText("乙的问题", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByTestId("session-title").filter({ hasText: "甲会话" }),
    ).toHaveCount(0);
    await assertUiHygiene(page);
  });
});
