import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Composer & workdir chrome", () => {
  test("textarea grows then shrinks after delete", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

    const input = page.getByTestId("chat-input");
    const baseH = await input.evaluate((el) => (el as HTMLTextAreaElement).getBoundingClientRect().height);

    const long = Array.from({ length: 8 }, (_, i) => `第 ${i + 1} 行测试内容，用于撑高输入框。`).join("\n");
    await input.fill(long);
    await input.dispatchEvent("input");
    const tallH = await input.evaluate((el) => (el as HTMLTextAreaElement).getBoundingClientRect().height);
    expect(tallH).toBeGreaterThan(baseH + 20);

    await input.fill("");
    await input.dispatchEvent("input");
    const afterH = await input.evaluate((el) => (el as HTMLTextAreaElement).getBoundingClientRect().height);
    expect(afterH).toBeLessThanOrEqual(baseH + 4);

    await assertUiHygiene(page);
  });

  test("workdir actions live on workspace rows (not a path bar)", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, workDir: "C:/tmp/stitch-e2e" });
    await page.goto("/");
    await expect(page.getByTestId("workspace-panel")).toBeVisible({ timeout: 15_000 });
    const panel = page.getByTestId("workspace-panel");
    await expect(panel.getByTestId("workdir-bar")).toHaveCount(0);
    await expect(page.getByTestId("workdir-pick")).toHaveCount(0);
    await expect(page.getByTestId("workdir-dialog")).toHaveCount(0);

    await page.getByTestId("workspace-more").first().click();
    await expect(page.getByTestId("workspace-menu")).toBeVisible();
    await expect(page.getByTestId("workspace-repath")).toHaveCount(0);
    await expect(page.getByTestId("workspace-rename-btn")).toHaveCount(0);
    await expect(page.getByTestId("workspace-open-folder")).toBeVisible();
    await expect(page.getByTestId("workspace-remove-btn")).toBeVisible();
    await page.getByTestId("chat-input").click();
    await expect(page.getByTestId("workspace-menu")).toHaveCount(0);

    await assertUiHygiene(page);
  });

  test("sidebar workspace panel lists current dir; add opens dialog", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, workDir: "C:/tmp/stitch-e2e" });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("workspace-panel")).toBeVisible();
    await expect(page.getByTestId("workspace-list")).toBeVisible();
    await expect(page.getByTestId("workspace-row").first()).toBeVisible();
    await expect(page.getByTestId("workspace-add")).toBeVisible();
    await expect(page.getByTestId("workspace-add")).toHaveAttribute("aria-label", "添加工作区");
    await page.getByTestId("workspace-add").click();
    await expect(page.getByTestId("workdir-dialog")).toBeVisible();
    await page.getByRole("button", { name: "取消" }).click();
    await expect(page.getByTestId("workdir-dialog")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("workspace tree nests sessions and can collapse", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, workDir: "C:/tmp/stitch-e2e" });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const group = page.getByTestId("workspace-group").first();
    await expect(group).toHaveAttribute("data-expanded", "1");
    await expect(page.getByTestId("session-row").first()).toBeVisible();
    await expect(page.getByTestId("session-title").first()).toBeVisible();
    await expect(page.getByTestId("session-time").first()).toBeVisible();
    await expect(group.getByTestId("session-new")).toBeVisible();
    await page.getByTestId("session-more").first().click();
    await expect(page.getByTestId("session-menu")).toBeVisible();
    await expect(page.getByTestId("session-copy-title")).toBeVisible();
    await expect(page.getByTestId("session-rename-btn")).toBeVisible();
    await expect(page.getByTestId("session-rollback-checkpoint")).toBeVisible();
    await expect(page.getByTestId("session-delete")).toBeVisible();
    await page.getByTestId("chat-input").click();
    await expect(page.getByTestId("session-menu")).toHaveCount(0);

    await page.getByTestId("workspace-collapse").first().click();
    await expect(group).toHaveAttribute("data-expanded", "0");
    await expect(group.getByTestId("session-row")).toHaveCount(0);

    // Active directory label also toggles fold (unified row, no separate chevron hover).
    await group.getByTestId("workspace-label").click();
    await expect(group).toHaveAttribute("data-expanded", "1");
    await expect(group.getByTestId("session-row").first()).toBeVisible();

    await page.getByTestId("workspace-collapse").first().click();
    await expect(group).toHaveAttribute("data-expanded", "0");

    await page.getByTestId("workspace-collapse").first().click();
    await expect(group).toHaveAttribute("data-expanded", "1");
    await expect(group.getByTestId("session-row").first()).toBeVisible();

    await expect(page.getByText("未绑定")).toHaveCount(0);

    await assertUiHygiene(page);
  });

  test("new session reuses empty shell in same workspace", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, workDir: "C:/tmp/stitch-e2e" });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const group = page.getByTestId("workspace-group").first();
    await group.getByTestId("session-new").click();
    await expect(group.getByTestId("session-row")).toHaveCount(1);
    await group.getByTestId("session-new").click();
    await expect(group.getByTestId("session-row")).toHaveCount(1);
    await expect(group.getByText("新会话")).toHaveCount(1);

    await assertUiHygiene(page);
  });

  test("activating a workspace does not reorder the list", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, workDir: "C:/tmp/ws-alpha" });
    await page.addInitScript(() => {
      localStorage.setItem(
        "stitch-workspaces",
        JSON.stringify({
          currentId: "w1",
          items: [
            { id: "w1", label: "alpha", path: "C:/tmp/ws-alpha", lastUsedAt: 1 },
            { id: "w2", label: "beta", path: "C:/tmp/ws-beta", lastUsedAt: 9_999 },
          ],
        }),
      );
    });
    await page.goto("/");
    await expect(page.getByTestId("workspace-row").first()).toBeVisible({ timeout: 15_000 });

    const labelsBefore = await page.getByTestId("workspace-label").allTextContents();
    expect(labelsBefore.map((t) => t.trim())).toEqual(["alpha", "beta"]);

    await page.getByTestId("workspace-row").nth(1).getByTestId("workspace-label").click();
    await expect(page.getByTestId("workspace-row").nth(1)).toHaveAttribute("data-active", "1");

    const labelsAfter = await page.getByTestId("workspace-label").allTextContents();
    expect(labelsAfter.map((t) => t.trim())).toEqual(["alpha", "beta"]);

    await assertUiHygiene(page);
  });

  test("workspace remove drops the row; open-folder stays available", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, workDir: "C:/tmp/ws-alpha" });
    await page.addInitScript(() => {
      localStorage.setItem(
        "stitch-workspaces",
        JSON.stringify({
          currentId: "w1",
          items: [
            { id: "w1", label: "alpha", path: "C:/tmp/ws-alpha", lastUsedAt: 1 },
            { id: "w2", label: "beta", path: "C:/tmp/ws-beta", lastUsedAt: 2 },
          ],
        }),
      );
    });
    await page.goto("/");
    await expect(page.getByTestId("workspace-row")).toHaveCount(2, { timeout: 15_000 });

    const beta = page.getByTestId("workspace-row").nth(1);
    await beta.hover();
    await beta.getByTestId("workspace-more").click();
    await page.getByTestId("workspace-open-folder").click();
    await expect(page.getByTestId("workspace-menu")).toHaveCount(0);

    await beta.hover();
    await beta.getByTestId("workspace-more").click();
    await page.getByTestId("workspace-remove-btn").click();
    await expect(page.getByTestId("workspace-row")).toHaveCount(1);
    await expect(page.getByTestId("workspace-label")).toHaveText("alpha");

    await assertUiHygiene(page);
  });

  test("model menu items have hover styles", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("model-menu-trigger").click();
    const menu = page.getByTestId("model-menu");
    await expect(menu).toBeVisible();
    const item = menu.getByTestId("model-menu-item").nth(1);
    await expect(item).toBeVisible();
    await item.hover();
    const bg = await item.evaluate((el) => getComputedStyle(el).backgroundColor);
    // Must not stay fully transparent after hover (dark/light both tint).
    expect(bg).not.toBe("rgba(0, 0, 0, 0)");
    expect(bg).not.toBe("transparent");
    await assertUiHygiene(page);
  });

  test("attach menu: Skills flyout and MCP settings entry", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("attach-menu-trigger")).toBeVisible();
    await page.getByTestId("attach-menu-trigger").click();
    await expect(page.getByTestId("attach-menu-pop")).toBeVisible();
    await page.getByTestId("attach-open-skills").click();
    await expect(page.getByTestId("capability-skill-demo-local")).toBeVisible();
    await expect(page.getByTestId("capability-skill-demo-user")).toBeVisible();
    await expect(page.getByTestId("capability-skill-demo-user")).toHaveAttribute(
      "data-scope",
      "user",
    );
    await page.getByTestId("capability-skill-demo-user").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/本机 Skill「用户全局 Skill」/);
    await page.getByTestId("attach-menu-trigger").click();
    await page.getByTestId("attach-open-skills").click();
    await page.getByTestId("capability-skill-demo-local").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/工作区 Skill「本机 Demo Skill」/);
    await page.getByTestId("attach-menu-trigger").click();
    await page.getByTestId("attach-open-mcp").click();
    await expect(page.getByTestId("capability-mcp-empty")).toBeVisible();
    await page.getByTestId("capability-mcp-settings").click();
    await expect(page.getByTestId("settings-tab-mcp")).toHaveClass(/settings-nav-item-active/);
    await assertUiHygiene(page);
  });
});
