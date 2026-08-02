import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Chat core production paths", () => {
  test("send streams tokens then unlocks on done", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamSlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("你好，测一下流式");
    await page.getByTestId("chat-send").click();

    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "停止生成", {
      timeout: 5_000,
    });
    await expect(page.getByTestId("stream-rail")).toBeVisible({ timeout: 5_000 });
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({ timeout: 12_000 });
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });
    await expect(page.getByTestId("stream-rail")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("stop during stream marks stopped and unlocks", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamSlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("请慢慢回复");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "停止生成", {
      timeout: 5_000,
    });
    await page.getByTestId("chat-send").click();
    await expect(page.getByText(/已停止生成/)).toBeVisible({ timeout: 8_000 });
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });
    await assertUiHygiene(page);
  });

  test("stop while tool running clears spinner", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      streamChat: true,
      streamRunningTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("审查本次改动");
    await page.getByTestId("chat-send").click();
    const tool = page.getByTestId("tool-status").first();
    await expect(tool).toBeVisible({ timeout: 5_000 });
    await expect(tool).toHaveAttribute("data-running", "true");
    await expect(tool.getByText("运行中")).toBeVisible();

    await page.getByTestId("chat-send").click();
    await expect(page.getByText(/已停止生成/)).toBeVisible({ timeout: 8_000 });
    await expect(tool).toHaveAttribute("data-running", "false");
    await expect(tool).toHaveAttribute("data-stopped", "true");
    await expect(tool.getByText("已停止")).toBeVisible();
    await expect(tool.locator(".spin")).toHaveCount(0);
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });
    await assertUiHygiene(page);
  });

  test("stop then new message drops aborted user from history", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamSlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("请慢慢审查改动");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "停止生成", {
      timeout: 5_000,
    });
    await page.getByTestId("chat-send").click();
    await expect(page.getByText(/已停止生成/)).toBeVisible({ timeout: 8_000 });

    await page.getByTestId("chat-input").fill("今天日期");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "停止生成", {
      timeout: 5_000,
    });
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 12_000,
    });

    const last = await page.evaluate(() => {
      const i = (window as unknown as { __TAURI_INTERNALS__?: {
        lastSendHistory?: Array<{ role: string; content: string }>;
        lastSendMessage?: string;
      } }).__TAURI_INTERNALS__;
      return {
        message: i?.lastSendMessage ?? "",
        history: i?.lastSendHistory ?? [],
      };
    });
    expect(last.message).toBe("今天日期");
    expect(last.history.map((h) => h.content).join("|")).not.toContain("请慢慢审查改动");
    await assertUiHygiene(page);
  });

  test("stop before first token still shows stopped marker", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamSlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("立刻停");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "停止生成", {
      timeout: 5_000,
    });
    // Cancel before slow tokens land
    await page.getByTestId("chat-send").click();
    await expect(page.getByRole("log").getByText(/已停止生成/)).toBeVisible({ timeout: 8_000 });
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });
    await assertUiHygiene(page);
  });

  test("markdown strips script tags", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamHtml: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("测消毒");
    await page.getByTestId("chat-send").click();
    await expect(page.getByText("安全内容")).toBeVisible({ timeout: 10_000 });
    const scripts = await page.locator(".md-content script").count();
    expect(scripts).toBe(0);
    await assertUiHygiene(page);
  });

  test("plan mode approve runs steps to done", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, planFlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("plan-mode-toggle").check();
    await page.getByTestId("chat-input").fill("列目录并总结");
    await page.getByTestId("chat-send").click();
    const plan = page.getByTestId("plan-card");
    await expect(plan).toBeVisible({ timeout: 8_000 });
    await expect(plan.getByText("待批准")).toBeVisible();
    await expect(page.getByTestId("stream-rail")).toContainText(/计划|批准/);
    await page.getByTestId("plan-approve").click();
    await expect(plan.getByText("已批准")).toBeVisible({ timeout: 5_000 });
    await expect(page.getByText("计划执行完成")).toBeVisible({ timeout: 8_000 });
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });
    await assertUiHygiene(page);
  });

  test("switch session mid-stream cancels and isolates markers", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamSlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("会话甲 marker-A");
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({ timeout: 10_000 });
    const sessionA = await page.locator('[data-testid="session-row"][data-active="true"]').getAttribute(
      "data-session-id",
    );
    expect(sessionA).toBeTruthy();

    await page.getByTestId("session-new").click();
    await page.getByTestId("chat-input").fill("会话乙 marker-B");
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({ timeout: 10_000 });
    const sessionB = await page.locator('[data-testid="session-row"][data-active="true"]').getAttribute(
      "data-session-id",
    );
    expect(sessionB).toBeTruthy();
    expect(sessionB).not.toBe(sessionA);

    await page.locator(`[data-testid="session-row"][data-session-id="${sessionA}"]`).click();
    await page.getByTestId("chat-input").fill("长流式内容 probe-switch");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "停止生成", {
      timeout: 5_000,
    });

    await page.locator(`[data-testid="session-row"][data-session-id="${sessionB}"]`).click();
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 8_000,
    });
    const log = page.getByRole("log");
    await expect(log.locator(".message-user").getByText("marker-B")).toBeVisible();
    await expect(log.getByText("probe-switch")).toHaveCount(0);

    await page.locator(`[data-testid="session-row"][data-session-id="${sessionA}"]`).click();
    await expect(log.getByText(/已停止生成/).first()).toBeVisible({ timeout: 5_000 });
    await expect(log.getByText("probe-switch").first()).toBeVisible();
    await assertUiHygiene(page);
  });

  test("corrupt plan without steps does not throw length error", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.addInitScript(() => {
      const id = "corrupt-plan-session";
      const now = Date.now();
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: id,
          sessions: {
            [id]: {
              id,
              title: "残缺计划",
              createdAt: now,
              updatedAt: now,
              messages: [
                {
                  id: "u1",
                  type: "message",
                  role: "user",
                  content: "请做计划",
                },
                {
                  id: "p1",
                  type: "plan",
                  title: "残缺计划卡",
                  phase: "approved",
                  // intentional: missing steps — production crash was steps.length
                },
              ],
            },
          },
        }),
      );
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText("残缺计划卡")).toBeVisible();
    await expect(page.getByTestId("diag-error")).toHaveCount(0);
    await assertUiHygiene(page);
  });
});
