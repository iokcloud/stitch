import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Composer send-history (↑ recall) · Esc layered exit · focus restore.

async function boot(page: Page) {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
}

/** Send via composer Enter and wait for the mocked stream to settle. */
async function sendAndWait(page: Page, text: string, replies: number) {
  const input = page.getByTestId("chat-input");
  await input.fill(text);
  await input.press("Enter");
  await expect(
    page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
  ).toHaveCount(replies, { timeout: 10_000 });
  await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
    timeout: 5_000,
  });
}

async function readHistory(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    try {
      return JSON.parse(localStorage.getItem("stitch-composer-history") || "[]") as string[];
    } catch {
      return [];
    }
  });
}

async function cancelCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const i = (
      window as unknown as { __TAURI_INTERNALS__?: { cancelGenerationCount?: number } }
    ).__TAURI_INTERNALS__;
    return i?.cancelGenerationCount ?? 0;
  });
}

async function expectComposerFocused(page: Page) {
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.id ?? ""), { timeout: 3_000 })
    .toBe("chat-input");
}

test.describe("composer send history", () => {
  test("↑ recalls sent messages newest-first, ↓ walks back to the draft", async ({
    page,
  }) => {
    await boot(page);
    await sendAndWait(page, "第一条消息", 1);
    await sendAndWait(page, "第二条消息", 2);
    expect(await readHistory(page)).toEqual(["第二条消息", "第一条消息"]);

    const input = page.getByTestId("chat-input");
    await expect(input).toHaveValue("");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("第二条消息");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("第一条消息");
    // Oldest entry: further ↑ stays put.
    await input.press("ArrowUp");
    await expect(input).toHaveValue("第一条消息");
    // ↓ walks back towards the stashed (empty) draft.
    await input.press("ArrowDown");
    await expect(input).toHaveValue("第二条消息");
    await input.press("ArrowDown");
    await expect(input).toHaveValue("");

    // ↑ with a non-empty draft must not clobber it.
    await input.fill("打字中的草稿");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("打字中的草稿");
    await assertUiHygiene(page);
  });

  test("history dedups on resend and persists across reload", async ({ page }) => {
    await boot(page);
    await sendAndWait(page, "甲消息", 1);
    await sendAndWait(page, "乙消息", 2);
    await sendAndWait(page, "甲消息", 3);
    // Resent text floats to the front instead of duplicating.
    expect(await readHistory(page)).toEqual(["甲消息", "乙消息"]);

    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    const input = page.getByTestId("chat-input");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("甲消息");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("乙消息");
    await assertUiHygiene(page);
  });
});

test.describe("esc layered exit", () => {
  test("Esc on attach menu closes it without stopping the generation", async ({
    page,
  }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamRunningTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

    // Open the + menu first, then start a generation with the menu still open
    // (keyboard send does not dismiss the flyout).
    await page.getByTestId("attach-menu-trigger").click();
    await expect(page.getByTestId("attach-menu-pop")).toBeVisible();
    await page.getByTestId("chat-input").fill("审查本次改动");
    await page.getByTestId("chat-input").press("Enter");

    const tool = page.getByTestId("tool-status").first();
    await expect(tool).toBeVisible({ timeout: 5_000 });
    await expect(tool).toHaveAttribute("data-running", "true");
    await expect(page.getByTestId("attach-menu-pop")).toBeVisible();

    // First Esc: dismiss the menu only — generation keeps running.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("attach-menu-pop")).toHaveCount(0);
    await expect(tool).toHaveAttribute("data-running", "true");
    expect(await cancelCount(page)).toBe(0);

    // Second Esc: now the window-level handler stops the generation.
    await page.keyboard.press("Escape");
    await expect(page.getByText(/已停止生成/)).toBeVisible({ timeout: 8_000 });
    await expect(tool).toHaveAttribute("data-stopped", "true");
    expect(await cancelCount(page)).toBe(1);
    await assertUiHygiene(page);
  });

  test("Esc on confirm card rejects only, does not cancel the session", async ({
    page,
  }) => {
    await mockTauri(page, { apiKeySet: true, confirmFlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("写文件并跑测试");
    await page.getByTestId("chat-send").click();

    const card = page.getByTestId("confirm-card");
    await expect(card).toBeVisible({ timeout: 5_000 });

    await page.keyboard.press("Escape");
    await expect(card).toHaveCount(0);
    // Rejection lands on the tool card; the session-level cancel IPC never fired.
    await expect(page.getByTestId("tool-status").first()).toContainText("已拒绝", {
      timeout: 8_000,
    });
    expect(await cancelCount(page)).toBe(0);
    await assertUiHygiene(page);
  });
});

test.describe("focus restore", () => {
  test("closing overlays returns focus to the composer", async ({ page }) => {
    await boot(page);

    // Command palette (Ctrl+K → Esc).
    await page.keyboard.press("Control+k");
    await expect(page.getByTestId("command-palette")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toHaveCount(0);
    await expectComposerFocused(page);

    // In-session find (Ctrl+F → Esc).
    await page.keyboard.press("Control+f");
    await expect(page.getByTestId("find-bar")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("find-bar")).toHaveCount(0);
    await expectComposerFocused(page);

    // Shortcuts help (Ctrl+/ → Esc).
    await page.keyboard.press("Control+/");
    await expect(page.getByTestId("shortcuts-dialog")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("shortcuts-dialog")).toHaveCount(0);
    await expectComposerFocused(page);

    // Attach menu (click → Esc).
    await page.getByTestId("attach-menu-trigger").click();
    await expect(page.getByTestId("attach-menu-pop")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("attach-menu-pop")).toHaveCount(0);
    await expectComposerFocused(page);
    await assertUiHygiene(page);
  });
});
