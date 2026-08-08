import type { Page } from "@playwright/test";

export type MockTauriOptions = {
  /** When true, bootstrap goes to chat; otherwise first-run settings. */
  apiKeySet?: boolean;
  apiTokenSet?: boolean;
  workDir?: string;
  /** Simulate token → done agent stream for send_message. */
  streamChat?: boolean;
  /** Done 回复内容定制（默认「流式回复完成…」短文——长文场景测试用）。 */
  streamDoneReply?: string;
  /** Slow tokens so stop can interrupt. */
  streamSlow?: boolean;
  /** Emit a running tool then hang until cancel (tests stop clears spinner). */
  streamRunningTool?: boolean;
  /** Emit tool_output lines between tool_start and tool_done (ADR-037 live output). */
  streamToolOutput?: boolean;
  /** Emit a desktop tool (screenshot) so compact mode auto-enters. */
  streamDesktopTool?: boolean;
  /** Done carries hit_iteration_cap (tests「继续执行」affordance). */
  streamCapDone?: boolean;
  /** Reply includes unsafe HTML to assert sanitization. */
  streamHtml?: boolean;
  /** When send_message has planMode, emit plan_proposed and wait for respond_plan. */
  planFlow?: boolean;
  /** When run_suite is invoked, emit step progress then a failure summary error. */
  suiteFail?: boolean;
  /** When run_suite is invoked, reject with HTTP 401 (account auth). */
  suiteAuthFail?: boolean;
  /** Emit consecutive confirm_request events (write then run) for confirm UX tests. */
  confirmFlow?: boolean;
  /** Emit one outside-workspace read_file confirm (工作区外按范围授权). */
  readConfirmFlow?: boolean;
  /** usage events carry a layered hot/warm/cold breakdown (分层指示器). */
  usageLayers?: boolean;
  /** Bootstrap with a vision-capable model (gpt-4o) so the paste gate opens. */
  visionModel?: boolean;
  /** Local vision describe layer enabled (opens the paste gate on deepseek). */
  localVision?: boolean;
  /** Seed allow rules for the settings 通用 tab management UI. */
  allowRules?: Array<{ tool: string; scope: string; value: string }>;
  /** When true, test_connection rejects with an auth-style error. */
  testConnectionFail?: boolean | string;
  /** When true, test_promptstdio rejects with an auth-style error. */
  testPromptstdioFail?: boolean | string;
  /** Membership for mature-scene G1 soft tip. */
  isMember?: boolean;
  /** When true, check_update reports a newer version (U1 UI path). */
  updateAvailable?: boolean;
  /** Emit a tool that fails (tool_done success:false) — 失败卡自动展开路径。 */
  streamFailTool?: boolean;
  /** When true, fetch_announce returns an announcement (公告横幅路径). */
  announceAvailable?: boolean;
};

/**
 * Stub Tauri v2 IPC before any page script runs.
 * Enough for Stitch bootstrap: get_config / get_work_dir / finish_startup / listen.
 */
