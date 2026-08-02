import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Agent anti-interruption UX", () => {
    test("iteration-cap done offers 继续执行 and resumes", async ({ page }) => {
        await mockTauri(page, { apiKeySet: true, streamChat: true, streamCapDone: true });
        await page.goto("/");
        await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

        await page.getByTestId("chat-input").fill("跑一个长任务");
        await page.getByTestId("chat-send").click();

        const continueBtn = page.getByTestId("msg-continue").first();
        await expect(continueBtn).toBeVisible({ timeout: 12_000 });
        await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
            timeout: 5_000,
        });

        await continueBtn.click();
        // 继续执行 becomes the new user turn and streaming resumes.
        await expect(page.locator(".message-user").filter({ hasText: "继续执行" })).toBeVisible({
            timeout: 5_000,
        });
        await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "停止生成", {
            timeout: 5_000,
        });
        await assertUiHygiene(page);
    });

    test("running tool chip ticks per-tool elapsed", async ({ page }) => {
        await mockTauri(page, {
            apiKeySet: true,
            streamChat: true,
            streamRunningTool: true,
        });
        await page.goto("/");
        await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });

        await page.getByTestId("chat-input").fill("跑一个长命令");
        await page.getByTestId("chat-send").click();
        const elapsed = page.getByTestId("tool-elapsed").first();
        await expect(elapsed).toBeVisible({ timeout: 6_000 });

        // Tick from tool start (not turn start): first reading stays small.
        const firstText = (await elapsed.innerText()).trim();
        expect(firstText).toMatch(/^\d+s$|^\d+m \d{2}s$/);

        // Keeps ticking while no events arrive (fake aliveness for long commands).
        await page.waitForTimeout(2_200);
        const laterText = (await elapsed.innerText()).trim();
        expect(laterText).toMatch(/^\d+s$|^\d+m \d{2}s$/);
        expect(laterText).not.toBe(firstText);

        await page.getByTestId("chat-send").click();
        await expect(page.getByText(/已停止生成/)).toBeVisible({ timeout: 8_000 });
        await assertUiHygiene(page);
    });
});
