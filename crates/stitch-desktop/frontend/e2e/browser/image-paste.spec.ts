import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

/** 1x1 transparent PNG (base64) — tiny enough for any paste path. */
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/** Dispatch a synthetic image paste into the composer (Chromium). */
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
      lastSendHistory?: Array<{ role: string; content: string; images?: string[] }>;
    } }).__TAURI_INTERNALS__;
    return {
      message: i?.lastSendMessage ?? "",
      images: i?.lastSendImages ?? [],
      history: i?.lastSendHistory ?? [],
    };
  });
}

test("image entry is visible; unsupported model opens guidance dialog", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  // Entry button is always visible.
  const attach = page.getByTestId("image-attach");
  await expect(attach).toBeVisible();
  // Clicking with a text-only model opens the guidance dialog.
  await attach.click();
  const dialog = page.getByTestId("image-guidance-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("本地视觉模型");
  // Jump to model settings from the dialog.
  await page.getByTestId("image-guidance-open-settings").click();
  await expect(page.getByTestId("settings-tab-model")).toBeVisible({ timeout: 5_000 });
  await assertUiHygiene(page);
});

test("vision model: image entry opens file picker and sends the image", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, streamChat: true, visionModel: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("image-attach").click();
  await page
    .getByTestId("image-file-input")
    .setInputFiles({ name: "shot.png", mimeType: "image/png", buffer: Buffer.from(PNG_B64, "base64") });
  await expect(page.getByTestId("pending-images")).toBeVisible({ timeout: 5_000 });
  await page.getByTestId("chat-input").fill("看看这张截图");
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

test("paste is inert for text-only models (deepseek)", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, streamChat: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await pasteImage(page);
  await expect(page.getByTestId("pending-images")).toHaveCount(0);
  await page.getByTestId("chat-input").fill("普通文本");
  await page.getByTestId("chat-send").click();
  await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
    timeout: 10_000,
  });
  const sent = await lastSend(page);
  expect(sent.images).toEqual([]);
  expect(sent.message).toBe("普通文本");
  await assertUiHygiene(page);
});

test("vision model: paste previews, sends images, bubble shows them", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, streamChat: true, visionModel: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await pasteImage(page);
  const preview = page.getByTestId("pending-images");
  await expect(preview).toBeVisible({ timeout: 5_000 });
  await expect(preview.locator("img")).toHaveCount(1);

  await page.getByTestId("chat-input").fill("看看这张截图");
  await page.getByTestId("chat-send").click();
  await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
    timeout: 10_000,
  });
  // Preview cleared after send.
  await expect(preview).toHaveCount(0);
  const sent = await lastSend(page);
  expect(sent.images.length).toBe(1);
  expect(sent.images[0]).toMatch(/^data:image\/png;base64,/);
  expect(sent.message).toBe("看看这张截图");
  // User bubble carries the image.
  await expect(page.locator('img[data-testid="msg-image"]')).toBeVisible();
  await assertUiHygiene(page);
});

test("image-only message sends without text", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, streamChat: true, visionModel: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await pasteImage(page);
  await expect(page.getByTestId("pending-images")).toBeVisible({ timeout: 5_000 });
  const send = page.getByTestId("chat-send");
  await expect(send).toBeEnabled();
  await send.click();
  await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
    timeout: 10_000,
  });
  const sent = await lastSend(page);
  expect(sent.message).toBe("");
  expect(sent.images.length).toBe(1);
  // A follow-up text send must carry the image-only turn in history
  // (historyForSend keeps messages with images even without text).
  await page.getByTestId("chat-input").fill("接着说");
  await page.getByTestId("chat-send").click();
  await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
    timeout: 10_000,
  });
  const sent2 = await lastSend(page);
  expect(sent2.history.some((h) => (h.images?.length ?? 0) > 0)).toBe(true);
  expect(sent2.message).toBe("接着说");
  await assertUiHygiene(page);
});

test("removing the preview re-disables send", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, visionModel: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await pasteImage(page);
  await expect(page.getByTestId("pending-images")).toBeVisible({ timeout: 5_000 });
  await page.getByTestId("pending-image-remove-0").click();
  await expect(page.getByTestId("pending-images")).toHaveCount(0);
  await expect(page.getByTestId("chat-send")).toBeDisabled();
  await assertUiHygiene(page);
});

test("edit-resend restores the image previews", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, streamChat: true, visionModel: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await pasteImage(page);
  await expect(page.getByTestId("pending-images")).toBeVisible({ timeout: 5_000 });
  await page.getByTestId("chat-input").fill("带图消息");
  await page.getByTestId("chat-send").click();
  await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
    timeout: 10_000,
  });
  // Edit the user bubble → previews come back into the composer.
  await page.locator(".message-user [aria-label='编辑']").first().click();
  await expect(page.getByTestId("pending-images")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("pending-images").locator("img")).toHaveCount(1);
  await expect(page.getByTestId("chat-input")).toHaveValue("带图消息");
  await assertUiHygiene(page);
});
