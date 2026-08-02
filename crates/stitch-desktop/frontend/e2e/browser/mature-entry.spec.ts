/**
 * Official mature scenes entry: library-only, fill composer (no auto-send).
 * Captures a shot for Layer V when accept.sh / smoke-ui runs.
 */
import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

const outDir = path.resolve("e2e/artifacts/mature-entry");

test.describe("Official mature scenes entry", () => {
  test("library lists mature scenes; click fills composer; welcome stays light-only", async ({
    page,
  }) => {
    fs.mkdirSync(outDir, { recursive: true });
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.goto("/");

    await expect(page.getByTestId("welcome-scenes")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("scene-structure")).toBeVisible();
    await expect(page.getByTestId("library-mature-scenes")).toHaveCount(0);

    await page.getByTestId("library-tab").click();
    await expect(page.getByTestId("library-mature-scenes")).toBeVisible();
    await expect(page.getByTestId("library-mature-debug-recover-auto")).toBeVisible();
    await expect(page.getByTestId("library-mature-checkpoint-resume")).toBeVisible();
    await expect(page.getByTestId("library-mature-merge-ready-auto")).toBeVisible();
    await expect(page.getByTestId("library-mature-scope-lock-audit")).toBeVisible();
    await expect(page.getByTestId("library-scenes")).toBeVisible();

    await page.screenshot({
      path: path.join(outDir, "library-mature-panel.png"),
      fullPage: false,
    });

    await page.getByTestId("library-mature-checkpoint-resume").click();
    const input = page.getByTestId("chat-input");
    await expect(input).toHaveValue(/长任务检查点续跑/);
    await expect(input).toHaveValue(/stitch-checkpoint\.json/);
    await expect(input).toHaveValue(/我的目标/);
    // G1: paid_pool + non-member → soft tip; send still available
    await expect(page.getByTestId("mature-soft-gate")).toBeVisible();
    await expect(page.getByTestId("mature-soft-gate")).toContainText(/会员方案|连接账号/);
    await expect(page.getByTestId("chat-send")).toBeEnabled();

    await page.getByTestId("mature-soft-gate-dismiss").click();
    await expect(page.getByTestId("mature-soft-gate")).toHaveCount(0);
    // Dismiss mutes for the session — re-select paid_pool stays quiet
    await page.getByTestId("library-mature-merge-ready-auto").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/合并前审查自动/);
    await expect(page.getByTestId("mature-soft-gate")).toHaveCount(0);

    await page.screenshot({
      path: path.join(outDir, "composer-filled.png"),
      fullPage: false,
    });

    await page.getByTestId("library-mature-debug-recover-auto").click();
    await expect(input).toHaveValue(/改崩后停线复原/);
    await expect(page.getByTestId("mature-soft-gate")).toHaveCount(0);

    await assertUiHygiene(page);
  });

  test("paid_pool soft tip hidden for members", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, isMember: true });
    await page.goto("/");
    await page.getByTestId("library-tab").click();
    await page.getByTestId("library-mature-merge-ready-auto").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/合并前审查自动/);
    await expect(page.getByTestId("mature-soft-gate")).toHaveCount(0);
    await assertUiHygiene(page);
  });
});
