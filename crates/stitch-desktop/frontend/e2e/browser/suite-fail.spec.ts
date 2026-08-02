import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Suite failure summary (mock)", () => {
  test("run_suite shows failure summary and marks plan steps", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      suiteFail: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("library-tab").click();
    await expect(page.getByTestId("library-panel")).toBeVisible();
    await page.getByTestId("library-tab-suites").click();
    await expect(page.getByText("演示套件")).toBeVisible({ timeout: 5_000 });
    await page.getByText("演示套件").click();

    await expect(page.getByText("未全部完成：第 2/2 步失败")).toBeVisible({
      timeout: 8_000,
    });
    await expect(page.getByText("已完成步骤")).toBeVisible();
    await expect(page.getByText("原因：模型超时")).toBeVisible();

    const plan = page.getByTestId("plan-card");
    await expect(plan).toBeVisible();
    await expect(plan.getByText("1 失败")).toBeVisible();
    await expect(plan.locator(".plan-step.is-failed")).toHaveCount(1);
    await expect(plan.locator(".plan-step.is-done")).toHaveCount(1);

    await assertUiHygiene(page);
  });
});
