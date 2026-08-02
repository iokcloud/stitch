import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Settings search: fuzzy filter in the nav, jump to field, flash highlight.

async function bootToSettings(page: Page) {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("command-palette")).toBeVisible();
  await page
    .getByTestId("palette-item")
    .filter({ hasText: "打开设置" })
    .first()
    .click();
  await expect(page.getByTestId("settings-view")).toBeVisible();
}

test.describe("settings search", () => {
  test("filters by Chinese keyword and jumps to the field in another tab", async ({
    page,
  }) => {
    await bootToSettings(page);
    const search = page.getByTestId("settings-search");

    await search.fill("主题");
    const hit = page.getByTestId("settings-search-hit").filter({ hasText: "外观" });
    await expect(hit).toBeVisible();
    await expect(hit).toContainText("通用");
    await hit.click();

    await expect(page.getByTestId("settings-tab-system")).toHaveClass(
      /settings-nav-item-active/,
    );
    await expect(page.getByTestId("settings-theme")).toBeVisible();
    // Jump target flashes briefly for orientation.
    await expect(page.locator(".settings-flash")).toHaveCount(1);
    // Query cleared, four tabs restored after the jump.
    await expect(search).toHaveValue("");
    await expect(page.getByTestId("settings-tab-model")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("Enter runs the active hit and expands advanced options when needed", async ({
    page,
  }) => {
    await bootToSettings(page);
    const search = page.getByTestId("settings-search");

    await search.fill("服务地址");
    await expect(page.getByTestId("settings-search-hit").first()).toHaveClass(/is-active/);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowUp");
    await search.press("Enter");

    await expect(page.getByTestId("settings-tab-account")).toHaveClass(
      /settings-nav-item-active/,
    );
    await expect(page.getByTestId("mcp-advanced-toggle")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await expect(page.getByTestId("prompt-api-base")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("no match shows empty state and Escape restores the tab list", async ({ page }) => {
    await bootToSettings(page);
    const search = page.getByTestId("settings-search");

    await search.fill("zzzz");
    await expect(page.getByTestId("settings-search-empty")).toBeVisible();
    await expect(page.getByTestId("settings-tab-model")).toHaveCount(0);

    await search.press("Escape");
    await expect(page.getByTestId("settings-tab-model")).toBeVisible();
    await expect(page.getByTestId("settings-tab-system")).toBeVisible();
    await assertUiHygiene(page);
  });
});
