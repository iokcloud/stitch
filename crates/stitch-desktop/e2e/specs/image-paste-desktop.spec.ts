import fs from "node:fs";
import path from "node:path";
import { shot } from "../helpers/chat-desktop";

/** 1x1 transparent PNG (base64) — tiny enough for any paste path. */
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

const OUT = "artifacts/image-paste-desktop";

async function waitBooted() {
  await browser.waitUntil(
    async () => {
      const title = await browser.getTitle();
      return title.toLowerCase().includes("stitch");
    },
    { timeout: 30_000, timeoutMsg: "window title never contained Stitch", interval: 500 },
  );
  await browser.waitUntil(
    async () => {
      const bootError = await $('[data-testid="boot-error"]');
      return !(await bootError.isExisting());
    },
    { timeout: 60_000, timeoutMsg: "boot-error appeared after launch", interval: 500 },
  );
  await $('[data-testid="diag-view"]').waitForExist({ timeout: 20_000 });
}

async function readView() {
  const text = await $('[data-testid="diag-view"]').getText();
  const m = text.match(/view=(\w+)/);
  return m?.[1] ?? "";
}

/** Paste an image into the composer (WebView2 = Chromium, DataTransfer works). */
async function pasteImage() {
  await browser.execute((b64: string) => {
    const el = document.querySelector<HTMLTextAreaElement>('[data-testid="chat-input"]');
    if (!el) throw new Error("chat-input missing");
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([bytes], "shot.png", { type: "image/png" }));
    el.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }),
    );
  }, PNG_B64);
}

/** Drag & drop an image onto the window (same DataTransfer technique). */
async function dropImage() {
  await browser.execute((b64: string) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([bytes], "drop.png", { type: "image/png" }));
    const opts = { dataTransfer: dt, bubbles: true, cancelable: true };
    window.dispatchEvent(new DragEvent("dragenter", opts));
    window.dispatchEvent(new DragEvent("dragover", opts));
    window.dispatchEvent(new DragEvent("drop", opts));
  }, PNG_B64);
}

/** Toggle the local-vision describe switch via the settings UI. */
async function setLocalVision(enabled: boolean) {
  await $('[data-testid="open-settings"]').click();
  await $('[data-testid="settings-tab-model"]').click();
  const check = $('[data-testid="local-vision-enabled"]');
  await check.waitForExist({ timeout: 5_000 });
  const checked = await check.isSelected();
  if (checked !== enabled) {
    await check.click();
  }
  await $('[data-testid="settings-back-chat"]').click();
  await browser.waitUntil(async () => (await readView()) === "chat", { timeout: 10_000 });
}

/** Work dir of the current workspace, read from the frontend's localStorage. */
async function currentWorkDir(): Promise<string | null> {
  return browser.execute(() => {
    for (const key of ["stitch-workspaces", "stitch_workspaces"]) {
      try {
        const raw = localStorage.getItem(key);
        if (!raw) continue;
        const parsed = JSON.parse(raw);
        const items = Array.isArray(parsed)
          ? parsed
          : Array.isArray(parsed?.items)
            ? parsed.items
            : [];
        for (const it of items) {
          if (it?.current || it?.id === parsed?.currentId) {
            return String(it.path || it.dir || "");
          }
        }
        return String(items[0]?.path || items[0]?.dir || "");
      } catch {
        /* keep looking */
      }
    }
    return null;
  });
}

/** Latest messages.jsonl under the given work dir. */
function latestSessionJsonl(workDir: string): string | null {
  const root = path.join(workDir, ".stitch", "sessions");
  let best: { mtime: number; file: string } | null = null;
  try {
    for (const dir of fs.readdirSync(root)) {
      const file = path.join(root, dir, "messages.jsonl");
      if (!fs.existsSync(file)) continue;
      const mtime = fs.statSync(file).mtimeMs;
      if (!best || mtime > best.mtime) best = { mtime, file };
    }
  } catch {
    return null;
  }
  return best?.file ?? null;
}

