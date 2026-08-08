import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// ADR-037: run_command live output — lines appear inside the running tool
// card without waiting for the command to finish.

test.describe("tool live output", () => {
  test("running tool card shows streaming lines and pins to the latest", async ({
    page,
  }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamToolOutput: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("安装依赖");
    await page.getByTestId("chat-input").press("Enter");

    // Tool card is running…
    const toolCard = page.getByTestId("tool-status");
    await expect(toolCard).toBeVisible({ timeout: 5_000 });

    // 默认只显示单行实时尾巴（高度稳定不抢滚动）——完整输出不自动展开。
    const tail = page.getByTestId("tool-live-tail");
    await expect(tail).toBeVisible({ timeout: 5_000 });
    await expect(page.getByTestId("tool-live-output")).toHaveCount(0);
    await expect(page.getByTestId("tool-progress")).toBeVisible();

    // 点击卡片头展开 → 完整 live 输出流式出现（ADR-037）并钉在最新行。
    await toolCard.locator(".tool-call-main").click();
    const live = page.getByTestId("tool-live-output");
    await expect(live).toBeVisible({ timeout: 5_000 });
    await expect(live).toContainText("resolve dep 1/40", { timeout: 5_000 });
    await expect(live).toContainText("linking done in 3.2s", { timeout: 8_000 });
    const tailVisible = await live.evaluate((el) => {
      const body = el.querySelector(".tool-shell-body") as HTMLElement | null;
      if (!body) return false;
      return body.scrollHeight - body.scrollTop - body.clientHeight < 4;
    });
    expect(tailVisible).toBe(true);
    // 再点收起 → 回到单行尾巴（切换往返确定状态，完成态折叠时才从收起出发）。
    await toolCard.locator(".tool-call-main").click();
    await expect(live).toHaveCount(0);
    await expect(tail).toBeVisible();

    // After done, the card carries the full final output and stops spinning.
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "安装完成" }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("tool-progress")).toHaveCount(0);
    await expect(toolCard).toHaveAttribute("data-running", "false");

    // Benchmark metrics ride along structured (ToolResult.metrics → tool_done).
    await expect(toolCard).toHaveAttribute("data-metrics", /"duration_ms":4123.5/);

    // Final detail (collapsed chip) expands to the complete output.
    await toolCard.locator(".tool-call-main").click();
    await expect(page.getByTestId("tool-shell")).toContainText("Done in 4.1s");
    await expect(page.getByTestId("tool-shell")).toContainText("Downloading packages");

    await assertUiHygiene(page);
  });
});
