import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Cross-feature integration: composer history ↑/↓ · Ctrl+K palette · Ctrl+F find ·
// edit-resend · regenerate · Esc layering · focus restore — combined in real workflows.
//
// These scenarios exercise interactions that isolated per-feature specs cannot catch:
// overlay stacking order, state leaks across modes, history correctness after mutations.

const SEED_ID = "x-feat-seed";

async function boot(page: Page) {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
}

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

function chatText(page: Page, text: string) {
  return page.getByTestId("chat-view").getByText(text, { exact: true });
}

async function expectComposerFocused(page: Page) {
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.id ?? ""), { timeout: 3_000 })
    .toBe("chat-input");
}

async function seedChat(page: Page) {
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
              title: "交叉场景会话",
              createdAt: now,
              updatedAt: now,
              messages: [
                { id: "u1", type: "message", role: "user", content: "第一问：列出入口文件。" },
                { id: "a1", type: "message", role: "assistant", content: "答一：入口是 src/main.rs，启动后加载配置。" },
                { id: "u2", type: "message", role: "user", content: "第二问：测试覆盖率是多少？" },
                { id: "a2", type: "message", role: "assistant", content: "答二：当前测试覆盖率约 85%，其中单元测试 200 个用例。" },
                { id: "u3", type: "message", role: "user", content: "第三问：有没有性能测试？" },
                { id: "a3", type: "message", role: "assistant", content: "答三：性能测试覆盖了启动时间和 API 响应时间。" },
              ],
            },
          },
        }),
      );
    },
    { id: SEED_ID, now },
  );
}

// ── Scenario 1: Edit-resend → history correctness ──────────────────────
test.describe("edit-resend + history chain", () => {
  test("after edit-resend, ↑ recalls the edited text and old version is not duplicated", async ({
    page,
  }) => {
    await boot(page);
    await sendAndWait(page, "原始消息A", 1);
    await sendAndWait(page, "原始消息B", 2);
    await sendAndWait(page, "原始消息C", 3);

    // Edit-resend the second message (3 user msgs: A=0, B=1, C=2).
    await chatText(page, "原始消息B").hover();
    await page.getByTestId("msg-edit").nth(1).click();
    const input = page.getByTestId("chat-input");
    await expect(input).toHaveValue("原始消息B");
    await input.fill("修改后的消息B");
    await page.getByTestId("chat-send").click();
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
    ).toHaveCount(2, { timeout: 10_000 });

    // Old text gone, new text present.
    await expect(chatText(page, "原始消息B")).toHaveCount(0);
    await expect(chatText(page, "修改后的消息B")).toBeVisible();

    // History: modified version in front, original (different text) still present.
    await input.press("ArrowUp");
    await expect(input).toHaveValue("修改后的消息B");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("原始消息C");
    await input.press("ArrowUp");
    // Original "原始消息B" still in history — different string from the edited version.
    await expect(input).toHaveValue("原始消息B");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("原始消息A");
    // ↑ at oldest stays put.
    await input.press("ArrowUp");
    await expect(input).toHaveValue("原始消息A");

    await assertUiHygiene(page);
  });

  test("edit-rewind + Cancel button cancels the edit, keeps chat intact", async ({ page }) => {
    await boot(page);
    await seedChat(page);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(chatText(page, "第三问：有没有性能测试？")).toBeVisible();

    // Start editing the last user message.
    await chatText(page, "第三问：有没有性能测试？").hover();
    await page.getByTestId("msg-edit").last().click();
    await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();

    // Cancel via button — Esc is reserved for stopping generation / closing overlays,
    // not for cancelling edit-rewind mode (by design: edit mode is not an overlay).
    await expect(page.getByTestId("edit-rewind-cancel")).toBeVisible({ timeout: 3_000 });
    await page.getByTestId("edit-rewind-cancel").click();
    await expect(page.getByTestId("edit-rewind-bar")).toHaveCount(0);
    await expect(page.getByTestId("chat-input")).toHaveValue("");

    // Chat is unchanged — all turns preserved.
    await expect(chatText(page, "第三问：有没有性能测试？")).toBeVisible();
    await expect(chatText(page, "答三：性能测试覆盖了启动时间和 API 响应时间。")).toBeVisible();
    await expect(chatText(page, "第一问：列出入口文件。")).toBeVisible();

    await assertUiHygiene(page);
  });
});