export async function mockTauri(page: Page, opts: MockTauriOptions = {}) {
  const apiKeySet = opts.apiKeySet ?? false;
  const apiTokenSet = opts.apiTokenSet ?? false;
  const workDir = opts.workDir ?? "C:/tmp/stitch-e2e";
  const streamChat = opts.streamChat ?? false;
  const streamDoneReply = opts.streamDoneReply;
  const streamSlow = opts.streamSlow ?? false;
  const streamRunningTool = opts.streamRunningTool ?? false;
  const streamToolOutput = opts.streamToolOutput ?? false;
  const streamDesktopTool = opts.streamDesktopTool ?? false;
  const streamCapDone = opts.streamCapDone ?? false;
  const streamHtml = opts.streamHtml ?? false;
  const planFlow = opts.planFlow ?? false;
  const suiteFail = opts.suiteFail ?? false;
  const suiteAuthFail = opts.suiteAuthFail ?? false;
  const confirmFlow = opts.confirmFlow ?? false;
  const doReadConfirm = opts.readConfirmFlow ?? false;
  const layersOn = opts.usageLayers ?? false;
  const visionOn = opts.visionModel ?? false;
  const localVisionOn = opts.localVision ?? false;
  const seededRules = opts.allowRules ?? [];
  const testConnectionFail = opts.testConnectionFail ?? false;
  const testPromptstdioFail = opts.testPromptstdioFail ?? false;
  const isMember = opts.isMember ?? false;
  const updateAvailable = opts.updateAvailable ?? false;
  const failTool = opts.streamFailTool ?? false;
  const announceOn = opts.announceAvailable ?? false;

  await page.addInitScript(
    ({
      apiKeySet: keySet,
      apiTokenSet: tokenSet,
      workDir: dir,
      streamChat: doStream,
      streamDoneReply: doneReply,
      streamSlow: slow,
      streamRunningTool: hangTool,
      streamToolOutput: liveTool,
      streamDesktopTool: desktopTool,
      streamCapDone: capDone,
      streamHtml: htmlReply,
      planFlow: doPlan,
      suiteFail: doSuiteFail,
      suiteAuthFail: doSuiteAuthFail,
      confirmFlow: doConfirm,
      readConfirmFlow: doReadConfirm,
      usageLayers: layersOn,
      visionModel: visionOn,
      localVision: localVisionOn,
      allowRules: seededRules,
      testConnectionFail: connFail,
      testPromptstdioFail: promptFail,
      isMember: member,
      updateAvailable: haveUpdate,
      streamFailTool: failTool,
      announceAvailable: announceOn,
    }) => {
      let allowRules: Array<{ tool: string; scope: string; value: string }> =
        seededRules.map((r) => ({ ...r }));
      const bootModel = visionOn ? "gpt-4o" : "deepseek-v4-flash";
      const config: {
        api_base: string;
        api_token_set: boolean;
        api_token_masked: string;
        active_mcp_id: string;
        mcp_profiles: Array<{
          id: string;
          label: string;
          api_base: string;
          api_token_masked: string;
          api_token_set: boolean;
        }>;
        mcp_servers: Array<{
          id: string;
          label: string;
          transport: string;
          enabled: boolean;
          command: string | null;
          args: string[];
          env?: Record<string, string>;
          cwd?: string | null;
          url: string | null;
          auth_set: boolean;
          auth_masked: string;
        }>;
        llm_provider: string;
        llm_api_base: string;
        llm_api_key_masked: string;
        llm_api_key_set: boolean;
        llm_model: string;
        active_profile_id: string;
        llm_profiles: Array<{
          id: string;
          label: string;
          provider: string;
          api_base: string;
          api_key_masked: string;
          api_key_set: boolean;
          model: string;
          supports_images?: boolean;
        }>;
        max_iterations: number;
        sediment_visibility: string;
        local_vision?: { enabled: boolean; api_base: string; model: string; timeout_secs: number };
      } = {
        api_base: "http://127.0.0.1:8090",
        api_token_set: tokenSet,
        api_token_masked: tokenSet ? "pts-****abcd" : "",
        active_mcp_id: "default",
        mcp_profiles: [
          {
            id: "default",
            label: "PromptStdio",
            api_base: "http://127.0.0.1:8090",
            api_token_masked: tokenSet ? "pts-****abcd" : "",
            api_token_set: tokenSet,
          },
        ],
        mcp_servers: [],
        llm_provider: "deepseek",
        llm_api_base: "https://api.deepseek.com",
        llm_api_key_masked: keySet ? "sk-****" : "",
        llm_api_key_set: keySet,
        llm_model: bootModel,
        active_profile_id: "default",
        llm_profiles: [
          {
            id: "default",
            label: visionOn ? "GPT-4o" : "DeepSeek",
            provider: visionOn ? "openai" : "deepseek",
            api_base: visionOn ? "https://api.openai.com" : "https://api.deepseek.com",
            api_key_masked: keySet ? "sk-****" : "",
            api_key_set: keySet,
            model: bootModel,
          },
        ],
        max_iterations: 25,
        sediment_visibility: "explore",
        local_vision: {
          enabled: localVisionOn,
          api_base: "http://127.0.0.1:11434/v1",
          model: "qwen3-vl:8b",
          timeout_secs: 30,
        },
      };

      function syncActiveFromProfile() {
        const p =
          config.llm_profiles.find((x) => x.id === config.active_profile_id) ||
          config.llm_profiles[0];
        if (!p) return;
        config.llm_provider = p.provider;
        config.llm_api_base = p.api_base;
        config.llm_model = p.model;
        config.llm_api_key_set = p.api_key_set;
        config.llm_api_key_masked = p.api_key_masked;
      }

      function syncActiveFromMcp() {
        const p =
          config.mcp_profiles.find((x) => x.id === config.active_mcp_id) ||
          config.mcp_profiles[0];
        if (!p) return;
        config.api_base = p.api_base;
        config.api_token_set = p.api_token_set;
        config.api_token_masked = p.api_token_masked;
      }

      function snapshot() {
        return {
          ...config,
          llm_profiles: config.llm_profiles.map((p) => ({
            ...p,
            // Mirror the Rust model_supports_vision heuristic so the paste
            // gate follows whatever model the test switched to.
            supports_images: /gpt-4o|gpt-4|claude|kimi|moonshot|qwen|glm-4v|gemini|vision/i.test(
              p.model,
            ),
          })),
          mcp_profiles: config.mcp_profiles.map((p) => ({ ...p })),
          mcp_servers: config.mcp_servers.map((p) => ({
            ...p,
            args: [...p.args],
            env: { ...(p.env || {}) },
          })),
        };
      }

      const callbacks = new Map<number, (...args: unknown[]) => void>();
      let nextCb = 1;
      const agentListeners = new Set<number>();
      let cancelRequested = false;
      let planWaiter: ((approved: boolean) => void) | null = null;
      const confirmWaiters = new Map<string, (approved: boolean) => void>();

      function emitAgent(payload: Record<string, unknown>) {
        for (const id of agentListeners) {
          callbacks.get(id)?.({ event: "agent-event", payload });
        }
      }

      function sleep(ms: number) {
        return new Promise((r) => setTimeout(r, ms));
      }

      function emitUsage(partial: {
        iteration: number;
        input_tokens: number;
        output_tokens: number;
        context_tokens: number;
        compacted?: boolean;
        layers?: {
          hot_msgs: number;
          warm_entries: number;
          cold_entries: number;
          hot_tokens: number;
          warm_tokens: number;
          cold_tokens: number;
          total_tokens: number;
          limit: number;
        } | null;
      }) {
        emitAgent({
          type: "usage",
          iteration: partial.iteration,
          input_tokens: partial.input_tokens,
          output_tokens: partial.output_tokens,
          context_tokens: partial.context_tokens,
          context_limit: 64_000,
          compacted: !!partial.compacted,
          ...(partial.layers !== undefined ? { layers: partial.layers } : {}),
        });
      }

      const layeredStats = {
        hot_msgs: 40,
        warm_entries: 3,
        cold_entries: 2,
        hot_tokens: 32_000,
        warm_tokens: 4_000,
        cold_tokens: 1_000,
        total_tokens: 37_000,
        limit: 64_000,
      };

      async function streamReply() {
        cancelRequested = false;
        emitUsage({
          iteration: 1,
          input_tokens: 1200,
          output_tokens: 0,
          context_tokens: 1800,
          layers: layersOn ? layeredStats : undefined,
        });
        if (hangTool) {
          emitAgent({ type: "tool_start", name: "git_diff", call_id: "call_hang_001" });
          while (!cancelRequested) {
            await sleep(120);
          }
          emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
          return;
        }
        if (desktopTool) {
          // Desktop tool → compact overlay auto-enter + live elapsed.
          const desktopCallId = "call_desktop_001";
          emitAgent({
            type: "tool_start",
            name: "desktop_screenshot",
            call_id: desktopCallId,
          });
          // Stay alive ~7s so the compact stopwatch has an assertion window;
          // cancellable at any tick (stop button test).
          for (let i = 0; i < 14; i++) {
            if (cancelRequested) {
              emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
              return;
            }
            await sleep(500);
          }
          if (cancelRequested) {
            emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
            return;
          }
          emitAgent({
            type: "tool_done",
            name: "desktop_screenshot",
            call_id: desktopCallId,
            success: true,
            summary: "已截图并识别到 2 个窗口",
            metrics: { duration_ms: 830.2 },
          });
          emitAgent({ type: "token", text: "截图完成" });
          emitAgent({
            type: "done",
            response: "已截图并识别窗口。",
            iterations: 1,
            input_tokens: 1400,
            output_tokens: 30,
            context_tokens: 2000,
            context_limit: 64_000,
          });
          return;
        }
        if (failTool) {
          // 失败工具：tool_done success:false → 卡片自动展开（失败观察路径）。
          const failCallId = "call_fail_001";
          emitAgent({ type: "tool_start", name: "run_command", call_id: failCallId });
          await sleep(200);
          emitAgent({
            type: "tool_done",
            name: "run_command",
            call_id: failCallId,
            success: false,
            summary: "安装失败：网络连接被拒绝（ECONNREFUSED）",
          });
          emitAgent({ type: "token", text: "安装失败" });
          emitAgent({
            type: "done",
            response: "安装失败，请检查网络后重试。",
            iterations: 1,
            input_tokens: 900,
            output_tokens: 20,
            context_tokens: 1200,
            context_limit: 64_000,
          });
          return;
        }
        if (liveTool) {
          // ADR-037: run_command with live stdout lines between start/done.
          const liveCallId = "call_live_001";
          emitAgent({ type: "tool_start", name: "run_command", call_id: liveCallId });
          const lines = [
            "Downloading packages…",
            "resolve dep 1/40",
            "resolve dep 12/40",
            "resolve dep 28/40",
            "build native module",
            "linking done in 3.2s",
          ];
          for (const line of lines) {
            if (cancelRequested) {
              emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
              return;
            }
            emitAgent({
              type: "tool_output",
              name: "run_command",
              call_id: liveCallId,
              text: `${line}\n`,
            });
            await sleep(150);
          }
          // Leave a wide assertion window: the tail line must stay visible
          // (and pinned) while the tool is still running.
          await sleep(900);
          emitAgent({
            type: "tool_done",
            name: "run_command",
            call_id: liveCallId,
            success: true,
            summary: `${lines.join("\n")}\nDone in 4.1s`,
            // Benchmark metrics ride along structured (mirrors real ToolResult).
            metrics: { duration_ms: 4123.5 },
          });
          emitAgent({ type: "token", text: "安装完成" });
          emitAgent({
            type: "done",
            response: "安装完成。依赖已就绪。",
            iterations: 1,
            input_tokens: 1400,
            output_tokens: 30,
            context_tokens: 2000,
            context_limit: 64_000,
          });
          return;
        }
        // Slow mode: pause before first token so stop-before-token is testable.
        if (slow) {
          await sleep(700);
          if (cancelRequested) {
            emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
            return;
          }
        }
        const parts = htmlReply
          ? ["<p>安全内容</p>", '<script>alert(1)</script><p>尾部</p>']
          : ["流式", "回复", "完成"];
        let outTok = 0;
        for (const text of parts) {
          if (cancelRequested) {
            emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
            return;
          }
          emitAgent({ type: "token", text });
          outTok += Math.max(1, Math.ceil(text.length / 2));
          emitUsage({
            iteration: 1,
            input_tokens: 1200,
            output_tokens: outTok,
            context_tokens: 1800 + outTok,
          });
          await sleep(slow ? 400 : 40);
        }
        if (cancelRequested) {
          emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
          return;
        }
        const response = htmlReply
          ? "<p>安全内容</p><script>alert(1)</script><p>尾部</p>"
          : (doneReply ?? "流式回复完成。已整理本次任务要点，便于你复用到个人库或下次会话。");
        emitAgent({
          type: "done",
          response,
          iterations: 1,
          input_tokens: 1200,
          output_tokens: outTok || 24,
          context_tokens: 1800 + (outTok || 24),
          context_limit: 64_000,
          hit_iteration_cap: !!capDone,
        });
      }

      async function runSuiteFail() {
        cancelRequested = false;
        emitAgent({ type: "plan_step_start", index: 0, description: "收集素材" });
        await sleep(40);
        emitAgent({ type: "plan_step_done", index: 0, description: "收集素材" });
        emitAgent({ type: "token", text: "已收集 2 条" });
        await sleep(30);
        emitAgent({ type: "plan_step_start", index: 1, description: "改写正文" });
        await sleep(40);
        emitAgent({
          type: "error",
          message:
            "套件「演示套件」未全部完成：第 2/2 步失败（改写正文）。\n\n" +
            "## 已完成步骤\n\n## 步骤 1/2\n已收集 2 条\n\n\n" +
            "## 失败步骤\n\n### 步骤 2/2 · 改写正文\n\n原因：模型超时\n",
        });
      }

      async function waitConfirm(id: string, tool: string, message: string): Promise<boolean> {
        // Register before emit: session-allow may respond synchronously in openConfirm.
        const p = new Promise<boolean>((resolve) => {
          confirmWaiters.set(id, resolve);
        });
        emitAgent({ type: "confirm_request", id, tool, message });
        return await p;
      }

      async function confirmThenRun() {
        cancelRequested = false;
        emitAgent({ type: "tool_start", name: "write_file", call_id: "call_write_001" });
        const okWrite = await waitConfirm(
          "confirm-write-1",
          "write_file",
          "Write to file: util/math_ops.py\nAllow?",
        );
        if (cancelRequested || !okWrite) {
          emitAgent({
            type: "tool_done",
            name: "write_file",
            call_id: "call_write_001",
            success: false,
            summary: "已拒绝",
          });
          emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
          return;
        }
        emitAgent({
          type: "tool_done",
          name: "write_file",
          call_id: "call_write_001",
          success: true,
          summary: "Wrote 12 lines to util/math_ops.py",
        });
        emitAgent({ type: "tool_start", name: "run_command", call_id: "call_run_001" });
        const okRun = await waitConfirm(
          "confirm-run-1",
          "run_command",
          "Run command: python tests/test_math_ops.py\nAllow?",
        );
        if (cancelRequested || !okRun) {
          emitAgent({
            type: "tool_done",
            name: "run_command",
            call_id: "call_run_001",
            success: false,
            summary: "已拒绝",
          });
          emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
          return;
        }
        emitAgent({
          type: "tool_done",
          name: "run_command",
          call_id: "call_run_001",
          success: true,
          summary: "===== 2 passed =====",
        });
        emitAgent({ type: "token", text: "确认流完成" });
        emitAgent({
          type: "done",
          response: "确认流完成",
          iterations: 1,
          input_tokens: 900,
          output_tokens: 28,
          context_tokens: 1200,
          context_limit: 64_000,
        });
      }

      // Outside-workspace read confirm（工作区外按范围授权）.
      async function confirmReadThenRun() {
        cancelRequested = false;
        emitAgent({ type: "tool_start", name: "read_file", call_id: "call_read_001" });
        const okRead = await waitConfirm(
          "confirm-read-1",
          "read_file",
          "Read outside workspace: C:/outside/ref.md\nAllow?",
        );
        if (cancelRequested || !okRead) {
          emitAgent({
            type: "tool_done",
            name: "read_file",
            call_id: "call_read_001",
            success: false,
            summary: "已拒绝",
          });
          emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
          return;
        }
        emitAgent({
          type: "tool_done",
          name: "read_file",
          call_id: "call_read_001",
          success: true,
          summary: "3 lines",
        });
        emitAgent({ type: "token", text: "已读取参考文件" });
        emitAgent({
          type: "done",
          response: "已读取参考文件",
          iterations: 1,
          input_tokens: 900,
          output_tokens: 22,
          context_tokens: 1200,
          context_limit: 64_000,
        });
      }

      async function planThenRun() {
        cancelRequested = false;
        const planId = "mock-plan-1";
        emitAgent({
          type: "plan_proposed",
          id: planId,
          plan: {
            title: "模拟计划",
            steps: [
              { description: "列出工作目录", status: "pending" },
              { description: "一句话总结", status: "pending" },
            ],
          },
        });
        const approved = await new Promise<boolean>((resolve) => {
          planWaiter = resolve;
        });
        planWaiter = null;
        if (cancelRequested) {
          emitAgent({ type: "cancelled", message: "Generation cancelled by user." });
          return;
        }
        if (!approved) {
          emitAgent({ type: "plan_rejected" });
          emitAgent({
            type: "done",
            response: "计划已拒绝，未执行。",
            iterations: 0,
            input_tokens: 800,
            output_tokens: 40,
            context_tokens: 900,
            context_limit: 64_000,
          });
          return;
        }
        emitUsage({
          iteration: 1,
          input_tokens: 1600,
          output_tokens: 0,
          context_tokens: 2200,
        });
        emitAgent({ type: "plan_approved" });
        emitAgent({ type: "plan_step_start", index: 0, description: "列出工作目录" });
        await sleep(40);
        emitAgent({ type: "plan_step_done", index: 0, description: "列出工作目录" });
        emitAgent({ type: "plan_step_start", index: 1, description: "一句话总结" });
        await sleep(40);
        emitAgent({ type: "plan_step_done", index: 1, description: "一句话总结" });
        emitAgent({ type: "token", text: "计划执行完成" });
        emitAgent({
          type: "done",
          response: "计划执行完成",
          iterations: 1,
          input_tokens: 1600,
          output_tokens: 36,
          context_tokens: 2300,
          context_limit: 64_000,
        });
      }

      (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        callbacks,
        /** E2E: last send_message history (for stop→new-turn assertions). */
        lastSendHistory: [] as Array<{ role: string; content: string }>,
        lastSendMessage: "" as string,
        /** E2E: cancel_generation invoke count (Esc must not over-cancel). */
        cancelGenerationCount: 0,
        transformCallback(cb: (...args: unknown[]) => void, once = false) {
          const id = nextCb++;
          callbacks.set(
            id,
            once
              ? (...args: unknown[]) => {
                  callbacks.delete(id);
                  cb(...args);
                }
              : cb,
          );
          return id;
        },
        unregisterCallback(id: number) {
          callbacks.delete(id);
          agentListeners.delete(id);
        },
        runCallback(id: number, data: unknown) {
          callbacks.get(id)?.(data);
        },
        invoke: async (cmd: string, args?: Record<string, unknown>) => {
          switch (cmd) {
            case "get_config":
              return snapshot();
            case "get_allow_rules":
              return allowRules.map((r) => ({ ...r }));
            case "remove_allow_rule": {
              const a = args ?? {};
              allowRules = allowRules.filter(
                (r) =>
                  !(
                    r.tool === String(a.tool ?? "") &&
                    r.scope === String(a.scope ?? "") &&
                    r.value === String(a.value ?? "")
                  ),
              );
              return allowRules.map((r) => ({ ...r }));
            }
            case "clear_allow_rules":
              allowRules = [];
              return [];
            case "save_config":
              if (args && typeof args.updates === "object" && args.updates) {
                const u = args.updates as Record<string, string>;
                if (u.llm_api_key) {
                  config.llm_api_key_set = true;
                  config.llm_api_key_masked = "sk-****";
                  const active = config.llm_profiles.find(
                    (p) => p.id === config.active_profile_id,
                  );
                  if (active) {
                    active.api_key_set = true;
                    active.api_key_masked = "sk-****";
                  }
                }
                if ("api_token" in u) {
                  if (u.api_token) {
                    config.api_token_set = true;
                    config.api_token_masked = "pts-****abcd";
                  } else {
                    config.api_token_set = false;
                    config.api_token_masked = "";
                  }
                  const activeMcp = config.mcp_profiles.find(
                    (p) => p.id === config.active_mcp_id,
                  );
                  if (activeMcp) {
                    activeMcp.api_token_set = config.api_token_set;
                    activeMcp.api_token_masked = config.api_token_masked;
                  }
                }
                if (u.api_base) {
                  config.api_base = u.api_base;
                  const activeMcp = config.mcp_profiles.find(
                    (p) => p.id === config.active_mcp_id,
                  );
                  if (activeMcp) activeMcp.api_base = u.api_base;
                }
                if (u.llm_provider) config.llm_provider = u.llm_provider;
                if (u.llm_api_base) config.llm_api_base = u.llm_api_base;
                if (u.llm_model) config.llm_model = u.llm_model;
                if (u.max_iterations) config.max_iterations = Number(u.max_iterations) || 25;
                if (u.sediment_visibility === "personal" || u.sediment_visibility === "explore") {
                  config.sediment_visibility = u.sediment_visibility;
                }
                const active = config.llm_profiles.find(
                  (p) => p.id === config.active_profile_id,
                );
                if (active) {
                  if (u.llm_provider) active.provider = u.llm_provider;
                  if (u.llm_api_base) active.api_base = u.llm_api_base;
                  if (u.llm_model) active.model = u.llm_model;
                }
              }
              return snapshot();
            case "upsert_llm_profile": {
              const a = (args?.args ?? args) as {
                id?: string;
                label?: string;
                provider?: string;
                api_base?: string;
                api_key?: string;
                model?: string;
              };
              const id = (a.id || "").trim() || "default";
              let p = config.llm_profiles.find((x) => x.id === id);
              if (!p) {
                p = {
                  id,
                  label: a.label || id,
                  provider: a.provider || "custom",
                  api_base: a.api_base || "",
                  api_key_masked: "",
                  api_key_set: false,
                  model: a.model || "",
                };
                config.llm_profiles.push(p);
              }
              if (a.label) p.label = a.label;
              if (a.provider) p.provider = a.provider;
              if (a.api_base != null) p.api_base = a.api_base;
              if (a.model) p.model = a.model;
              if (a.api_key) {
                p.api_key_set = true;
                p.api_key_masked = "sk-****";
              }
              if (!config.active_profile_id || config.active_profile_id === id) {
                config.active_profile_id = id;
                syncActiveFromProfile();
              }
              return snapshot();
            }
            case "delete_llm_profile": {
              const id = String(args?.id ?? "").trim();
              config.llm_profiles = config.llm_profiles.filter((p) => p.id !== id);
              if (config.active_profile_id === id) {
                config.active_profile_id = config.llm_profiles[0]?.id || "default";
                syncActiveFromProfile();
              }
              return snapshot();
            }
            case "set_active_llm_profile": {
              const id = String(args?.id ?? "").trim();
              if (config.llm_profiles.some((p) => p.id === id)) {
                config.active_profile_id = id;
                syncActiveFromProfile();
              }
              return snapshot();
            }
            case "upsert_mcp_profile": {
              const a = (args?.args ?? args) as {
                id?: string;
                label?: string;
                api_base?: string;
                api_token?: string;
              };
              const id = (a.id || "").trim() || "default";
              let p = config.mcp_profiles.find((x) => x.id === id);
              if (!p) {
                p = {
                  id,
                  label: a.label || "PromptStdio",
                  api_base: a.api_base || "https://www.promptstdio.com",
                  api_token_masked: "",
                  api_token_set: false,
                };
                config.mcp_profiles.push(p);
              }
              if (a.label) p.label = a.label;
              if (a.api_base != null) p.api_base = a.api_base;
              if (a.api_token) {
                p.api_token_set = true;
                p.api_token_masked = "pts-****abcd";
              }
              if (!config.active_mcp_id || config.active_mcp_id === id) {
                config.active_mcp_id = id;
                syncActiveFromMcp();
              }
              return snapshot();
            }
            case "delete_mcp_profile": {
              const id = String(args?.id ?? "").trim();
              config.mcp_profiles = config.mcp_profiles.filter((p) => p.id !== id);
              if (config.active_mcp_id === id) {
                config.active_mcp_id = config.mcp_profiles[0]?.id || "default";
                syncActiveFromMcp();
              }
              return snapshot();
            }
            case "set_active_mcp_profile": {
              const id = String(args?.id ?? "").trim();
              if (config.mcp_profiles.some((p) => p.id === id)) {
                config.active_mcp_id = id;
                syncActiveFromMcp();
              }
              return snapshot();
            }
            case "clear_mcp_profile_token": {
              const id = String(args?.id ?? "").trim();
              const p = config.mcp_profiles.find((x) => x.id === id);
              if (p) {
                p.api_token_set = false;
                p.api_token_masked = "";
              }
              if (config.active_mcp_id === id) {
                config.api_token_set = false;
                config.api_token_masked = "";
              }
              return snapshot();
            }
            case "upsert_mcp_server": {
              const a = (args?.args ?? args) as {
                id?: string;
                label?: string;
                transport?: string;
                enabled?: boolean;
                command?: string;
                args?: string[];
                env?: Record<string, string>;
                cwd?: string;
                url?: string;
                auth_token?: string;
              };
              const id = (a.id || "").trim() || "srv";
              let p = config.mcp_servers.find((x) => x.id === id);
              if (!p) {
                p = {
                  id,
                  label: a.label || id,
                  transport: a.transport || "stdio",
                  enabled: a.enabled !== false,
                  command: a.command || null,
                  args: a.args || [],
                  env: {},
                  cwd: null,
                  url: a.url || null,
                  auth_set: false,
                  auth_masked: "",
                };
                config.mcp_servers.push(p);
              }
              if (a.label) p.label = a.label;
              if (a.transport) p.transport = a.transport;
              if (a.enabled != null) p.enabled = !!a.enabled;
              if (a.command != null) p.command = a.command;
              if (a.args) p.args = a.args;
              if (a.env) p.env = { ...a.env };
              if (a.cwd != null) p.cwd = a.cwd.trim() || null;
              if (a.url != null) p.url = a.url;
              if (a.auth_token) {
                p.auth_set = true;
                p.auth_masked = "Bear****oken";
              }
              return snapshot();
            }
            case "import_mcp_servers": {
              const a = (args?.args ?? args) as {
                json?: string;
                replace?: boolean;
              };
              const raw = String(a.json || "").trim();
              if (!raw) throw new Error("JSON 无效");
              let parsed: {
                mcpServers?: Record<
                  string,
                  {
                    command?: string;
                    args?: string[];
                    env?: Record<string, string>;
                    cwd?: string;
                    url?: string;
                    type?: string;
                    headers?: Record<string, string>;
                  }
                >;
              };
              try {
                parsed = JSON.parse(raw);
              } catch {
                throw new Error("JSON 无效");
              }
              const map = parsed.mcpServers || {};
              if (a.replace) config.mcp_servers = [];
              for (const [id, entry] of Object.entries(map)) {
                const transport =
                  entry.type === "sse"
                    ? "sse"
                    : entry.url
                      ? "http"
                      : "stdio";
                let p = config.mcp_servers.find((x) => x.id === id);
                if (!p) {
                  p = {
                    id,
                    label: id,
                    transport,
                    enabled: true,
                    command: entry.command || null,
                    args: entry.args || [],
                    env: entry.env || {},
                    cwd: entry.cwd || null,
                    url: entry.url || null,
                    auth_set: !!(entry.headers && entry.headers.Authorization),
                    auth_masked: entry.headers?.Authorization
                      ? "Bear****oken"
                      : "",
                  };
                  config.mcp_servers.push(p);
                } else {
                  p.transport = transport;
                  p.command = entry.command || null;
                  p.args = entry.args || [];
                  p.env = entry.env || {};
                  p.cwd = entry.cwd || null;
                  p.url = entry.url || null;
                }
              }
              return snapshot();
            }
            case "add_promptstdio_mcp_preset": {
              let p = config.mcp_servers.find((x) => x.id === "promptstdio");
              if (!p) {
                p = {
                  id: "promptstdio",
                  label: "PromptStdio",
                  transport: "http",
                  enabled: false,
                  command: null,
                  args: [],
                  url: `${config.api_base.replace(/\/$/, "")}/mcp`,
                  env: {},
                  cwd: null,
                  auth_set: !!config.api_token_set,
                  auth_masked: config.api_token_set ? "Bear****oken" : "",
                };
                config.mcp_servers.push(p);
              }
              return snapshot();
            }
            case "delete_mcp_server": {
              const id = String(args?.id ?? "").trim();
              config.mcp_servers = config.mcp_servers.filter((p) => p.id !== id);
              return snapshot();
            }
            case "set_mcp_server_enabled": {
              const id = String(args?.id ?? "").trim();
              const p = config.mcp_servers.find((x) => x.id === id);
              if (p) p.enabled = !!args?.enabled;
              return snapshot();
            }
            case "test_mcp_server": {
              const id = String(args?.id ?? "").trim();
              const p = config.mcp_servers.find((x) => x.id === id);
              if (!p) throw new Error("找不到 MCP 服务");
              if (p.transport === "stdio" && !p.command) {
                throw new Error("stdio 服务须填写命令");
              }
              if (p.transport === "http" && !p.url) {
                throw new Error("HTTP 服务须填写地址");
              }
              return `MCP 连接成功 · ${p.label} · 可用工具 2 个`;
            }
            case "get_work_dir":
              return dir;
            case "set_work_dir":
              return typeof args?.path === "string" ? args.path : dir;
            case "browse_work_dir":
              return dir;
            case "open_folder_path":
              return null;
            case "finish_startup":
            case "clear_taskbar_progress":
            case "set_titlebar_theme":
              return null;
            case "respond_confirmation": {
              const id = typeof args?.id === "string" ? args.id : "";
              const approved = !!args?.approved;
              const waiter = confirmWaiters.get(id);
              if (waiter) {
                confirmWaiters.delete(id);
                waiter(approved);
              }
              // Test hook: confirm responses incl. remembered rules, in order.
              const history = (
                window as unknown as { __stitchConfirmHistory?: unknown[] }
              ).__stitchConfirmHistory ??= [];
              history.push({ id, approved, remember: args?.remember ?? null });
              return null;
            }
            case "frontend_log":
              return null;
            case "respond_plan":
              planWaiter?.(!!args?.approved);
              return null;
            case "cancel_generation": {
              cancelRequested = true;
              const internals = (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> })
                .__TAURI_INTERNALS__;
              internals.cancelGenerationCount =
                ((internals.cancelGenerationCount as number | undefined) ?? 0) + 1;
              planWaiter?.(false);
              for (const [id, waiter] of confirmWaiters) {
                confirmWaiters.delete(id);
                waiter(false);
              }
              return null;
            }
            case "list_suites":
              return [
                {
                  id: "task_suite:demo",
                  title: "演示套件",
                  description: "e2e",
                  tags: ["demo"],
                  step_count: 2,
                },
              ];
            case "list_agents":
              return [
                {
                  id: "task_agent:demo",
                  name: "演示智能体",
                  task_suite_id: "task_suite:demo",
                  task_suite_title: "演示套件",
                  trigger_mode: "manual",
                  file_write_permission: "ask",
                  step_strategy: "sequential",
                  failure_policy: "stop",
                },
              ];
            case "list_skills":
              return [
                {
                  slug: "pm-prd-demo",
                  title: "PM PRD 转 Demo",
                  description: "粘贴 PRD，拆架构并产出可演示前端。",
                  version: "2.0.1",
                  install_phrase:
                    "帮我安装 PromptStdio 的「PM PRD 转 Demo」Skill，装到你认为合适的 Skill 目录",
                  sync_phrase:
                    "帮我更新 PromptStdio 的「PM PRD 转 Demo」Skill，写到你认为合适的 Skill 目录",
                  price_display: null,
                },
              ];
            case "list_local_skills":
              return [
                {
                  slug: "demo-local",
                  title: "本机 Demo Skill",
                  description: "用于能力条冒烟",
                  rel_path: ".agents/skills/demo-local",
                  scope: "workspace",
                },
                {
                  slug: "demo-user",
                  title: "用户全局 Skill",
                  description: "来自 ~/.agents/skills",
                  rel_path: "~/.agents/skills/demo-user",
                  scope: "user",
                },
              ];
            case "export_skill":
              return {
                path: "D:\\backup\\demo-local",
                files: 3,
              };
            case "create_prompt": {
              if (!config.api_token_set) {
                throw new Error("请先在设置中填写 PromptStdio API Token");
              }
              const body =
                args && typeof args.args === "object" && args.args
                  ? (args.args as Record<string, unknown>)
                  : {};
              const title =
                typeof body.title === "string" && body.title.trim()
                  ? body.title.trim()
                  : "未命名";
              return { id: "prompt:e2e-sediment", title };
            }
            case "submit_explore": {
              if (!config.api_token_set) {
                throw new Error("请先在设置中填写 PromptStdio API Token");
              }
              return {
                system_prompt_id: "sys-e2e-sediment",
                slug: "ugc-e2e-sediment",
                status: "draft",
                already_submitted: false,
              };
            }
            case "track_usage":
              return null;
            case "run_suite":
              if (doSuiteAuthFail) {
                throw new Error('API error 401: {"error":{"code":401,"message":"Unauthorized"}}');
              }
              if (doSuiteFail) {
                await runSuiteFail();
              }
              return null;
            case "run_agent":
              return null;
            case "test_connection":
              if (connFail) {
                const msg =
                  typeof connFail === "string"
                    ? connFail
                    : '模型 API 错误: {"error":{"message":"Authentication Fails, Your api key: aaaa is invalid","type":"authentication_error"}}';
                throw new Error(msg);
              }
              return true;
            case "test_promptstdio": {
              if (promptFail) {
                const msg =
                  typeof promptFail === "string"
                    ? promptFail
                    : "PromptStdio 连接失败: unauthorized 401 invalid token";
                throw new Error(msg);
              }
              const a = (args?.args ?? args ?? {}) as {
                profile_id?: string;
                api_token?: string;
                api_base?: string;
              };
              const overrideTok = (a.api_token || "").trim();
              if (overrideTok) {
                const base = (a.api_base || config.api_base || "").trim();
                return `PromptStdio 连接成功 · ${base} · 套件列表可读（本页 1 条）`;
              }
              const pid = (a.profile_id || config.active_mcp_id || "").trim();
              const p =
                config.mcp_profiles.find((x) => x.id === pid) ||
                config.mcp_profiles.find((x) => x.id === config.active_mcp_id);
              if (!p?.api_token_set && !config.api_token_set) {
                throw new Error("请先在设置中填写 PromptStdio API Token");
              }
              return `PromptStdio 连接成功 · ${p?.api_base || config.api_base} · 套件列表可读（本页 1 条）`;
            }
            case "set_window_title": {
              const internals = (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> })
                .__TAURI_INTERNALS__;
              internals.lastWindowTitle = typeof args?.title === "string" ? args.title : "";
              return null;
            }
            case "fetch_announce":
              if (announceOn) {
                return {
                  id: "mock-announce-1",
                  title: "Stitch 0.2.3 已发布",
                  body: "修复滚动控制与失败卡折叠；新增侧栏分割线拖动。",
                  url: "https://www.promptstdio.com/stitch",
                };
              }
              return null;
            case "check_update":
              if (haveUpdate) {
                return {
                  available: true,
                  current_version: "0.1.2",
                  latest_version: "0.1.3",
                  release_notes: "修复若干问题；新增启动更新提醒。",
                };
              }
              return {
                available: false,
                current_version: "0.1.2",
              };
            case "install_update":
              if (!haveUpdate) {
                throw new Error("没有可用的更新");
              }
              return null;
            case "get_membership":
              return {
                token_set: tokenSet,
                is_member: !!member,
                status: member ? "active" : tokenSet ? "none" : "none",
                plan: member ? "yearly" : null,
                pricing_url: "https://www.promptstdio.com/pricing",
              };
            case "open_external_url":
              return null;
            case "start_account_connect": {
              // Layer A: simulate successful website connect → token written.
              const activeId = config.active_mcp_id || "default";
              let profile = config.mcp_profiles.find((p) => p.id === activeId);
              if (!profile) {
                profile = {
                  id: activeId,
                  label: "PromptStdio",
                  api_base: config.api_base || "https://www.promptstdio.com",
                  api_token_set: false,
                  api_token_masked: "",
                };
                config.mcp_profiles.push(profile);
              }
              profile.api_token_set = true;
              profile.api_token_masked = "ps_••••••••";
              config.api_token_set = true;
              config.api_token_masked = "ps_••••••••";
              return snapshot();
            }
            case "send_message": {
              const planMode = (args?.planMode ?? args?.plan_mode) === "on";
              const hist = args?.history;
              const internals = (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> })
                .__TAURI_INTERNALS__;
              internals.lastSendHistory = Array.isArray(hist) ? hist : [];
              internals.lastSendMessage =
                typeof args?.message === "string" ? args.message : "";
              internals.lastSendImages = Array.isArray(args?.images) ? args.images : [];
              if (doReadConfirm) {
                await confirmReadThenRun();
              } else if (doConfirm) {
                await confirmThenRun();
              } else if (doPlan && planMode) {
                await planThenRun();
              } else if (doStream) {
                await streamReply();
              }
              return null;
            }
            case "clear_agent_session":
              return null;
            case "drop_agent_memory":
              return null;
            case "list_session_checkpoints":
              return [
                {
                  epoch: 2,
                  parent_epoch: 1,
                  compression_level: "full",
                  summary_preview: "当前：完成工具折叠",
                  created_at: "2026-07-29T12:00:00Z",
                },
                {
                  epoch: 1,
                  parent_epoch: 0,
                  compression_level: "full",
                  summary_preview: "先前：落盘权威历史",
                  created_at: "2026-07-29T11:00:00Z",
                },
              ];
            case "diff_session_checkpoints":
              return {
                from_epoch: Number(args?.fromEpoch ?? args?.from_epoch ?? 1),
                to_epoch: Number(args?.toEpoch ?? args?.to_epoch ?? 2),
                summary_changed: true,
                text: "检查点 1 → 2\n摘要有更新\n目标:\n  + 当前：完成工具折叠\n  - 先前：落盘权威历史",
              };
            case "rollback_session_epoch": {
              const epoch = Number(args?.targetEpoch ?? args?.target_epoch ?? 1);
              return {
                epoch,
                summary: "先前：落盘权威历史",
                resume_text: "Goals:\n- 落盘权威历史",
              };
            }
            case "gc_orphan_agent_sessions":
              return 0;
            case "latest_workspace_checkpoint":
              return {
                session_id: "peer-sess",
                epoch: 2,
                summary_preview: "先前工作区检查点摘要",
                resume_text: "Goals:\n- 跨会话续作",
                created_at: "2026-07-29T12:00:00Z",
              };
            case "plugin:event|listen": {
              const eventName = typeof args?.event === "string" ? args.event : "";
              const handler = args?.handler;
              const id =
                typeof handler === "number"
                  ? handler
                  : typeof args?.handler === "number"
                    ? (args.handler as number)
                    : nextCb++;
              if (eventName === "agent-event") {
                agentListeners.add(id);
              }
              return id;
            }
            case "plugin:event|unlisten":
              if (typeof args?.eventId === "number") {
                agentListeners.delete(args.eventId as number);
              }
              return null;
            case "plugin:event|unlisten":
              if (typeof args?.eventId === "number") {
                agentListeners.delete(args.eventId as number);
              }
              return null;
            case "snap_compact_window":
              return null;
            case "set_compact_mode":
              return null;
            case "save_window_state": {
              const internals = (
                window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }
              ).__TAURI_INTERNALS__;
              internals.saveWindowStateCalls =
                ((internals.saveWindowStateCalls as number | undefined) ?? 0) + 1;
              internals.lastSaveWindowStateArgs = args ?? null;
              return null;
            }
            default:
              console.warn("[e2e mock] unhandled invoke:", cmd, args);
              return null;
          }
        },
      };
    },
    {
      apiKeySet,
      apiTokenSet,
      workDir,
      streamChat,
      streamDoneReply,
      streamSlow,
      streamRunningTool,
      streamToolOutput,
      streamDesktopTool,
      streamCapDone,
      streamHtml,
      planFlow,
      suiteFail,
      suiteAuthFail,
      confirmFlow,
      readConfirmFlow: doReadConfirm,
      usageLayers: layersOn,
      visionModel: visionOn,
      localVision: localVisionOn,
      allowRules: seededRules,
      testConnectionFail,
      testPromptstdioFail,
      isMember,
      updateAvailable,
      streamFailTool: failTool,
      announceAvailable: announceOn,
    },
  );
}
