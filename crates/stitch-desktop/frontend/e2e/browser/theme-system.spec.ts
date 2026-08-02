/**
 * Default theme preference is system (follows prefers-color-scheme).
 */
import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Theme follows system by default", () => {
  test("no stored preference → data-theme matches colorScheme", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await mockTauri(page, { apiKeySet: true });
    await page.addInitScript(() => {
      localStorage.removeItem("stitch-theme");
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page.getByTestId("toggle-theme")).toHaveAttribute("data-theme-pref", "system");
    await assertUiHygiene(page);
  });

  test("toggle cycles system → light → dark → system", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light" });
    await mockTauri(page, { apiKeySet: true });
    await page.addInitScript(() => {
      localStorage.removeItem("stitch-theme");
    });
    await page.goto("/");
    await expect(page.getByTestId("toggle-theme")).toHaveAttribute("data-theme-pref", "system");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");

    await page.getByTestId("toggle-theme").click();
    await expect(page.getByTestId("toggle-theme")).toHaveAttribute("data-theme-pref", "light");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");

    await page.getByTestId("toggle-theme").click();
    await expect(page.getByTestId("toggle-theme")).toHaveAttribute("data-theme-pref", "dark");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

    await page.getByTestId("toggle-theme").click();
    await expect(page.getByTestId("toggle-theme")).toHaveAttribute("data-theme-pref", "system");
    await assertUiHygiene(page);
  });
});