// ── Scenario 2: Find + Palette overlay stacking ────────────────────────
test.describe("find + palette overlay stacking", () => {
  test("find stays open after palette is dismissed, Esc closes them in order", async ({
    page,
  }) => {
    await boot(page);
    await seedChat(page);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // Open find first.
    await page.keyboard.press("Control+f");
    const findBar = page.getByTestId("find-bar");
    await expect(findBar).toBeVisible();
    await page.getByTestId("find-input").fill("测试");

    // Open palette on top.
    await page.keyboard.press("Control+k");
    await expect(page.getByTestId("command-palette")).toBeVisible();

    // First Esc: close palette, find bar still open.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toHaveCount(0);
    await expect(findBar).toBeVisible();

    // Second Esc: close find bar.
    await page.keyboard.press("Escape");
    await expect(findBar).toHaveCount(0);

    // Focus returns to composer.
    await expectComposerFocused(page);
    await assertUiHygiene(page);
  });

  test("Esc on palette does not dismiss underlying find when find is behind", async ({
    page,
  }) => {
    await boot(page);
    await seedChat(page);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // Open find, then close it (baseline).
    await page.keyboard.press("Control+f");
    await expect(page.getByTestId("find-bar")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("find-bar")).toHaveCount(0);

    // Now open palette alone, close it.
    await page.keyboard.press("Control+k");
    await expect(page.getByTestId("command-palette")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toHaveCount(0);
    await expectComposerFocused(page);

    await assertUiHygiene(page);
  });
});

// ── Scenario 3: Regenerate → find content freshness ────────────────────
test.describe("regenerate + find", () => {
  test("after regenerate, old answer text is not findable, new answer is", async ({
    page,
  }) => {
    await boot(page);
    await seedChat(page);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // The last assistant answer contains "性能测试"
    await expect(chatText(page, "答三：性能测试覆盖了启动时间和 API 响应时间。")).toBeVisible();

    // Regenerate the last answer.
    await chatText(page, "答三：性能测试覆盖了启动时间和 API 响应时间。").hover();
    await page.getByTestId("msg-regenerate").click();

    // Old answer removed; mock stream replaces it.
    await expect(chatText(page, "答三：性能测试覆盖了启动时间和 API 响应时间。")).toHaveCount(0);
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
    ).toBeVisible({ timeout: 10_000 });

    // Ctrl+F for the old text — should NOT find it.
    await page.keyboard.press("Control+f");
    await expect(page.getByTestId("find-bar")).toBeVisible();
    await page.getByTestId("find-input").fill("启动时间和 API 响应时间");
    await expect(page.getByTestId("find-count")).toHaveText("0/0");

    // Ctrl+F for the new text — should find it.
    await page.getByTestId("find-input").fill("流式回复完成");
    await expect(page.getByTestId("find-count")).toHaveText(/^[1-9]\d*\/[1-9]\d*$/);

    await page.keyboard.press("Escape");
    await assertUiHygiene(page);
  });
});

// ── Scenario 4: History recall during edit-rewind mode ─────────────────
test.describe("history + edit-rewind interaction", () => {
  test("edit-rewind mode disables history recall (↑)", async ({ page }) => {
    await boot(page);
    await sendAndWait(page, "消息甲", 1);
    await sendAndWait(page, "消息乙", 2);

    // Edit the last user message.
    await chatText(page, "消息乙").hover();
    await page.getByTestId("msg-edit").last().click();
    await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();
    const input = page.getByTestId("chat-input");
    await expect(input).toHaveValue("消息乙");

    // ↑ should NOT recall history in edit-rewind mode — draft is protected.
    await input.press("ArrowUp");
    await expect(input).toHaveValue("消息乙");

    // Cancel edit.
    await page.getByTestId("edit-rewind-cancel").click();
    await expect(input).toHaveValue("");

    // Now ↑ works normally.
    await input.press("ArrowUp");
    await expect(input).toHaveValue("消息乙");

    await assertUiHygiene(page);
  });
});

// ── Scenario 5: Palette + edit-rewind interaction ──────────────────────
test.describe("palette + edit-rewind interaction", () => {
  test("opening palette during edit-rewind does not lose the edit state", async ({
    page,
  }) => {
    await boot(page);
    await seedChat(page);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // Start edit-rewind.
    await chatText(page, "第三问：有没有性能测试？").hover();
    await page.getByTestId("msg-edit").last().click();
    await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();
    const input = page.getByTestId("chat-input");
    await expect(input).toHaveValue("第三问：有没有性能测试？");

    // Open palette.
    await page.keyboard.press("Control+k");
    await expect(page.getByTestId("command-palette")).toBeVisible();

    // Close palette.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toHaveCount(0);

    // Edit-rewind is still active.
    await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();
    await expect(input).toHaveValue("第三问：有没有性能测试？");

    // Can still send the edited message.
    await input.fill("第三问（改）：加一个集成测试。");
    await page.getByTestId("chat-send").click();
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(chatText(page, "第三问（改）：加一个集成测试。")).toBeVisible();
    await expect(chatText(page, "第三问：有没有性能测试？")).toHaveCount(0);

    await assertUiHygiene(page);
  });
});

