import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Agent UX polish (mock)", () => {
  test("tool call chip has no checkmark glyph and uses SVG", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, planFlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("plan-mode-toggle").check();
    await page.getByTestId("chat-input").fill("列目录");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("plan-card")).toBeVisible({ timeout: 8_000 });
    await page.getByTestId("plan-approve").click();
    await expect(page.getByText("计划执行完成")).toBeVisible({ timeout: 8_000 });

    // No unicode checkmark / folder emoji in tool/plan chrome
    const dirty = await page.evaluate(() => {
      const roots = [
        ...document.querySelectorAll('[data-testid="tool-status"]'),
        ...document.querySelectorAll('[data-testid="plan-card"]'),
      ];
      const banned = /[✓✔✅❌📁📄]/;
      return roots.map((r) => r.textContent || "").find((t) => banned.test(t)) ?? null;
    });
    expect(dirty).toBeNull();
    await assertUiHygiene(page);
  });

  test("multiline input grows composer then sends", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true });
    await page.goto("/");
    const input = page.getByTestId("chat-input");
    await expect(input).toBeVisible({ timeout: 15_000 });
    const h0 = await input.evaluate((el) => el.getBoundingClientRect().height);
    const multi = ["第一行", "第二行 alpha", "第三行 beta", "请回复：收到多行"].join("\n");
    await input.fill(multi);
    await input.dispatchEvent("input");
    const h1 = await input.evaluate((el) => el.getBoundingClientRect().height);
    expect(h1).toBeGreaterThan(h0 + 10);
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    // User bubble (not sediment title which also echoes the prompt)
    await expect(page.getByRole("log").locator(".whitespace-pre-wrap").filter({ hasText: "alpha" })).toBeVisible();
    await expect(page.getByTestId("sediment-card")).toHaveCount(0);
    await page.getByTestId("msg-sediment").click();
    await expect(page.getByTestId("sediment-card")).toBeVisible();
    await page.getByTestId("session-more").first().click();
    await expect(page.getByTestId("session-copy-title")).toBeVisible();
    await expect(page.getByTestId("session-rename-btn")).toBeVisible();
    await expect(page.getByTestId("session-rollback-checkpoint")).toBeVisible();
    await expect(page.getByTestId("session-delete")).toBeVisible();
    await page.getByTestId("session-rollback-checkpoint").click();
    await expect(page.getByTestId("checkpoint-dialog")).toBeVisible();
    await expect(page.getByTestId("checkpoint-list")).toBeVisible();
    await expect(page.getByTestId("checkpoint-current")).toBeVisible();
    await expect(page.getByTestId("checkpoint-epoch-1")).toBeVisible();
    await expect(page.getByTestId("checkpoint-diff")).toContainText("检查点");
    await page.getByTestId("checkpoint-cancel").click();
    await expect(page.getByTestId("checkpoint-dialog")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("long assistant content shows expand control", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      streamChat: true,
      streamHtml: false,
    });
    // Inject a long assistant reply via localStorage session then reload
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    const longBody = Array.from({ length: 40 }, (_, i) => `段落 ${i + 1}：这是用于测试长内容折叠的句子，重复填充以保证超过折叠阈值。`).join(
      "\n\n",
    );
    await page.evaluate((body) => {
      const id = "long-msg-session";
      const now = Date.now();
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: id,
          sessions: {
            [id]: {
              id,
              title: "长内容",
              createdAt: now,
              updatedAt: now,
              messages: [
                { id: "u1", type: "message", role: "user", content: "写长文" },
                { id: "a1", type: "message", role: "assistant", content: body },
              ],
            },
          },
        }),
      );
    }, longBody);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByRole("button", { name: "展开全文" })).toBeVisible({ timeout: 5_000 });

    // Foot bar overlays the clip edge so sliced glyphs stay under opaque fill.
    const clampLayout = await page.evaluate(() => {
      const body = document.querySelector(".msg-body.is-clamped") as HTMLElement | null;
      const clamp = document.querySelector(".message-clamp") as HTMLElement | null;
      const bar = document.querySelector(".message-clamp-bar") as HTMLElement | null;
      if (!body || !clamp || !bar) return { ok: false, reason: "missing clamp shell" };
      const afterContent = getComputedStyle(clamp, "::after").content;
      const hasCoveringAfter =
        !!afterContent && afterContent !== "none" && afterContent !== '""' && afterContent !== "normal";
      const barCs = getComputedStyle(bar);
      const bodyBox = body.getBoundingClientRect();
      const barBox = bar.getBoundingClientRect();
      const overlaysFoot =
        barCs.position === "absolute" &&
        Math.abs(barBox.bottom - bodyBox.bottom) < 2 &&
        barBox.height >= 28;
      return {
        ok: !hasCoveringAfter && overlaysFoot,
        hasCoveringAfter,
        overlaysFoot,
        barPosition: barCs.position,
        clampMaxH: getComputedStyle(clamp).maxHeight,
      };
    });
    expect(clampLayout.ok, JSON.stringify(clampLayout)).toBe(true);

    // Full-width bar hit target (not only the chevron glyph).
    const barBox = await page.locator(".message-clamp-bar").boundingBox();
    expect(barBox).toBeTruthy();
    expect(barBox!.width).toBeGreaterThan(200);
    await page.locator('[data-testid="message-expand"]').click({ position: { x: Math.floor(barBox!.width * 0.7), y: 12 } });
    // exact: avoid matching sidebar「收起侧栏」
    await expect(page.getByRole("button", { name: "收起", exact: true })).toBeVisible();
    await assertUiHygiene(page);
  });

  test("stream rail shows summary, elapsed, format and progress", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamSlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("慢慢回复");
    await page.getByTestId("chat-send").click();
    const rail = page.getByTestId("stream-rail");
    await expect(rail).toBeVisible({ timeout: 5_000 });
    await expect(page.getByTestId("stream-summary")).toBeVisible();
    await expect(page.getByTestId("stream-elapsed")).toBeVisible();
    await expect(page.getByTestId("stream-format")).toBeVisible();
    await expect(page.getByTestId("stream-progress")).toBeVisible();
    await expect(page.getByTestId("stream-usage")).toBeVisible();
    await expect(page.getByTestId("stream-tokens")).toContainText("tok");
    await expect(page.getByTestId("stream-context")).toContainText("Ctx");
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({ timeout: 15_000 });
    await assertUiHygiene(page);
  });

  test("usage meter shows context and turn tokens after stream", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("usage-meter")).toBeVisible();
    await expect(page.getByTestId("usage-context")).toContainText("Ctx");
    await page.getByTestId("chat-input").fill("用量探针");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("stream-usage")).toBeVisible({ timeout: 5_000 });
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.getByTestId("usage-turn")).toContainText("本轮");
    await expect(page.getByTestId("usage-iters")).toContainText("次调用");
    await expect(page.getByTestId("usage-context")).toContainText(/Ctx\s+\d/);
    // No layering happened in this mock → no segmented indicator.
    await expect(page.getByTestId("usage-layers")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("usage meter shows layered hot/warm/cold segments after compact", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, usageLayers: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("分层探针");
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    const layered = page.getByTestId("usage-layers");
    await expect(layered).toBeVisible();
    await expect(layered.locator("i")).toHaveCount(3);
    // Title detail: 热/温/冷 + archived entry count.
    const title = await page.getByTestId("usage-meter").getAttribute("title");
    expect(title).toContain("热");
    expect(title).toContain("温");
    expect(title).toContain("冷");
    expect(title).toContain("归档 5 条");
    await assertUiHygiene(page);
  });

  test("code block offers copy icon; links styled as links", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    const body = [
      "见文档 https://example.com/docs",
      "",
      "```python",
      "print('hello')",
      "```",
    ].join("\n");
    await page.evaluate((content) => {
      const id = "fmt-session";
      const now = Date.now();
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: id,
          sessions: {
            [id]: {
              id,
              title: "格式",
              createdAt: now,
              updatedAt: now,
              messages: [
                { id: "u1", type: "message", role: "user", content: "格式" },
                { id: "a1", type: "message", role: "assistant", content },
              ],
            },
          },
        }),
      );
    }, body);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.locator(".md-code-copy")).toBeVisible();
    await expect(page.locator("a.md-link")).toHaveAttribute("href", "https://example.com/docs");
    await page.locator(".md-code-copy").click();
    await expect(page.locator(".md-code-copy.is-copied")).toBeVisible({ timeout: 2_000 });
    await assertUiHygiene(page);
  });

  test("code and tool blocks scroll inside capped height", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, planFlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const longCode = Array.from({ length: 60 }, (_, i) => `line_${i + 1} = ${i * i}`).join("\n");
    const md = `说明\n\n\`\`\`python\n${longCode}\n\`\`\`\n`;
    await page.evaluate((body) => {
      const id = "block-scroll-session";
      const now = Date.now();
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: id,
          sessions: {
            [id]: {
              id,
              title: "区块滚动",
              createdAt: now,
              updatedAt: now,
              messages: [
                { id: "u1", type: "message", role: "user", content: "给代码" },
                { id: "a1", type: "message", role: "assistant", content: body },
                {
                  id: "t1",
                  type: "tool",
                  name: "run_command",
                  done: true,
                  error: false,
                  summary: "完成",
                  detail: Array.from({ length: 80 }, (_, i) => `stdout line ${i + 1}`).join("\n"),
                  expanded: true,
                },
              ],
            },
          },
        }),
      );
    }, md);
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const scrollStats = await page.evaluate(() => {
      const pre = document.querySelector(".md-content pre") as HTMLElement | null;
      const tool =
        (document.querySelector(".tool-shell-body") as HTMLElement | null) ||
        (document.querySelector(".tool-call-detail") as HTMLElement | null);
      if (!pre || !tool) return { ok: false, reason: "missing pre/tool" };
      const preCs = getComputedStyle(pre);
      const toolCs = getComputedStyle(tool);
      return {
        ok:
          pre.scrollHeight > pre.clientHeight + 8 &&
          tool.scrollHeight > tool.clientHeight + 8 &&
          (preCs.overflowY === "auto" || preCs.overflowY === "scroll") &&
          (toolCs.overflowY === "auto" || toolCs.overflowY === "scroll"),
        preScrollable: pre.scrollHeight > pre.clientHeight + 8,
        toolScrollable: tool.scrollHeight > tool.clientHeight + 8,
        preOverflow: preCs.overflowY,
        toolOverflow: toolCs.overflowY,
        preMaxH: preCs.maxHeight,
        toolMaxH: toolCs.maxHeight,
      };
    });
    expect(scrollStats.ok, JSON.stringify(scrollStats)).toBe(true);
    await assertUiHygiene(page);
  });

  test("confirm is inline in chat; session allow skips remount flash", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, confirmFlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("写文件并跑测试");
    await page.getByTestId("chat-send").click();

    const card = page.getByTestId("confirm-card");
    await expect(card).toBeVisible({ timeout: 5_000 });
    // Must live inside the chat log, not a fullscreen overlay.
    const inLog = await card.evaluate((el) => !!el.closest('[role="log"]'));
    expect(inLog).toBe(true);
    await expect(page.getByTestId("confirm-payload")).toBeVisible();
    await expect(card.locator(".confirm-shell, .confirm-path")).toHaveCount(1);
    await expect(page.getByRole("heading", { name: /写入文件|运行命令|编辑文件/ })).toBeVisible();
    await expect(page.locator(".confirm-overlay")).toHaveCount(0);

    // Remember-rule wiring: path/shell confirms offer「记住此规则」, and the
    // checked rule travels with the response to the backend.
    const remember = page.getByTestId("confirm-remember");
    await expect(remember).toBeVisible();
    await remember.check();
    await page.getByTestId("confirm-session-allow").check();
    await page.getByTestId("confirm-allow").click();

    // The write-confirm response carries the remembered rule; the later
    // run_command auto-approval (session allow) appends after it. Assert the
    // first recorded response — deterministic ordering.
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            (
              window as unknown as { __stitchConfirmHistory?: unknown[] }
            ).__stitchConfirmHistory?.[0] ?? null,
        ),
      )
      .toEqual({
        id: "confirm-write-1",
        approved: true,
        remember: { tool: "write_file", scope: "path", value: "util/math_ops.py" },
      });

    // Session allow: subsequent confirms must not remount the card.
    await expect(card).toBeHidden({ timeout: 8_000 });
    await expect(page.getByText("确认流完成")).toBeVisible({ timeout: 8_000 });
    await expect(page.getByTestId("tool-status")).toHaveCount(2);
    await assertUiHygiene(page);
  });

  test("outside-workspace read confirm renders read_file card with remember", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, readConfirmFlow: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("读取参考文件");
    await page.getByTestId("chat-send").click();

    const card = page.getByTestId("confirm-card");
    await expect(card).toBeVisible({ timeout: 5_000 });
    await expect(page.getByRole("heading", { name: "读取文件" })).toBeVisible();
    await expect(page.locator(".confirm-path-value")).toHaveText("C:/outside/ref.md");
    await expect(page.getByTestId("confirm-remember")).toBeVisible();

    // 记住此规则: read_file path scope travels with the response.
    await page.getByTestId("confirm-remember").check();
    await page.getByTestId("confirm-allow").click();
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            (
              window as unknown as { __stitchConfirmHistory?: unknown[] }
            ).__stitchConfirmHistory?.[0] ?? null,
        ),
      )
      .toEqual({
        id: "confirm-read-1",
        approved: true,
        remember: { tool: "read_file", scope: "path", value: "C:/outside/ref.md" },
      });
    await expect(page.getByText("已读取参考文件")).toBeVisible({ timeout: 8_000 });
    await assertUiHygiene(page);
  });

  test("list_directory renders listing rows; run_command uses shell style", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.evaluate(() => {
      const id = "tool-style-session";
      const now = Date.now();
      localStorage.setItem(
        "stitch-sessions",
        JSON.stringify({
          current: id,
          sessions: {
            [id]: {
              id,
              title: "工具样式",
              createdAt: now,
              updatedAt: now,
              messages: [
                { id: "u1", type: "message", role: "user", content: "列目录" },
                {
                  id: "t1",
                  type: "tool",
                  name: "list_directory",
                  done: true,
                  error: false,
                  summary: "src/",
                  detail: "src/\n[dir] lib \n[file] main.rs (1.2 KB)\n[file] app.css (4.0 KB)",
                  expanded: true,
                },
                {
                  id: "t2",
                  type: "tool",
                  name: "run_command",
                  done: true,
                  error: false,
                  summary: "python -m pytest",
                  detail: "===== 2 passed in 0.12s =====\nok",
                  expanded: true,
                },
              ],
            },
          },
        }),
      );
    });
    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    // Consecutive process tools fold into an overview; expand to inspect styles.
    await expect(page.getByTestId("tool-group")).toBeVisible();
    await expect(page.getByTestId("tool-group")).toHaveAttribute("data-count", "2");
    const groupOpen = page.getByTestId("tool-group-body");
    if ((await groupOpen.count()) === 0) {
      await page.getByTestId("tool-group-toggle").click();
    }
    await expect(page.getByTestId("tool-group-body")).toBeVisible();
    await expect(page.locator('[data-tool="list_directory"] .tool-call-name')).toHaveText("列出目录");
    await expect(page.getByTestId("tool-listing")).toBeVisible();
    await expect(page.getByTestId("tool-listing").locator(".tool-listing-row")).toHaveCount(3);
    await expect(page.getByTestId("tool-format").first()).toHaveText("目录");
    await expect(page.locator('[data-tool="run_command"] .tool-call-name')).toHaveText("运行命令");
    await expect(page.getByTestId("tool-shell")).toBeVisible();
    await expect(page.locator('[data-tool="run_command"] [data-testid="tool-format"]')).toHaveText(
      "终端",
    );
    await assertUiHygiene(page);
  });

  test("welcome scene uses short session title not prompt dump", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, workDir: "C:\\tmp\\stitch-mock" });
    await page.goto("/");
    await expect(page.getByTestId("welcome-scenes")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("load-prev-checkpoint")).toBeVisible();
    await page.getByTestId("load-prev-checkpoint").click();
    await expect(page.getByRole("log")).toContainText("已载入上一检查点");
    await expect(page.getByTestId("welcome-scenes")).toHaveCount(0);
    await page.getByTestId("session-new").first().click();
    await expect(page.getByTestId("welcome-scenes")).toBeVisible({ timeout: 10_000 });
    await page.getByTestId("scene-structure").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.locator(".session-row-active [data-testid='session-title']")).toHaveText(
      "了解项目结构",
    );
    await assertUiHygiene(page);
  });

  test("welcome scenes and sediment card after chat done", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("welcome-scenes")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("scene-structure")).toBeVisible();
    await expect(page.getByTestId("library-mature-scenes")).toHaveCount(0);
    await page.getByTestId("library-tab").click();
    await expect(page.getByTestId("library-scenes")).toBeVisible();
    await expect(page.getByTestId("library-mature-scenes")).toBeVisible();
    await expect(page.getByTestId("library-mature-debug-recover-auto")).toBeVisible();
    await page.getByTestId("library-mature-debug-recover-auto").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/改崩后停线复原/);
    await expect(page.getByTestId("chat-input")).toHaveValue(/stitch-debug-recover-report\.md/);
    await page.getByTestId("chat-input").fill("帮我总结这次任务");
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.getByTestId("sediment-card")).toHaveCount(0);
    await expect(page.getByTestId("msg-sediment")).toHaveAttribute("data-sediment-ready", "true");
    await page.getByTestId("msg-sediment").click();
    await expect(page.getByTestId("sediment-card")).toBeVisible({ timeout: 5_000 });
    await page.getByTestId("sediment-save").click();
    await expect(page.getByTestId("sediment-saved")).toBeVisible({ timeout: 5_000 });
    await expect(page.getByTestId("sediment-saved")).toHaveText(/审核/);
    await expect(page.getByTestId("msg-sediment")).toHaveAttribute("data-sediment-ready", "false");
    await assertUiHygiene(page);
  });

  test("mature scene sediment is short playbook not full prompt dump", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await page.getByTestId("library-tab").click();
    await page.getByTestId("library-mature-merge-ready-auto").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/合并前审查自动/);
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    await page.getByTestId("msg-sediment").click();
    const card = page.getByTestId("sediment-card");
    await expect(card).toBeVisible({ timeout: 5_000 });
    await expect(card.locator(".sediment-card-title")).toHaveText("合并前审查自动");
    const preview = page.getByTestId("sediment-preview");
    await expect(preview).toContainText("下次怎么跑");
    await expect(preview).toContainText("精选");
    await expect(preview).toContainText("stitch-merge-ready-report.md");
    await expect(preview).not.toContainText("按「合并前审查自动」跑完");
    await page.getByTestId("sediment-rerun").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/合并前审查自动/);
    await expect(page.getByTestId("chat-input")).toHaveValue(/MERGE_OK|MERGE_BLOCK/);
    await assertUiHygiene(page);
  });

  test("sediment without account prioritizes copy", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: false, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("settings-account-dot")).toBeVisible();
    await page.getByTestId("library-tab").click();
    await page.getByTestId("library-mature-debug-recover-auto").click();
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    await page.getByTestId("msg-sediment").click();
    const card = page.getByTestId("sediment-card");
    await expect(card).toBeVisible({ timeout: 5_000 });
    await expect(page.getByTestId("sediment-copy")).toBeVisible();
    await expect(page.getByTestId("sediment-copy")).not.toHaveClass(/btn-primary/);
    await expect(page.getByTestId("sediment-connect")).toBeVisible();
    await expect(page.getByTestId("sediment-connect")).toHaveClass(/is-muted/);
    await expect(page.getByTestId("sediment-save")).toHaveCount(0);
    await expect(page.getByTestId("sediment-rerun")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("sediment personal visibility saves to library only", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await page.getByTestId("open-settings").click();
    await page.getByTestId("settings-tab-account").click();
    await page.getByTestId("sediment-vis-personal").click();
    // settings-save persists and navigates to chat (thenChat).
    await page.getByTestId("settings-save").click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("chat-input").fill("帮我总结这次任务");
    await page.getByTestId("chat-send").click();
    await expect(page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" })).toBeVisible({
      timeout: 10_000,
    });
    await page.getByTestId("msg-sediment").click();
    await expect(page.getByTestId("sediment-save")).toHaveText("保存到个人库");
    await page.getByTestId("sediment-save").click();
    await expect(page.getByTestId("sediment-saved")).toHaveText("已保存到个人库");
    await assertUiHygiene(page);
  });
});
