import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

async function pasteImage(page: Page) {
  await page.getByTestId("chat-input").evaluate((el, b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([bytes], "shot.png", { type: "image/png" }));
    el.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }),
    );
  }, PNG_B64);
}

async function lastSend(page: Page) {
  return page.evaluate(() => {
    const i = (window as unknown as { __TAURI_INTERNALS__?: {
      lastSendMessage?: string;
      lastSendImages?: string[];
    } }).__TAURI_INTERNALS__;
    return { message: i?.lastSendMessage ?? "", images: i?.lastSendImages ?? [] };
  });
}

test("local vision on: deepseek can paste, preview, and send images", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, streamChat: true, localVision: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  // Paste opens the preview instead of the guidance dialog.
  await pasteImage(page);
  await expect(page.getByTestId("pending-images")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("image-guidance-dialog")).toHaveCount(0);
  await page.getByTestId("chat-input").fill("描述这张图");
  await page.getByTestId("chat-send").click();
  await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
    timeout: 10_000,
  });
  const sent = await lastSend(page);
  expect(sent.images.length).toBe(1);
  expect(sent.images[0]).toMatch(/^data:image\/png;base64,/);
  await expect(page.locator('img[data-testid="msg-image"]')).toBeVisible();
  await assertUiHygiene(page);
});

test("local vision off: deepseek still gets the guidance dialog", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await pasteImage(page);
  await expect(page.getByTestId("pending-images")).toHaveCount(0);
  await page.getByTestId("image-attach").click();
  await expect(page.getByTestId("image-guidance-dialog")).toBeVisible();
  await assertUiHygiene(page);
});

async function dropImage(page: Page) {
  await page.evaluate((b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([bytes], "drop.png", { type: "image/png" }));
    const opts = { dataTransfer: dt, bubbles: true, cancelable: true };
    window.dispatchEvent(new DragEvent("dragenter", opts));
    window.dispatchEvent(new DragEvent("dragover", opts));
    window.dispatchEvent(new DragEvent("drop", opts));
  }, PNG_B64);
}

test("drag & drop adds images with local vision on", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, streamChat: true, localVision: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await dropImage(page);
  await expect(page.getByTestId("pending-images")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("pending-images").locator("img")).toHaveCount(1);
  await page.getByTestId("chat-input").fill("拖进来的图");
  await page.getByTestId("chat-send").click();
  await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
    timeout: 10_000,
  });
  const sent = await page.evaluate(() => {
    const i = (window as unknown as { __TAURI_INTERNALS__?: { lastSendImages?: string[] } })
      .__TAURI_INTERNALS__;
    return i?.lastSendImages ?? [];
  });
  expect(sent.length).toBe(1);
  await assertUiHygiene(page);
});

test("drag & drop without vision opens guidance instead of preview", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await dropImage(page);
  await expect(page.getByTestId("image-guidance-dialog")).toBeVisible();
  await expect(page.getByTestId("pending-images")).toHaveCount(0);
  await assertUiHygiene(page);
});
