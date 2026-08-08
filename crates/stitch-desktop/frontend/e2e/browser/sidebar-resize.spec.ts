import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// 左右侧分割线：拖动调宽（clamp 200–480）、双击还原、键盘微调、
// 宽度持久化、折叠时不可拖。

test.describe("侧栏分割线拖动", () => {
  test("默认宽度 256，可拖到 380 并持久化", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const sidebar = page.getByTestId("chat-sidebar");
    await expect(sidebar).toBeVisible();
    const w0 = (await sidebar.boundingBox())!.width;
    expect(w0).toBeGreaterThan(250);
    expect(w0).toBeLessThan(270);

    // 拖动分割线 +120px。
    const resizer = page.getByTestId("sidebar-resizer");
    const box = (await resizer.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2, { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);

    const w1 = (await sidebar.boundingBox())!.width;
    expect(w1).toBeGreaterThan(370);
    expect(w1).toBeLessThan(390);

    // 持久化：重载后保持。
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    const w2 = (await page.getByTestId("chat-sidebar").boundingBox())!.width;
    expect(w2).toBeGreaterThan(370);
    expect(w2).toBeLessThan(390);

    await assertUiHygiene(page);
  });

  test("拖动 clamp 到边界（≤200 / ≥480）", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const sidebar = page.getByTestId("chat-sidebar");
    const resizer = page.getByTestId("sidebar-resizer");
    const box = (await resizer.boundingBox())!;

    // 向左猛拖 → 停在 200。
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x - 400, box.y + box.height / 2, { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);
    expect((await sidebar.boundingBox())!.width).toBeLessThanOrEqual(202);

    // 向右猛拖 → 停在 480（宽度已变——重新取 resizer 当前位置）。
    const box2 = (await resizer.boundingBox())!;
    await page.mouse.move(box2.x + box2.width / 2, box2.y + box2.height / 2);
    await page.mouse.down();
    await page.mouse.move(box2.x + 900, box2.y + box2.height / 2, { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);
    expect((await sidebar.boundingBox())!.width).toBeGreaterThanOrEqual(478);

    await assertUiHygiene(page);
  });

  test("双击还原默认宽度；键盘 ←/→ 微调", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const sidebar = page.getByTestId("chat-sidebar");
    const resizer = page.getByTestId("sidebar-resizer");
    const box = (await resizer.boundingBox())!;

    // 先拖宽。
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 100, box.y + box.height / 2, { steps: 4 });
    await page.mouse.up();
    await page.waitForTimeout(250);
    expect((await sidebar.boundingBox())!.width).toBeGreaterThan(350);

    // 双击还原 256（等宽度过渡结束再测）。
    await resizer.dblclick();
    await page.waitForTimeout(250);
    const wReset = (await sidebar.boundingBox())!.width;
    expect(wReset).toBeGreaterThan(250);
    expect(wReset).toBeLessThan(270);

    // 键盘 → 加宽 16px。
    await resizer.focus();
    await page.keyboard.press("ArrowRight");
    await page.waitForTimeout(250);
    const wPlus = (await sidebar.boundingBox())!.width;
    expect(wPlus).toBeGreaterThan(wReset + 10);

    await assertUiHygiene(page);
  });

  test("窄窗口下上限随窗口（45% 视口，不挤垮主区）", async ({ page }) => {
    // 800×600 → 45% = 360px 上限。
    await page.setViewportSize({ width: 800, height: 600 });
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const sidebar = page.getByTestId("chat-sidebar");
    const resizer = page.getByTestId("sidebar-resizer");
    const box = (await resizer.boundingBox())!;

    // 向右猛拖 → 停在 360（而非固定 480）。
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 900, box.y + box.height / 2, { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);
    const w = (await sidebar.boundingBox())!.width;
    expect(w).toBeGreaterThanOrEqual(358);
    expect(w).toBeLessThanOrEqual(362);

    await assertUiHygiene(page);
  });

  test("折叠侧栏后分割线消失", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await expect(page.getByTestId("sidebar-resizer")).toBeVisible();
    // 侧栏头部「收起侧栏」按钮。
    await page.getByRole("button", { name: "收起侧栏" }).click();
    await expect(page.getByTestId("sidebar-resizer")).toHaveCount(0);
    // 折叠后 w-0 + border-r(border-box) 残留 1px 边框，Playwright 判可见——
    // 用 opacity 0 断言视觉隐藏。
    await expect(page.getByTestId("chat-sidebar")).toHaveCSS("opacity", "0");

    await assertUiHygiene(page);
  });
});