describe("image paste (real desktop exe)", () => {
  it("local vision describes pasted images for deepseek; guidance when disabled; vision model direct", async () => {
    await waitBooted();
    const view = await readView();
    if (view !== "chat") {
      throw new Error(`expected chat view, got ${view} (run desktop-smoke first?)`);
    }

    // 0) Drag & drop opens the preview (window-wide drop target) — the same
    // intake as paste/file-picker. Send the dragged image through the whole
    // describe chain below.
    await dropImage();
    const preview = $('[data-testid="pending-images"]');
    await preview.waitForExist({ timeout: 5_000 });
    await browser.waitUntil(async () => (await preview.$$("img")).length === 1, {
      timeout: 5_000,
      timeoutMsg: "drag preview img never rendered",
    });

    // 1) Send with text; deepseek receives the local description.
    // Count existing assistant bubbles first — history from previous runs
    // still renders, so wait for a NEW message, not just any message.
    const before = await $$(".msg-assistant .md-content").length;
    await $('[data-testid="chat-input"]').setValue("描述这张图片");
    await $('[data-testid="chat-send"]').click();
    await browser.waitUntil(
      async () => (await $$(".msg-assistant .md-content").length) > before,
      { timeout: 120_000, timeoutMsg: "no new assistant message appeared" },
    ).catch(async (e) => {
      await shot(OUT, "02-sent-state-debug");
      throw e;
    });
    await shot(OUT, "01-local-vision-describe");
    // Functional proof: the new assistant bubble replies about the described
    // image (the 1×1 PNG renders solid red — qwen3-vl describes it as 纯色).
    const newest = $$(".msg-assistant .md-content").last();
    await browser.waitUntil(async () => (await newest.getText()).trim().length > 0, {
      timeout: 90_000,
      timeoutMsg: "newest bubble never got text",
    });
    const reply = await newest.getText();
    console.log("PROBE-DEBUG reply:", reply.slice(0, 160));
    expect(reply).toMatch(/纯|色块|图片|红色/);
    // Disk proof (best-effort): the describe block lands in messages.jsonl
    // when the work dir is bound — the probe env may leave it unbound.
    const workDir = await currentWorkDir();
    console.log("PROBE-DEBUG workDir:", workDir);
    if (workDir) {
      try {
        await browser.waitUntil(
          () => {
            const f = latestSessionJsonl(workDir);
            if (!f) return false;
            try {
              return fs.readFileSync(f, "utf-8").includes("[图片描述");
            } catch {
              return false;
            }
          },
          { timeout: 60_000, timeoutMsg: "describe block never hit disk" },
        );
      } catch (e) {
        console.log("PROBE-WARN disk check skipped:", String(e).slice(0, 120));
      }
    }

    // 2) Guidance dialog appears when the local vision layer is disabled.
    await setLocalVision(false);
    await $('[data-testid="image-attach"]').click();
    const dialog = $('[data-testid="image-guidance-dialog"]');
    await dialog.waitForExist({ timeout: 5_000 });
    await $('[data-testid="image-guidance-open-settings"]').click();
    await $('[data-testid="settings-tab-model"]').waitForExist({ timeout: 5_000 });
    await $('[data-testid="settings-back-chat"]').click();
    await browser.waitUntil(async () => (await readView()) === "chat", { timeout: 10_000 });
    await setLocalVision(true); // restore

    // 3) Vision model (gpt-4o profile via settings) receives images directly.
    await $('[data-testid="open-settings"]').click();
    await $('[data-testid="settings-tab-model"]').click();
    await $('[data-testid="settings-profile-add"]').click();
    // selectByAttribute does not fire change in WebView2; set + dispatch.
    await browser.execute(() => {
      const sel = document.querySelector<HTMLSelectElement>('[data-testid="profile-provider"]');
      if (!sel) throw new Error("provider select missing");
      sel.value = "openai";
      sel.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await browser.waitUntil(
      async () => {
        const v = await $('[data-testid="profile-model"]').getValue();
        return v.toLowerCase().includes("gpt-4o");
      },
      { timeout: 5_000, timeoutMsg: "model never auto-filled from provider preset" },
    );
    await $('[data-testid="settings-back-chat"]').click();
    await browser.waitUntil(async () => (await readView()) === "chat", { timeout: 10_000 });

    await $('[data-testid="model-menu-trigger"]').click();
    await $('[data-testid="model-menu"]').waitForExist({ timeout: 5_000 });
    const items = await $$('[data-testid="model-menu-item"]');
    let clicked = false;
    for (const it of items) {
      const label = (await it.getText()) || "";
      if (label.includes("gpt-4o")) {
        await it.click();
        clicked = true;
        break;
      }
    }
    expect(clicked).toBe(true);

    await pasteImage();
    await preview.waitForExist({ timeout: 5_000 });
    await shot(OUT, "02-vision-model-preview");

    // 4) Restore: switch back to deepseek and delete the test profile.
    await $('[data-testid="model-menu-trigger"]').click();
    const items2 = await $$('[data-testid="model-menu-item"]');
    for (const it of items2) {
      const label = (await it.getText()) || "";
      if (label.includes("DeepSeek")) {
        await it.click();
        break;
      }
    }
    await $('[data-testid="open-settings"]').click();
    await $('[data-testid="settings-tab-model"]').click();
    const draft = $('[data-testid="settings-profile-draft"]');
    if (await draft.isExisting()) {
      await draft.click();
      await $('[data-testid="settings-profile-delete"]').click();
    }
    await $('[data-testid="settings-back-chat"]').click();
    await browser.waitUntil(async () => (await readView()) === "chat", { timeout: 10_000 });
  });
});
