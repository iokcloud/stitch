import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { mockTauri } from "../helpers/mock-tauri";

// Probe-tier visual evidence for chat-depth chrome (Layer V):
// regenerate action / user edit action / edit chip / find bar with marks.

const outDir = path.resolve("e2e/artifacts/chat-depth-visual");
const SEED_ID = "chat-depth-visual-seed";

async function boot(page: Page, theme: "light" | "dark") {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
  await page.addInitScript((t) => {
    localStorage.setItem("stitch-theme", t);
  }, theme);
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
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
              title: "主链路视觉",
              createdAt: now,
              updatedAt: now,
              messages: [
                {
                  id: "u1",
                  type: "message",
                  role: "user",
                  content: "帮我把 README 的安装章节补齐。",
                },
                {
                  id: "a1",
                  type: "message",
                  role: "assistant",
                  content:
                    "已补齐安装章节，包含前置依赖、三步安装与常见问题。\n\n- 依赖：Rust 1.85+\n- 步骤：clone → build → run\n\n如需调整语气或补充截图，告诉我。",
                },
              ],
            },
          },
        }),
      );
    },
    { id: SEED_ID, now },
  );
  await page.reload();
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
}

async function walk(page: Page, theme: "light" | "dark") {
  await boot(page, theme);

  // 01 assistant hover → 复制 / 重新生成 / 保存
  await page.locator(".msg-assistant").first().hover();
  await expect(page.getByTestId("msg-regenerate")).toBeVisible();
  await page.screenshot({ path: path.join(outDir, `${theme}-01-assistant-actions.png`) });

  // 02 user bubble hover → 编辑
  await page.getByText("帮我把 README 的安装章节补齐。", { exact: true }).hover();
  await expect(page.getByTestId("msg-edit")).toBeVisible();
  await page.screenshot({ path: path.join(outDir, `${theme}-02-user-edit.png`) });

  // 03 edit chip above composer
  await page.getByTestId("msg-edit").click();
  await expect(page.getByTestId("edit-rewind-bar")).toBeVisible();
  await page.screenshot({ path: path.join(outDir, `${theme}-03-edit-chip.png`) });
  await page.getByTestId("edit-rewind-cancel").click();

  // 04 find bar with matches
  await page.keyboard.press("Control+f");
  await page.getByTestId("find-input").fill("安装");
  await expect(page.getByTestId("find-count")).toHaveText(/^[1-9]\d*\/[1-9]\d*$/);
  await page.screenshot({ path: path.join(outDir, `${theme}-04-find.png`) });
}

test.describe("chat depth visual (probe)", () => {
  test("light", async ({ page }) => {
    await walk(page, "light");
  });
  test("dark", async ({ page }) => {
    await walk(page, "dark");
  });
});
