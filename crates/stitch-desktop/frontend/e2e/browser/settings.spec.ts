import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

async function expectView(page: import("@playwright/test").Page, view: "settings" | "chat") {
  await expect(page.getByTestId("diag-view")).toHaveText(new RegExp(`view=${view}`));
  if (view === "settings") {
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await expect(page.getByTestId("chat-view")).toHaveCount(0);
  } else {
    await expect(page.getByTestId("chat-view")).toBeVisible();
    await expect(page.getByTestId("settings-view")).toHaveCount(0);
  }
  const hookView = await page.evaluate(() =>
    (window as unknown as { __stitchView?: () => string }).__stitchView?.(),
  );
  expect(hookView).toBe(view);
  const dataView = await page.evaluate(() => document.documentElement.dataset.appView);
  expect(dataView).toBe(view);
  await assertUiHygiene(page);
}

test.describe("Settings navigation & model smoke", () => {
  test("first-run: save with API key enters chat", async ({ page }) => {
    await mockTauri(page, { apiKeySet: false });
    await page.goto("/");

    await expect(page.getByTestId("settings-view")).toBeVisible({ timeout: 15_000 });
    // 无密钥默认进首次向导——「跳过，去设置」回到设置页首启模式
    await page.getByRole("button", { name: "跳过，去设置" }).click();
    await expect(page.getByTestId("firstrun-wizard")).toHaveCount(0);
    await expect(page.getByTestId("settings-go-chat")).toHaveCount(0);

    await page.getByLabel("API 密钥").fill("sk-test-e2e-key");
    await page.getByTestId("settings-save").click();

    await expectView(page, "chat");
    await expect(page.getByTestId("chat-input")).toBeVisible();
  });

  test("first-run: save without API key stays on settings", async ({ page }) => {
    await mockTauri(page, { apiKeySet: false });
    await page.goto("/");

    await expect(page.getByTestId("settings-view")).toBeVisible({ timeout: 15_000 });
    await page.getByRole("button", { name: "跳过，去设置" }).click();
    await expect(page.getByTestId("firstrun-wizard")).toHaveCount(0);
    await page.getByTestId("settings-save").click();

    await expect(page.getByTestId("settings-footer-status")).toHaveText(/请先填写 API Key/);
    await expectView(page, "settings");
  });

  test("chat ↔ settings round-trip updates nav singleton", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");

    await expectView(page, "chat");
    await expect(page.locator("#app-loader")).toHaveCount(0, { timeout: 5_000 });
    await page.getByTestId("open-settings").click();

    await expectView(page, "settings");
    await page.getByTestId("settings-back-chat").click();
    await expectView(page, "chat");
  });

  test("typed key alone cannot enter chat until test succeeds", async ({ page }) => {
    await mockTauri(page, { apiKeySet: false });
    await page.goto("/");
    await expect(page.getByTestId("settings-view")).toBeVisible({ timeout: 15_000 });
    await page.getByRole("button", { name: "跳过，去设置" }).click();
    await expect(page.getByTestId("firstrun-wizard")).toHaveCount(0);

    await page.getByLabel("API 密钥").fill("sk-typed-no-save");
    await expect(page.getByTestId("settings-go-chat")).toHaveCount(0);
    await expect(page.getByTestId("settings-back-chat")).toHaveCount(0);

    await page.getByTestId("settings-test").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/已连接/, {
      timeout: 10_000,
    });
    await page.getByTestId("settings-go-chat").click();
    await expectView(page, "chat");
  });

  test("invalid API key: friendly error and block return to chat", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, testConnectionFail: true });
    await page.goto("/");
    await expectView(page, "chat");
    await page.getByTestId("open-settings").click();
    await expectView(page, "settings");
    await page.getByTestId("settings-tab-model").click();

    await expect(page.getByTestId("api-key-stored")).toBeVisible();
    await page.getByLabel("API 密钥").fill("aaa");
    await expect(page.getByTestId("settings-back-chat")).toHaveCount(0);

    await page.getByTestId("settings-test").click();
    const status = page.getByTestId("settings-footer-status");
    await expect(status).toHaveText(/API Key 无效/, { timeout: 10_000 });
    await expect(status).toHaveClass(/is-error/);
    await expect(status).not.toHaveText(/\{|"error"|Authentication Fails/);
    // Failed probe must not wipe the stored-key checkmark (key not persisted).
    await expect(page.getByTestId("api-key-stored")).toBeVisible();
    await expect(page.getByTestId("settings-back-chat")).toHaveCount(0);
    await expect(page.getByTestId("settings-go-chat")).toHaveCount(0);
    await expectView(page, "settings");
    await assertUiHygiene(page);
  });

  test("failed test keeps blocking chat until a later success", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, testConnectionFail: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-model").click();
    await page.getByLabel("API 密钥").fill("bad-key");
    await page.getByTestId("settings-test").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/API Key 无效/, {
      timeout: 10_000,
    });
    // Clear typed key — still blocked because last test failed.
    await page.getByLabel("API 密钥").fill("");
    await expect(page.getByTestId("settings-back-chat")).toHaveCount(0);
    await expectView(page, "settings");
  });

  test("window.__stitchShowChat hook forces chat", async ({ page }) => {
    await mockTauri(page, { apiKeySet: false });
    await page.goto("/");
    await expectView(page, "settings");

    await page.evaluate(() => {
      (window as unknown as { __stitchShowChat?: (r?: string) => void }).__stitchShowChat?.(
        "e2e-hook",
      );
    });
    await expectView(page, "chat");
  });

  test("provider change updates model preset; save returns to chat", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-model").click();

    await page.getByLabel("选择模型提供商").selectOption("openai");
    await expect(page.getByLabel("模型名称")).toHaveValue("gpt-4o");
    await page.getByLabel("选择模型提供商").selectOption("deepseek");
    await expect(page.getByLabel("模型名称")).toHaveValue("deepseek-v4-flash");
    await expect(page.getByTestId("profile-api-base")).toHaveValue(
      "https://api.deepseek.com",
    );

    await page.getByLabel("模型名称").fill("deepseek-v4-pro");
    await page.getByTestId("settings-save").click();

    await expectView(page, "chat");
    await expect(page.getByRole("button", { name: /deepseek-v4-pro/ })).toBeVisible();
  });

  test("multi profile: add second config and switch in chat", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-model").click();
    await expect(page.getByTestId("settings-profile-list")).toBeVisible();

    await page.getByTestId("settings-profile-add").click();
    await page.getByTestId("profile-provider").selectOption("openai");
    await expect(page.getByTestId("profile-model")).toHaveValue("gpt-4o");
    await expect(page.getByTestId("profile-api-base")).toHaveValue(
      "https://api.openai.com/v1",
    );
    await page.getByLabel("API 密钥").fill("sk-openai-e2e");
    await page.getByTestId("settings-test").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/已连接/, {
      timeout: 10_000,
    });
    await page.getByTestId("settings-back-chat").click();
    await expectView(page, "chat");

    await page.getByTestId("model-menu-trigger").click();
    await expect(page.getByTestId("model-menu")).toBeVisible();
    await page
      .getByTestId("model-menu-item")
      .filter({ hasText: /DeepSeek/ })
      .first()
      .click();
    await expect(page.getByTestId("model-menu-trigger")).toHaveText(/DeepSeek · /);
  });

  test("provider zhipu fills base; pasted completions url normalizes", async ({
    page,
  }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-model").click();

    await page.getByTestId("settings-profile-add").click();
    await page.getByTestId("profile-provider").selectOption("zhipu");
    await expect(page.getByTestId("profile-api-base")).toHaveValue(
      "https://open.bigmodel.cn/api/paas/v4",
    );
    await expect(page.getByTestId("profile-model")).toHaveValue("glm-4-flash");

    await page
      .getByTestId("profile-api-base")
      .fill("https://api.openai.com/v1/chat/completions");
    await page.getByTestId("profile-api-base").blur();
    await expect(page.getByTestId("profile-api-base")).toHaveValue(
      "https://api.openai.com/v1",
    );
  });

  test("test connection button updates status", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-model").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();

    await page.getByTestId("settings-test").click();
    const status = page.getByTestId("settings-footer-status");
    await expect(status).toHaveText(/已连接/, { timeout: 10_000 });
    await expect(status).toHaveClass(/is-ok/);
    await expect(page.getByTestId("settings-status-check")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("enter chat recovers from corrupt plan history", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.addInitScript(() => {
      const id = "settings-corrupt-plan";
      const now = Date.now();
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: id,
          sessions: {
            [id]: {
              id,
              title: "坏计划",
              createdAt: now,
              updatedAt: now,
              messages: [
                { id: "u1", type: "message", role: "user", content: "计划" },
                { id: "p1", type: "plan", title: "坏计划卡", phase: "approved" },
              ],
            },
          },
        }),
      );
    });
    await page.goto("/");
    // May land on chat (repaired). Open settings via chrome, then return — proves recovery.
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible({ timeout: 10_000 });
    await page.getByTestId("settings-back-chat").click();
    await expectView(page, "chat");
    await expect(page.getByTestId("diag-error")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("MCP tab: clear token + test connection", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-account").click();
    await expect(page.getByTestId("settings-promptstdio")).toBeVisible();
    await expect(page.getByTestId("settings-mcp-list")).toBeVisible();
    await expect(page.getByRole("button", { name: "设为默认" })).toHaveCount(0);
    await expect(page.getByTestId("api-token-masked")).toBeVisible();
    await expect(page.getByTestId("settings-mcp-chip-meta")).toHaveText("已保存");
    // No developer env pills / workdir in settings
    await expect(page.getByText("本地 8090")).toHaveCount(0);
    await expect(page.getByTestId("settings-workdir")).toHaveCount(0);

    await page.getByTestId("settings-test-promptstdio").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/账号可用/, {
      timeout: 10_000,
    });
    await expect(page.getByTestId("settings-mcp-chip-meta")).toHaveText("可用");

    await page.getByTestId("api-token-clear").click();
    await expect(page.getByTestId("api-token-stored")).toHaveCount(0);
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/已清除/, {
      timeout: 10_000,
    });
    await assertUiHygiene(page);
  });

  test("account tab: probe fail shows 已失效 not 已连接", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, testPromptstdioFail: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-account").click();
    await expect(page.getByTestId("settings-mcp-chip-meta")).toHaveText("已保存");
    await page.getByTestId("settings-test-promptstdio").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/Token 无效/, {
      timeout: 10_000,
    });
    await expect(page.getByTestId("settings-mcp-chip-meta")).toHaveText("已失效");
    await expect(page.getByTestId("settings-mcp-chip-meta")).not.toHaveText(/已连接/);
    await assertUiHygiene(page);
  });

  test("account tab: open website connect writes token", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: false });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-account").click();
    await expect(page.getByTestId("account-connect-banner")).toBeVisible();
    await page.getByTestId("account-connect-web").click();
    await expect(page.getByTestId("api-token-stored")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("account-connect-banner")).toHaveCount(0);
    await expect(page.getByTestId("settings-account-dot")).toHaveCount(0);
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/账号已连接/);
    await expect(page.getByTestId("settings-mcp-chip-meta")).toHaveText("可用");
    await assertUiHygiene(page);
  });

  test("MCP tab: add second account and set default", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-account").click();

    await page.getByTestId("settings-mcp-add").click();
    await expect(page.getByTestId("settings-mcp-draft")).toBeVisible();
    await page.getByTestId("mcp-label").fill("预发");
    await page.getByTestId("api-token-input").fill("pts-staging-token");
    // Persist via test (form Save navigates to chat).
    await page.getByTestId("settings-test-promptstdio").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/账号可用/, {
      timeout: 10_000,
    });

    await expect(page.getByTestId("settings-mcp-list").locator(".profile-chip")).toHaveCount(2);
    await expect(page.getByTestId("settings-mcp-set-default")).toBeVisible();
    await page.getByTestId("settings-mcp-set-default").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/已设为默认/, {
      timeout: 10_000,
    });
    await expect(page.getByTestId("settings-mcp-set-default")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("MCP protocol: add stdio via JSON and test", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-mcp").click();
    await expect(page.getByRole("heading", { name: "MCP" })).toBeVisible();
    await expect(page.getByTestId("settings-server-add")).toBeVisible();

    await page.getByTestId("settings-server-add").click();
    await expect(page.getByTestId("settings-server-draft")).toBeVisible();
    await page.getByTestId("server-json").fill(
      JSON.stringify(
        {
          name: "Filesystem",
          command: "npx",
          args: ["-y", "@modelcontextprotocol/server-filesystem"],
          env: { FOO: "bar" },
        },
        null,
        2,
      ),
    );
    await page.getByTestId("settings-test-mcp-server").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/服务已连接/, {
      timeout: 10_000,
    });
    await expect(page.getByTestId("settings-server-list").locator(".profile-chip")).toHaveCount(1);
    await assertUiHygiene(page);
  });

  test("MCP protocol: import Cursor mcpServers JSON", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-mcp").click();
    await page.getByTestId("settings-server-import").click();
    await expect(page.getByTestId("settings-mcp-import")).toBeVisible();
    await page.getByTestId("server-import-json").fill(
      JSON.stringify({
        mcpServers: {
          github: {
            command: "npx",
            args: ["-y", "@modelcontextprotocol/server-github"],
            env: { GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_x" },
          },
        },
      }),
    );
    await page.getByTestId("settings-server-import-apply").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/已导入/);
    await expect(page.getByTestId("settings-server-github")).toBeVisible();
    await page.getByTestId("settings-server-github").click();
    await expect(page.getByTestId("server-json")).toHaveValue(/GITHUB_PERSONAL_ACCESS_TOKEN/);
    await expect(page.getByTestId("server-json")).toHaveValue(/npx/);
    await assertUiHygiene(page);
  });

  test("MCP protocol: one-click PromptStdio preset stays disabled", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-mcp").click();

    await expect(page.getByTestId("settings-mcp-empty")).toBeVisible();
    await page.getByTestId("settings-server-add-promptstdio").click();
    await expect(page.getByTestId("settings-server-promptstdio")).toBeVisible();
    await expect(page.getByTestId("server-json")).toHaveValue(/\/mcp/);
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/默认停用/);
    await expect(page.getByTestId("settings-server-toggle")).toHaveText("启用");
    await assertUiHygiene(page);
  });

  test("settings tabs: model / account / mcp / general; no workdir module", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("settings-account-dot")).toBeVisible();
    await page.getByTestId("open-settings").click();
    // No account token → gear deep-links to account tab (ADR-033 red-dot).
    await expect(page.getByTestId("settings-tab-account")).toHaveClass(/settings-nav-item-active/);
    await expect(page.getByTestId("settings-sediment-visibility")).toBeVisible();
    await expect(page.getByTestId("sediment-vis-explore")).toHaveAttribute("aria-checked", "true");
    await expect(page.getByTestId("settings-account-dot")).toBeVisible();
    await page.getByTestId("settings-tab-model").click();
    await expect(page.getByTestId("settings-model")).toBeVisible();
    await expect(page.getByTestId("settings-tab-mcp")).toHaveText("MCP");
    await expect(page.getByTestId("settings-tab-system")).toHaveText("通用");
    await page.getByTestId("settings-tab-system").click();
    await expect(page.getByTestId("settings-system")).toBeVisible();
    await expect(page.getByRole("heading", { name: "通用" })).toBeVisible();
    await expect(page.getByText(/工作目录在左侧工作区/)).toBeVisible();
    await expect(page.getByTestId("settings-workdir")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("system tab: theme light / dark / follow system", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.emulateMedia({ colorScheme: "light" });
    await page.addInitScript(() => {
      localStorage.setItem("stitch-theme", "system");
    });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-system").click();

    await expect(page.getByTestId("settings-theme")).toBeVisible();
    await expect(page.getByTestId("settings-theme-light")).toHaveText("浅色");
    await expect(page.getByTestId("settings-theme-dark")).toHaveText("深色");
    await expect(page.getByTestId("settings-theme-system")).toHaveText("跟随系统");
    await expect(page.getByTestId("settings-theme-system")).toHaveAttribute(
      "aria-selected",
      "true",
    );

    await page.getByTestId("settings-theme-dark").click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page.getByTestId("settings-theme-dark")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await page.evaluate(() => localStorage.getItem("stitch-theme"))).toBe("dark");

    await page.getByTestId("settings-theme-light").click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(page.getByTestId("settings-theme-light")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await page.evaluate(() => localStorage.getItem("stitch-theme"))).toBe("light");

    await page.getByTestId("settings-theme-system").click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(page.getByTestId("settings-theme-system")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await page.evaluate(() => localStorage.getItem("stitch-theme"))).toBe("system");

    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("chat-view")).toBeVisible();
    await expect(page.getByTestId("toggle-theme")).toHaveAttribute("data-theme-pref", "system");
    await assertUiHygiene(page);
  });

  test("system tab: check update shows latest (mock)", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-system").click();
    await page.getByTestId("check-update").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/已是最新版本/, {
      timeout: 10_000,
    });
    await expect(page.getByTestId("settings-status-check")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("system tab: check update shows available (mock U1)", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, updateAvailable: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-system").click();
    await page.getByTestId("check-update").click();
    await expect(page.getByTestId("settings-footer-status")).toHaveText(/发现新版本/, {
      timeout: 10_000,
    });
    await expect(page.getByTestId("check-update")).toHaveText(/安装更新/);
    await assertUiHygiene(page);
  });

  test("API Key eye toggle reveals typed plaintext", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-model").click();

    const input = page.getByTestId("api-key-input");
    await expect(input).toHaveValue("");
    const secret = "sk-visible-plaintext-e2e";
    await input.fill(secret);
    await page.getByTestId("api-key-toggle").click();
    await expect(input).toHaveAttribute("type", "text");
    await expect(input).toHaveValue(secret);
    await assertUiHygiene(page);
  });

  test("boot reaches finish_startup path (diag shows revealed)", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("diag-view")).toHaveText(/view=chat/, { timeout: 15_000 });
    // Must progress past splash — last diag should mention reveal or navigate.
    await expect(page.getByTestId("diag-last")).toContainText(/revealed|navigate|bootstrap|config/, {
      timeout: 5_000,
    });
    await expect(page.getByTestId("chat-input")).toBeVisible();
    await assertUiHygiene(page);
  });
});
