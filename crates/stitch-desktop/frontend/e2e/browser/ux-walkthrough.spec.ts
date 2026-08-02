import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { mockTauri } from "../helpers/mock-tauri";

// Probe-tier visual walkthrough of the surfaces added in the 2026-07-31
// UI/UX round (palette / shortcuts / compact / model menu / settings panes /
// welcome / composer counter), in both themes. Screenshots feed Layer V.
// Not part of the default smoke; run directly:
//   npx playwright test ux-walkthrough

const outDir = path.resolve("e2e/artifacts/ux-walkthrough");

async function boot(page: Page, theme: "light" | "dark") {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
  await page.addInitScript((t) => {
    localStorage.setItem("stitch-theme", t);
  }, theme);
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
}

async function seedBusyChat(page: Page) {
  const id = "ux-walk-seed";
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
              title: "界面走查会话",
              createdAt: now,
              updatedAt: now,
              messages: [
                {
                  id: "u1",
                  type: "message",
                  role: "user",
                  content: "帮我把这个模块的 README 补齐，并跑一遍测试。",
                },
                {
                  id: "t1",
                  type: "tool",
                  name: "write_file",
                  done: true,
                  error: false,
                  summary: "README.md",
                  detail: "# Demo\n\ncontent",
                  expanded: false,
                },
                {
                  id: "a1",
                  type: "message",
                  role: "assistant",
                  content:
                    "已补齐 README 的三个章节。接下来跑 `cargo test` 验证，预计十秒内完成。\n\n- 安装步骤\n- 使用示例\n- 常见问题",
                },
                {
                  id: "u2",
                  type: "message",
                  role: "user",
                  content: "测试里有一个失败，先别管，继续写 CHANGELOG。",
                },
                {
                  id: "a2",
                  type: "message",
                  role: "assistant",
                  content:
                    "好，跳过失败用例。CHANGELOG 已按 keepachangelog 格式起草 v0.2.0 段落。",
                },
              ],
            },
          },
        }),
      );
    },
    { id, now },
  );
}

async function shot(page: Page, name: string) {
  await page.screenshot({ path: path.join(outDir, `${name}.png`) });
}

async function walk(page: Page, theme: "light" | "dark") {
  await boot(page, theme);

  // 01 welcome (fresh, no sessions)
  await expect(page.getByTestId("welcome-scenes")).toBeVisible();
  await shot(page, `${theme}-01-welcome`);

  // 02 busy chat with turn divider + composer counter
  await seedBusyChat(page);
  await page.reload();
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText("帮我把这个模块的 README 补齐")).toBeVisible();
  const filler =
    "请把 CHANGELOG 再补一节，说明这次改动的兼容性注意事项，".repeat(10);
  await page.locator(".composer-input").fill(filler);
  await expect(page.locator(".composer-count")).toBeVisible();
  await shot(page, `${theme}-02-chat-busy`);
  await page.locator(".composer-input").fill("");

  // 03 command palette
  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("command-palette")).toBeVisible();
  await shot(page, `${theme}-03-palette`);
  await page.keyboard.press("Escape");

  // 04 shortcuts dialog
  await page.keyboard.press("Control+/");
  await expect(page.getByTestId("shortcuts-dialog")).toBeVisible();
  await shot(page, `${theme}-04-shortcuts`);
  await page.keyboard.press("Escape");

  // 05 model menu
  await page.getByTestId("model-menu-trigger").click();
  await expect(page.getByTestId("model-menu")).toBeVisible();
  await shot(page, `${theme}-05-model-menu`);
  await page.keyboard.press("Escape");

  // 06 compact overlay bar (attribute-driven; window resize is Tauri-side)
  await page.evaluate(() => {
    document.documentElement.setAttribute("data-compact", "true");
  });
  await expect(page.getByTestId("compact-bar")).toBeVisible();
  await shot(page, `${theme}-06-compact`);
  await page.evaluate(() => {
    document.documentElement.removeAttribute("data-compact");
  });

  // 07-10 settings panes
  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("settings-view")).toBeVisible();
  for (const [pane, name] of [
    ["model", "07-settings-model"],
    ["account", "08-settings-account"],
    ["mcp", "09-settings-mcp"],
    ["system", "10-settings-general"],
  ] as const) {
    await page.getByTestId(`settings-tab-${pane}`).click();
    await shot(page, `${theme}-${name}`);
  }
}

test.describe("UX walkthrough (probe)", () => {
  test("light", async ({ page }) => {
    await walk(page, "light");
  });
  test("dark", async ({ page }) => {
    await walk(page, "dark");
  });
});