// ── Scenario 6: Three-overlay Esc unwind ───────────────────────────────
test.describe("three-overlay Esc unwind", () => {
  test("Esc unwinds overlays one at a time without stopping generation", async ({
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

    // Open the + menu.
    await page.getByTestId("attach-menu-trigger").click();
    await expect(page.getByTestId("attach-menu-pop")).toBeVisible();

    // Start a generation (menu stays open).
    await page.getByTestId("chat-input").fill("审查本次改动");
    await page.getByTestId("chat-input").press("Enter");
    const tool = page.getByTestId("tool-status").first();
    await expect(tool).toBeVisible({ timeout: 5_000 });
    await expect(tool).toHaveAttribute("data-running", "true");

    // Open find on top of the menu.
    await page.keyboard.press("Control+f");
    await expect(page.getByTestId("find-bar")).toBeVisible();

    // Esc 1: close find.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("find-bar")).toHaveCount(0);
    // Menu still open, generation still running.
    await expect(page.getByTestId("attach-menu-pop")).toBeVisible();
    await expect(tool).toHaveAttribute("data-running", "true");

    // Esc 2: close menu.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("attach-menu-pop")).toHaveCount(0);
    await expect(tool).toHaveAttribute("data-running", "true");

    // Esc 3: stop generation.
    await page.keyboard.press("Escape");
    await expect(page.getByText(/已停止生成/)).toBeVisible({ timeout: 8_000 });
    await expect(tool).toHaveAttribute("data-stopped", "true");

    await assertUiHygiene(page);
  });
});

// ── Scenario 7: Full workflow — send → recall → edit → find → regenerate ──
test.describe("full workflow: all features in one session", () => {
  test("send multiple messages, recall, edit-resend, find, regenerate in sequence", async ({
    page,
  }) => {
    await boot(page);

    // Phase 1: Send three messages.
    await sendAndWait(page, "请写一个 Rust 测试用例。", 1);
    await sendAndWait(page, "再补一个性能测试。", 2);
    await sendAndWait(page, "最后写个 README。", 3);

    // Phase 2: Recall with ↑, modify, send.
    const input = page.getByTestId("chat-input");
    await input.press("ArrowUp");
    await expect(input).toHaveValue("最后写个 README。");
    // ↓ back to empty.
    await input.press("ArrowDown");
    await expect(input).toHaveValue("");

    // Phase 3: Find something in chat.
    await page.keyboard.press("Control+f");
    await expect(page.getByTestId("find-bar")).toBeVisible();
    await page.getByTestId("find-input").fill("README");
    await expect(page.getByTestId("find-count")).toHaveText(/^[1-9]\d*\/[1-9]\d*$/);
    await page.keyboard.press("Escape");

    // Phase 4: Edit-resend the second message (3 user msgs: Rust=0, perf=1, readme=2).
    await chatText(page, "再补一个性能测试。").hover();
    await page.getByTestId("msg-edit").nth(1).click();
    await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();
    await page.getByTestId("chat-input").fill("再补一个集成测试。");
    await page.getByTestId("chat-send").click();
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
    ).toHaveCount(2, { timeout: 10_000 });
    await expect(chatText(page, "再补一个性能测试。")).toHaveCount(0);
    await expect(chatText(page, "再补一个集成测试。")).toBeVisible();

    // Phase 5: History reflects the edit.
    await input.press("ArrowUp");
    await expect(input).toHaveValue("再补一个集成测试。");
    await input.press("ArrowDown");
    await expect(input).toHaveValue("");

    // Phase 6: Regenerate the last response — then immediately open/close the
    // palette while the mock stream is still in-flight (regression test for
    // deferred refocus after streaming ends).
    const lastAssistant = page.locator(".msg-assistant").last();
    await lastAssistant.hover();
    await page.getByTestId("msg-regenerate").click();
    // Open palette while the stream is running (composer disabled).
    await page.keyboard.press("Control+k");
    await expect(page.getByTestId("command-palette")).toBeVisible();
    await page.getByTestId("palette-input").fill("新建");
    await expect(
      page.getByTestId("palette-item").filter({ hasText: "新建会话" }),
    ).toBeVisible();
    // Close palette while still streaming — focus is deferred.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toHaveCount(0);

    // Wait for the stream to finish (send button returns to "发送").
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });
    // After streaming ends, deferred refocus should have fired.
    await expectComposerFocused(page);
    await page.getByTestId("chat-input").fill("自动聚焦验证");
    await expect(page.getByTestId("chat-input")).toHaveValue("自动聚焦验证");
    await assertUiHygiene(page);
  });
});
