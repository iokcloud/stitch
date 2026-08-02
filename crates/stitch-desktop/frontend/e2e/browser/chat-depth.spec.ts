import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Chat main-loop depth: regenerate / edit-resend / in-session find.

const SEED_ID = "chat-depth-seed";

async function seedTwoTurns(page: Page) {
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
              title: "主链路会话",
              createdAt: now,
              updatedAt: now,
              messages: [
                {
                  id: "u1",
                  type: "message",
                  role: "user",
                  content: "第一问：列出入口文件。",
                },
                {
                  id: "a1",
                  type: "message",
                  role: "assistant",
                  content: "答一：入口是 src/main.rs。",
                },
                {
                  id: "u2",
                  type: "message",
                  role: "user",
                  content: "第二问：跑一遍测试。",
                },
                {
                  id: "a2",
                  type: "message",
                  role: "assistant",
                  content: "答二：测试全部通过。",
                },
              ],
            },
          },
        }),
      );
    },
    { id: SEED_ID, now },
  );
}

// The hidden compact-bar mirrors the in-flight label — scope text queries
// to the chat view to avoid strict-mode collisions with it.
function chatText(page: Page, text: string) {
  return page.getByTestId("chat-view").getByText(text, { exact: true });
}

async function boot(page: Page) {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await seedTwoTurns(page);
  await page.reload();
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await expect(chatText(page, "第二问：跑一遍测试。")).toBeVisible();
}

test.describe("chat depth", () => {
  test("regenerate rewinds to the user turn and re-answers", async ({ page }) => {
    await boot(page);
    // 重新生成 only on the last assistant message
    await expect(page.getByTestId("msg-regenerate")).toHaveCount(1);
    await chatText(page, "答二：测试全部通过。").hover();
    await page.getByTestId("msg-regenerate").click();

    // Old answer removed; streamed replacement arrives (mock streamChat).
    await expect(chatText(page, "答二：测试全部通过。")).toHaveCount(0);
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
    ).toBeVisible({ timeout: 10_000 });
    // User turns kept; first turn untouched.
    await expect(chatText(page, "第二问：跑一遍测试。")).toBeVisible();
    await expect(chatText(page, "答一：入口是 src/main.rs。")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("edit user message resends and drops the tail", async ({ page }) => {
    await boot(page);
    await chatText(page, "第二问：跑一遍测试。").hover();
    await page.getByTestId("msg-edit").last().click();

    await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();
    const input = page.getByTestId("chat-input");
    await expect(input).toHaveValue("第二问：跑一遍测试。");
    await input.fill("第二问（改）：只跑单元测试。");
    await page.getByTestId("chat-send").click();

    // Original user turn and everything after it are gone.
    await expect(chatText(page, "第二问：跑一遍测试。")).toHaveCount(0);
    await expect(chatText(page, "答二：测试全部通过。")).toHaveCount(0);
    await expect(chatText(page, "第二问（改）：只跑单元测试。")).toBeVisible();
    // First turn untouched.
    await expect(chatText(page, "答一：入口是 src/main.rs。")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("edit-rewind can be cancelled", async ({ page }) => {
    await boot(page);
    await chatText(page, "第二问：跑一遍测试。").hover();
    await page.getByTestId("msg-edit").last().click();
    await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();
    await page.getByTestId("edit-rewind-cancel").click();
    await expect(page.getByTestId("edit-rewind-bar")).toHaveCount(0);
    await expect(page.getByTestId("chat-input")).toHaveValue("");
    // Nothing was dropped.
    await expect(chatText(page, "答二：测试全部通过。")).toBeVisible();
  });

  test("Ctrl+F finds matches in the session", async ({ page }) => {
    await boot(page);
    await page.keyboard.press("Control+f");
    await expect(page.getByTestId("find-bar")).toBeVisible();
    await page.getByTestId("find-input").fill("测试");
    await expect(page.getByTestId("find-count")).toHaveText(/^[1-9]\d*\/[1-9]\d*$/);
    // Enter navigates without closing; Esc closes.
    await page.getByTestId("find-input").press("Enter");
    await expect(page.getByTestId("find-bar")).toBeVisible();
    await page.getByTestId("find-input").press("Escape");
    await expect(page.getByTestId("find-bar")).toHaveCount(0);
    await assertUiHygiene(page);
  });
});
