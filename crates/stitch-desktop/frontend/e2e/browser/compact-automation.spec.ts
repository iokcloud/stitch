import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Desktop automation → compact morph overlay: auto-enter on desktop tool,
// tool label + live elapsed visible, expand restores the full window.

test.describe("compact automation overlay", () => {
  test("desktop tool morphs the window into an executing floating bar", async ({
    page,
  }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamDesktopTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // Not compact before a desktop tool runs.
    await expect(page.getByTestId("compact-bar")).toBeHidden();

    await page.getByTestId("chat-input").fill("截个屏看看");
    await page.getByTestId("chat-input").press("Enter");

    // Auto-enter: overlay mode + floating bar visible.
    await expect(page.getByTestId("compact-bar")).toBeVisible({ timeout: 5_000 });
    expect(
      await page.evaluate(() => document.documentElement.getAttribute("data-compact")),
    ).toBe("true");

    // Execution state: Chinese tool label + breathing glow (no stopwatch).
    await expect(page.getByTestId("compact-tool")).toContainText("正在执行 截图", {
      timeout: 5_000,
    });
    // 呼吸光晕：浮条带 glow 动画（animation-name 含 compact-glow 即呼吸在跑）
    await expect
      .poll(async () =>
        page.locator(".compact-bar").evaluate((el) => getComputedStyle(el).animationName),
      )
      .toContain("compact-glow");

    // Drag region present on the label area (parkable anywhere).
    expect(
      await page
        .getByTestId("compact-tool")
        .evaluate((el) => !!el.closest("[data-tauri-drag-region]")),
    ).toBe(true);

    // Expand restores the full window and hides the bar.
    await page.getByTestId("compact-expand").click();
    await expect(page.getByTestId("compact-bar")).toBeHidden();
    expect(
      await page.evaluate(() => document.documentElement.getAttribute("data-compact")),
    ).toBeNull();

    await assertUiHygiene(page);
  });

  test("stop button cancels the running desktop turn", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamDesktopTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("截个屏看看");
    await page.getByTestId("chat-input").press("Enter");

    await expect(page.getByTestId("compact-bar")).toBeVisible({ timeout: 5_000 });
    await page.getByTestId("compact-bar").getByText("停止").click();

    // Turn ends: overlay exits, bar hidden, chat shows the stop state.
    await expect(page.getByTestId("compact-bar")).toBeHidden({ timeout: 5_000 });
    expect(
      await page.evaluate(() => document.documentElement.getAttribute("data-compact")),
    ).toBeNull();
  });

  test("turn completion holds a visible 已完成 state before restoring", async ({
    page,
  }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamDesktopTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("截个屏看看");
    await page.getByTestId("chat-input").press("Enter");
    await expect(page.getByTestId("compact-bar")).toBeVisible({ timeout: 5_000 });

    // Mock finishes the turn (~7s tool) → the bar switches to 已完成 and
    // lingers ~2.6s (visible to the human eye) before restoring the window.
    await expect(page.getByTestId("compact-tool")).toContainText("已完成", {
      timeout: 12_000,
    });
    await expect(page.getByTestId("compact-done")).toBeVisible();
    await expect(page.getByTestId("compact-bar")).toBeHidden({ timeout: 6_000 });
    expect(
      await page.evaluate(() => document.documentElement.getAttribute("data-compact")),
    ).toBeNull();
  });

  test("manual toggle hotkey round-trips and pins the bar", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamDesktopTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // Round trip: hotkey enters manual compact (idle state, no stop button),
    // hotkey again exits back to the full window.
    await page.keyboard.press("Control+Shift+KeyC");
    await expect(page.getByTestId("compact-bar")).toBeVisible({ timeout: 5_000 });
    await expect(page.getByTestId("compact-tool")).toHaveText("紧凑模式");
    await expect(page.getByTestId("compact-bar").getByText("停止")).toHaveCount(0);
    await page.keyboard.press("Control+Shift+KeyC");
    await expect(page.getByTestId("compact-bar")).toBeHidden({ timeout: 5_000 });

    // Pinned mid-turn: a desktop turn auto-enters the bar; the hotkey pins
    // it, so the turn finishing keeps the bar until toggled again.
    await page.getByTestId("chat-input").fill("截个屏看看");
    await page.getByTestId("chat-input").press("Enter");
    await expect(page.getByTestId("compact-tool")).toContainText("正在执行", {
      timeout: 5_000,
    });
    await page.keyboard.press("Control+Shift+KeyC"); // pin the running bar
    // After the mock turn ends (~7s) the bar stays, back to the idle label.
    await expect(page.getByTestId("compact-tool")).toHaveText("紧凑模式", {
      timeout: 12_000,
    });
    expect(
      await page.evaluate(() => document.documentElement.getAttribute("data-compact")),
    ).toBe("true");

    // Hotkey exits back to the full window.
    await page.keyboard.press("Control+Shift+KeyC");
    await expect(page.getByTestId("compact-bar")).toBeHidden({ timeout: 5_000 });
    expect(
      await page.evaluate(() => document.documentElement.getAttribute("data-compact")),
    ).toBeNull();
  });

  test("command palette entry enters compact mode", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.keyboard.press("Control+k");
    const item = page.getByTestId("palette-item").filter({ hasText: "进入紧凑模式" });
    await expect(item).toBeVisible();
    await item.click();
    await expect(page.getByTestId("compact-bar")).toBeVisible({ timeout: 5_000 });
    await expect(page.getByTestId("compact-tool")).toHaveText("紧凑模式");

    // In compact mode the app UI (incl. palette) is hidden — exit via hotkey.
    await page.keyboard.press("Control+Shift+KeyC");
    await expect(page.getByTestId("compact-bar")).toBeHidden({ timeout: 5_000 });
  });

  test("topbar button toggles compact mode (logo morph)", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // 顶栏按钮进入 compact（logo 变幻动画 260ms 后切换）
    await page.getByTestId("topbar-compact-toggle").click();
    await expect(page.getByTestId("compact-bar")).toBeVisible({ timeout: 5_000 });
    await expect(page.locator("html")).toHaveAttribute("data-compact", "true");

    // 按钮高亮（紧凑模式激活态）
    await expect(page.getByTestId("topbar-compact-toggle")).toHaveClass(/is-active/);

    // 再点退出
    await page.getByTestId("topbar-compact-toggle").click();
    await expect(page.getByTestId("compact-bar")).toBeHidden({ timeout: 5_000 });
  });
});
