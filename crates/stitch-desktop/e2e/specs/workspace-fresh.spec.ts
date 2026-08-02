/**
 * Clean-slate workspace tree: no「未绑定」after wiping front-end storage.
 * Run: cd e2e && npm run smoke -- --spec ./specs/workspace-fresh.spec.ts
 */
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import { bootChat, newSession, setWorkDir, shot } from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(__dirname, "../artifacts/workspace-fresh");
const repoRoot = path.resolve(__dirname, "../../../../../");

describe("workspace fresh after cleared storage", () => {
  it("shows named workspace only — never 未绑定", async () => {
    await bootChat();

    await browser.execute(() => {
      for (const k of [
        "stitch-sessions",
        "stitch-workspaces",
        "stitch-workspace-collapse",
        "stitch-recent-dirs",
      ]) {
        localStorage.removeItem(k);
      }
    });
    await browser.refresh();
    await bootChat();

    const set = await setWorkDir(repoRoot);
    expect(set.applied.toLowerCase()).toMatch(/promptstdio/);

    await newSession();
    await browser.pause(500);

    await $('[data-testid="workspace-panel"]').waitForExist({ timeout: 10_000 });

    const unboundCount = await browser.execute(() => {
      const tree = document.querySelector('[data-testid="workspace-list"]');
      return ((tree?.textContent || "").match(/未绑定/g) || []).length;
    });
    expect(unboundCount).toBe(0);

    const groups = await $$('[data-testid="workspace-group"]');
    expect(groups.length).toBeGreaterThanOrEqual(1);

    const labels = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="workspace-row"] .workspace-label')].map(
        (el) => el.textContent?.trim() || "",
      ),
    );
    expect(labels.every((l) => l !== "未绑定")).toBe(true);
    expect(labels.some((l) => /promptstdio/i.test(l))).toBe(true);

    await shot(OUT, "01-fresh-no-unbound");
    expect(await findVisibleUiLeak()).toBeNull();
  });
});
