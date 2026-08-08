/**
 * Real-exe full chain: account chip 已保存 → 测试连接 → 可用|已失效 (never 已连接-only).
 * Run: cd e2e && npm run account-chip-probe
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import { waitBooted, shot } from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(__dirname, "../artifacts/account-chip-probe");

async function readView() {
  const text = await $('[data-testid="diag-view"]').getText();
  const m = text.match(/view=(\w+)/);
  return m?.[1] ?? "";
}

async function openSettings() {
  if ((await readView()) === "settings") return;
  const gear = await $('[data-testid="open-settings"]');
  if (!(await gear.isExisting())) {
    // First-run settings already open.
    return;
  }
  await gear.click();
  await browser.waitUntil(async () => (await readView()) === "settings", {
    timeout: 10_000,
    timeoutMsg: "open-settings failed",
  });
}

function writeReport(payload: Record<string, unknown>) {
  fs.mkdirSync(OUT, { recursive: true });
  fs.writeFileSync(path.join(OUT, "PROBE-REPORT.json"), JSON.stringify(payload, null, 2));
}

describe("account chip probe real exe", () => {
  it("chip 已保存 → test → 可用 or 已失效 aligned with footer", async () => {
    await waitBooted();
    await openSettings();
    await $('[data-testid="settings-tab-account"]').click();
    await $('[data-testid="settings-promptstdio"]').waitForExist({ timeout: 10_000 });
    await shot(OUT, "01-account-open");

    const chipMeta = async () => {
      const active = await $('.profile-chip-active [data-testid="settings-mcp-chip-meta"]');
      if (await active.isExisting()) return (await active.getText()).trim();
      const any = await $('[data-testid="settings-mcp-chip-meta"]');
      return (await any.isExisting()) ? (await any.getText()).trim() : "";
    };

    const hasToken = await $('[data-testid="api-token-stored"]').isExisting();
    const banner = await $('[data-testid="account-connect-banner"]').isExisting();
    const chipBefore = await chipMeta();

    expect(chipBefore).not.toMatch(/已连接/);

    if (!hasToken) {
      expect(banner).toBe(true);
      expect(chipBefore === "" || chipBefore === "未设置" || chipBefore === "未保存").toBe(true);
      await shot(OUT, "02-no-token-banner");
      writeReport({
        verdict: "PASS_NO_TOKEN",
        chipBefore,
        note: "本机无 Token：展示连接横幅；未跑探测（无矛盾态）",
      });
      expect(await findVisibleUiLeak()).toBeNull();
      return;
    }

    expect(chipBefore).toBe("已保存");
    await shot(OUT, "02-chip-saved");

    await $('[data-testid="settings-test-promptstdio"]').click();
    await browser.waitUntil(
      async () => {
        const status = await $('[data-testid="settings-footer-status"]');
        if (!(await status.isExisting())) return false;
        const t = (await status.getText()).trim();
        return /账号可用|Token 无效|账号连接失败/.test(t);
      },
      { timeout: 45_000, timeoutMsg: "probe footer never settled", interval: 400 },
    );

    const footer = (await $('[data-testid="settings-footer-status"]').getText()).trim();
    const chipAfter = await chipMeta();
    await shot(OUT, "03-after-probe");

    expect(chipAfter).not.toMatch(/已连接/);

    if (/账号可用/.test(footer)) {
      expect(chipAfter).toBe("可用");
      writeReport({ verdict: "PASS_OK", chipBefore, chipAfter, footer });
    } else if (/Token 无效|账号连接失败/.test(footer)) {
      expect(chipAfter).toBe("已失效");
      writeReport({ verdict: "PASS_INVALID", chipBefore, chipAfter, footer });
    } else {
      writeReport({ verdict: "FAIL_UNEXPECTED", chipBefore, chipAfter, footer });
      throw new Error(`unexpected footer after probe: ${footer} · chip=${chipAfter}`);
    }

    expect(await findVisibleUiLeak()).toBeNull();
  });
});
