import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Layout state memory: sidebar tab + library sub-tab persist across reload.

async function boot(page: Page) {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
}

test.describe("layout state memory", () => {
  test("sidebar tab persists across reload", async ({ page }) => {
    await boot(page);

    // Default: 会话 tab active.
    await expect(page.getByTestId("library-tab")).toHaveAttribute(
      "aria-selected",
      "false",
    );

    await page.getByTestId("library-tab").click();
    await expect(page.getByTestId("library-tab")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(page.getByTestId("library-panel")).toBeVisible();

    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // After reload the 场景 tab is restored (not the default 会话).
    await expect(page.getByTestId("library-tab")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(page.getByTestId("library-panel")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("library sub-tab persists across reload", async ({ page }) => {
    await boot(page);
    await page.getByTestId("library-tab").click();
    await expect(page.getByTestId("library-panel")).toBeVisible();

    // Default sub-tab is 精选 (scenes).
    await expect(page.getByTestId("library-tab-skills")).toHaveAttribute(
      "aria-selected",
      "false",
    );

    await page.getByTestId("library-tab-skills").click();
    await expect(page.getByTestId("library-tab-skills")).toHaveAttribute(
      "aria-selected",
      "true",
    );

    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // Library tab itself is also restored (part of the same layout memory).
    await expect(page.getByTestId("library-panel")).toBeVisible();
    await expect(page.getByTestId("library-tab-skills")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await assertUiHygiene(page);
  });

  test("invalid persisted kind falls back to scenes", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.addInitScript(() => {
      try {
        localStorage.setItem("stitch-library-kind", "not-a-kind");
        localStorage.setItem("stitch-sidebar-tab", "library");
      } catch {
        /* ignore */
      }
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await expect(page.getByTestId("library-panel")).toBeVisible();
    // 精选 (scenes) restored as fallback; invalid value is overwritten.
    await expect(page.getByTestId("library-tab-scenes")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(page.getByTestId("library-tab-suites")).toHaveAttribute(
      "aria-selected",
      "false",
    );
    await assertUiHygiene(page);
  });
});
