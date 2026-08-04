<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import "../app.css";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import ChatView from "$lib/components/ChatView.svelte";
  import WorkDirDialog from "$lib/components/WorkDirDialog.svelte";
  import CheckpointDialog from "$lib/components/CheckpointDialog.svelte";
  import DiagBanner from "$lib/components/DiagBanner.svelte";
  import ToastStack from "$lib/components/ToastStack.svelte";
  import { pushToast } from "$lib/stores/toasts";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import ShortcutsDialog from "$lib/components/ShortcutsDialog.svelte";
  import {
    paletteOpen,
    shortcutsOpen,
    chatFindOpen,
    togglePalette,
    toggleShortcuts,
  } from "$lib/stores/palette";
  import {
    initState,
    refreshConfig,
    refreshWorkDir,
    installWorkDirHooks,
    openConfirm,
    dismissConfirm,
    clearConfirmSessionAllow,
    skillRecording,
    skillRecordSteps,
    lastSendSource,
    toggleSidebar,
    autoContinueEnabled,
    shouldAutoContinue,
    requestAutoContinue,
  } from "$lib/stores/app";
  import { syncFromLiveWorkDir } from "$lib/stores/workspaces";
  import { nav, installNavHooks } from "$lib/nav.svelte";
  import { diag, diagError, installGlobalDiagHandlers } from "$lib/diag";
  import { formatElapsed } from "$lib/output-format";
  import { compact, compactLabel } from "$lib/stores/compact.svelte";
import { AUTO_CONTINUE_DELAY_MS, REVEAL_SAFETY_MS, WINDOW_PERSIST_DEBOUNCE_MS } from "$lib/timing";
import { stream } from "$lib/stores/stream.svelte";
  import {
    ensureSession,
    ensureSessionLlm,
    hygieneSessions,
    appendItem,
    appendToolDetail,
    insertItemBefore,
    moveItemToEnd,
    patchItem,
    removeItem,
    newMessage,
    newTool,
    newPlan,
    findLatestPlanId,
    flushSessionPersist,
    sessionsData,
    prefillSedimentCandidate,
    markUndoneToolsStopped,
    markActivePlanInterrupted,
    currentSessionId,
    createSession,
  } from "$lib/stores/sessions";
  import type { PlanStep } from "$lib/types";
  import { initTheme, theme } from "$lib/stores/theme";
  import { installNativeContextMenu } from "$lib/native-context-menu";
import { terminalOpen, toggleTerminal } from "$lib/terminal/store";
  import {
    finishStartup,
    clearTaskbarProgress,
    listenAgentEvents,
    trackUsage,
    cancelGeneration,
    saveWindowState,
    snapCompactWindow,
  } from "$lib/ipc";
  import { applyDoneUsage, applyUsageEvent } from "$lib/stores/usage";

  let entering = $state(true);
  const pendingTools = new Map<string, string>();
  /** ADR-037 live tool output: per-tool line buffer + one rAF flush. */
  const toolOutputBuf = new Map<string, string>();
  let toolOutputRaf: number | null = null;

  const DESKTOP_TOOLS = new Set([
    "desktop_screenshot", "desktop_click", "desktop_type",
    "desktop_key", "desktop_scroll", "desktop_hover",
    "desktop_window_action", "desktop_browser",
    "desktop_app_launch", "desktop_window_list",
  ]);

  // Stopwatch for the whole compact turn (survives fast tool churn).
  // $effect must live in a component context — module scope is invalid.
  $effect(() => {
    if (!compact.mode) {
      compact.elapsedMs = 0;
      return;
    }
    const tick = () => {
      compact.elapsedMs = Date.now() - compact.since;
    };
    tick();
    const id = window.setInterval(tick, 500);
    return () => window.clearInterval(id);
  });

  function paintReady(): Promise<void> {
    return new Promise((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
  }

    let revealed = false;

  // Window geometry persistence: debounce persist on resize / move / close.
  let maximizedState = false;
  let persistTimer: ReturnType<typeof setTimeout> | null = null;

  let snapTimer: ReturnType<typeof setTimeout> | null = null;

  function schedulePersistWindowState(immediate = false) {
    if (persistTimer) clearTimeout(persistTimer);
    if (immediate) {
      persistTimer = null;
      void saveWindowState(maximizedState).catch(() => {});
      return;
    }
    persistTimer = setTimeout(() => {
      persistTimer = null;
      void saveWindowState(maximizedState).catch(() => {});
    }, WINDOW_PERSIST_DEBOUNCE_MS);
  }

  function installWindowStatePersistence() {
    // Mock (Layer A) browsers have no Tauri event API; guard before subscribing.
    const tauriEvents = (
      window as unknown as { __TAURI__?: { event?: unknown } }
    ).__TAURI__?.event;
    if (!tauriEvents) return () => {};

    let unlistenResize: (() => void) | undefined;
    let unlistenMove: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    let disposed = false;

    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      try {
        maximizedState = await win.isMaximized();
      } catch {
        /* ignore */
      }
      if (disposed) return;
      unlistenResize = await win.onResized(() => {
        void win
          .isMaximized()
          .then((m) => {
            maximizedState = m;
            schedulePersistWindowState();
          })
          .catch(() => {});
      });
      unlistenMove = await win.onMoved(() => {
        schedulePersistWindowState();
        // 浮条拖拽停止后自动吸附屏幕边缘（防抖 400ms）
        if (compact.mode) {
          if (snapTimer) clearTimeout(snapTimer);
          snapTimer = setTimeout(() => {
            snapTimer = null;
            void snapCompactWindow().catch(() => {});
          }, WINDOW_PERSIST_DEBOUNCE_MS);
        }
      });
      unlistenClose = await win.onCloseRequested(() =>
        schedulePersistWindowState(true),
      );
      // 窗口失焦态：顶栏降档（mock 环境无 Tauri 事件——属性缺省即聚焦态）
      unlistenFocus = await win.onFocusChanged(({ payload }) => {
        document.documentElement.setAttribute(
          "data-window-focused",
          String(payload),
        );
      });
    })().catch(() => {});

    return () => {
      disposed = true;
      unlistenResize?.();
      unlistenMove?.();
      unlistenClose?.();
      unlistenFocus?.();
      if (persistTimer) clearTimeout(persistTimer);
    };
  }

  /** Persist window geometry from the outside (tests / shutdown paths). */
  function requestWindowStatePersist(): void {
    schedulePersistWindowState(true);
  }

  function dismissLoader() {
    const loader = document.getElementById("app-loader");
    if (!loader) return;
    loader.style.pointerEvents = "none";
    loader.classList.add("fade-out");
    setTimeout(() => loader.remove(), 500);
  }

  async function revealWindow() {
    if (revealed) {
      entering = false;
      nav.markShellReady();
      dismissLoader();
      return;
    }
    revealed = true;
    await paintReady();
    try {
      await finishStartup(get(theme) === "dark");
    } catch {
      /* ignore */
    }
    entering = false;
    nav.markShellReady();
    dismissLoader();
    setTimeout(() => {
      void clearTaskbarProgress();
    }, 4000);
  }

  function activeStreamSid(): string | null {
    return stream.streamSessionId;
  }

  /** Stable key for concurrent tool calls (ADR-037). */
  function toolCallKey(ev: { call_id?: string; name: string }): string {
    return ev.call_id || ev.name;
  }

  function endStreamUi() {
    stream.endTurn();
    pendingTools.clear();
    dismissConfirm();
    clearConfirmSessionAllow();
    flushSessionPersist();
  }

  /** Commit live streamingContent into the bubble (done / cancelled). */
  function flushStreamRender() {
    const id = stream.streamingBubbleId;
    if (!id) return;
    patchItem(id, { content: stream.streamingContent }, activeStreamSid());
  }

  function isCancelMessage(msg: string | undefined): boolean {
    const m = (msg || "").toLowerCase();
    return m.includes("cancelled") || m.includes("canceled") || m.includes("已停止");
  }

  /** Always leave a stopped bubble — including cancel before the first token. */
  function markStreamStopped(sid: string | null) {
    const id = stream.streamingBubbleId;
    if (id) {
      const content = stream.streamingContent
        .replace(/\n\n— 已停止生成\s*$/, "")
        .trim();
      patchItem(
        id,
        {
          content: content ? `${content}\n\n— 已停止生成` : "— 已停止生成",
          stopped: true,
        },
        sid,
      );
      return;
    }
    if (sid) {
      appendItem(newMessage("assistant", "— 已停止生成", false, { stopped: true }), sid);
    }
  }

  onMount(() => {
    initTheme();
    document.documentElement.removeAttribute("data-compact");
    installGlobalDiagHandlers();
    installNavHooks();
    installWorkDirHooks();
    const uninstallContextMenu = installNativeContextMenu();
    const uninstallWindowState = installWindowStatePersistence();
    diag("bootstrap start");

    let unlisten: (() => void) | undefined;

    async function bootstrap() {
      const cfg = await refreshConfig();
      await refreshWorkDir();
      syncFromLiveWorkDir();
      hygieneSessions();
      diag(
        `config loaded key_set=${!!cfg?.llm_api_key_set} token_set=${!!cfg?.api_token_set}`,
      );

      if (cfg?.llm_api_key_set) {
        ensureSession();
        ensureSessionLlm();
        nav.showChat("bootstrap");
      } else {
        nav.showSettings({ firstRun: true });
      }

      // Reveal UI before event wiring — never leave the splash overlay
      // blocking clicks while listen() is pending.
      initState.set({ phase: "ready" });
      await revealWindow();
      diag(`revealed; view=${nav.view}`);

      unlisten = await listenAgentEvents((ev) => {
          const sid = activeStreamSid();
          switch (ev.type) {
            case "token": {
              stream.appendToken(ev.text ?? "");
              let id = stream.streamingBubbleId;
              if (!id) {
                // Placeholder only — live text comes from streamingContent (no per-token store writes).
                const item = newMessage("assistant", "");
                appendItem(item, sid);
                stream.streamingBubbleId = item.id;
              }
              break;
            }
            case "tool_start": {
              if (get(skillRecording)) {
                skillRecordSteps.update((n) => n + 1);
              }
              // 不再自动打开终端面板（用户反馈命令执行时输入区上方弹出
              // 终端区很怪；主流做法是输出留在消息流工具卡内）。终端仍可
              // 经命令面板手动打开。
              const tool = newTool(ev.name, {
                recorded: get(skillRecording),
              });
              const aid = stream.streamingBubbleId;
              if (aid) insertItemBefore(aid, tool, sid);
              else appendItem(tool, sid);
              pendingTools.set(toolCallKey(ev), tool.id);
              // Auto-enter compact mode when desktop tools run; track the
              // current tool for the label. A lingering「已完成」hold from a
              // previous turn yields to the new turn immediately.
              // 自动变形已取消（用户反馈变形形态不佳）——桌面工具执行时
              // 保持全窗，仅记录当前工具供手动浮条显示。
              if (DESKTOP_TOOLS.has(ev.name)) {
                compact.beginRun();
                compact.tool = ev.name;
              }
              break;
            }
            case "tool_output": {
              const id = pendingTools.get(toolCallKey(ev));
              if (!id) break;
              // Buffer per-tool lines and flush once per frame — a chatty
              // command (npm install) would otherwise re-render per line.
              toolOutputBuf.set(id, (toolOutputBuf.get(id) ?? "") + (ev.text ?? ""));
              if (toolOutputRaf === null) {
                toolOutputRaf = requestAnimationFrame(() => {
                  toolOutputRaf = null;
                  for (const [tid, chunk] of toolOutputBuf) {
                    appendToolDetail(tid, chunk, sid);
                  }
                  toolOutputBuf.clear();
                });
              }
              break;
            }
            case "tool_done": {
              const key = toolCallKey(ev);
              const id = pendingTools.get(key);
              if (id) {
                // Flush any buffered live output before merging the summary.
                if (toolOutputRaf !== null) {
                  cancelAnimationFrame(toolOutputRaf);
                  toolOutputRaf = null;
                }
                const pending = toolOutputBuf.get(id) ?? "";
                toolOutputBuf.delete(id);
                if (pending) {
                  appendToolDetail(id, pending, sid);
                }
                let liveDetail = "";
                const data = get(sessionsData);
                if (sid && data.sessions[sid]) {
                  const item = data.sessions[sid].messages.find((m) => m.id === id);
                  if (item?.type === "tool") {
                    liveDetail = item.detail ?? "";
                  }
                }
                const mergedDetail = liveDetail + (ev.summary || "");
                patchItem(
                  id,
                  {
                    done: true,
                    error: !ev.success,
                    summary: ev.summary || (ev.success ? "完成" : "失败"),
                    detail: mergedDetail || ev.summary || "",
                    // Success stays collapsed; failures auto-expand for observability
                    expanded: !ev.success,
                    metrics: ev.metrics,
                  },
                  sid,
                );
                pendingTools.delete(key);
              }
              if (DESKTOP_TOOLS.has(ev.name) && compact.tool === ev.name) {
                compact.tool = null;
              }
              break;
            }
            case "confirm_request":
              openConfirm(ev.id, ev.tool, ev.message);
              break;
            case "usage": {
              applyUsageEvent(ev);
              break;
            }
            case "notice": {
              if (ev.message) pushToast(ev.message, "info");
              break;
            }
            case "done": {
              flushStreamRender();
              const id = stream.streamingBubbleId;
              const responseText = (ev.response || "").trim();
              if (id && responseText) {
                patchItem(
                  id,
                  { content: responseText, hitCap: !!ev.hit_iteration_cap },
                  sid,
                );
              }
              // Final answer after tools; prefill sediment quietly (ADR-036).
              if (id) moveItemToEnd(id, sid);
              prefillSedimentCandidate(sid, responseText);
              applyDoneUsage(ev);
              try {
                const session = sid ? get(sessionsData).sessions[sid] : null;
                const toolNames = (session?.messages ?? [])
                  .filter((m) => m.type === "tool")
                  .map((m) => m.name)
                  .filter(Boolean);
                const unique = [...new Set(toolNames)];
                void trackUsage("stitch_chat_done", {
                  from: get(lastSendSource) || "chat",
                  tools: unique.slice(0, 12).join(","),
                  tool_count: String(toolNames.length),
                });
                lastSendSource.set("chat");
              } catch {
                /* ignore analytics */
              }
              endStreamUi();
              compact.clearRun();
              compact.scheduleExit();
              if (sid && sid !== get(currentSessionId)) {
                const title = get(sessionsData).sessions[sid]?.title || "后台会话";
                pushToast(`「${title}」已完成回复`);
              }
              // Auto-chain「继续执行」on iteration cap, capped per session;
              // a manual send resets the chain (resetAutoContinue in ChatView).
              if (
                ev.hit_iteration_cap &&
                sid &&
                get(autoContinueEnabled) &&
                shouldAutoContinue(sid)
              ) {
                const contSid = sid;
                setTimeout(() => {
                  if (!stream.isStreaming && stream.streamSessionId === contSid) {
                    void requestAutoContinue(contSid);
                  }
                }, AUTO_CONTINUE_DELAY_MS);
              }
              break;
            }
            case "cancelled": {
              flushStreamRender();
              markUndoneToolsStopped(sid);
              markActivePlanInterrupted(sid);
              markStreamStopped(sid);
              endStreamUi();
              compact.clearRun();
              // Pinned (manual) mode keeps the bar — only auto mode restores.
              if (!compact.pinned) void compact.exit();
              break;
            }
            case "error": {
              if (isCancelMessage(ev.message)) {
                flushStreamRender();
                markUndoneToolsStopped(sid);
                markActivePlanInterrupted(sid);
                markStreamStopped(sid);
                endStreamUi();
                compact.clearRun();
                if (!compact.pinned) void compact.exit();
                break;
              }
              const pid = findLatestPlanId(sid);
              if (pid && sid) {
                const plan = get(sessionsData).sessions[sid]?.messages.find(
                  (m) => m.id === pid && m.type === "plan",
                );
                if (plan && plan.type === "plan") {
                  const baseSteps = Array.isArray(plan.steps) ? plan.steps : [];
                  const steps: PlanStep[] = baseSteps.map((s) =>
                    s.status === "in_progress"
                      ? { ...s, status: "failed" }
                      : s.status === "pending"
                        ? { ...s, status: "skipped" }
                        : s,
                  );
                  patchItem(pid, { steps }, sid);
                }
              }
              const id = stream.streamingBubbleId;
              if (id) removeItem(id, sid);
              appendItem(newMessage("assistant", ev.message || "发生错误", true), sid);
              endStreamUi();
              if (sid && sid !== get(currentSessionId)) {
                pushToast(`后台会话出错：${(ev.message || "发生错误").slice(0, 80)}`, "error");
              }
              break;
            }
            case "plan_proposed": {
              appendItem(newPlan(ev.plan, ev.id), sid);
              break;
            }
            case "plan_rejected": {
              const pid = findLatestPlanId(sid);
              if (pid) patchItem(pid, { phase: "rejected" }, sid);
              endStreamUi();
              break;
            }
            case "plan_approved": {
              const pid = findLatestPlanId(sid);
              if (pid) patchItem(pid, { phase: "approved" }, sid);
              break;
            }
            case "plan_step_start": {
              const pid = findLatestPlanId(sid);
              if (!pid || !sid) break;
              const plan = get(sessionsData).sessions[sid]?.messages.find(
                (m) => m.id === pid && m.type === "plan",
              );
              if (plan && plan.type === "plan") {
                let steps: PlanStep[] = [...(Array.isArray(plan.steps) ? plan.steps : [])];
                while (steps.length <= ev.index) {
                  steps.push({
                    description: `步骤 ${steps.length + 1}`,
                    status: "pending",
                  });
                }
                steps = steps.map((s, i) =>
                  i === ev.index
                    ? {
                        ...s,
                        status: "in_progress",
                        description: ev.description || s.description,
                      }
                    : s.status === "in_progress"
                      ? { ...s, status: "pending" }
                      : s,
                );
                patchItem(pid, { steps }, sid);
              }
              break;
            }
            case "plan_step_done": {
              const pid = findLatestPlanId(sid);
              if (!pid || !sid) break;
              const plan = get(sessionsData).sessions[sid]?.messages.find(
                (m) => m.id === pid && m.type === "plan",
              );
              if (plan && plan.type === "plan") {
                let steps: PlanStep[] = [...(Array.isArray(plan.steps) ? plan.steps : [])];
                while (steps.length <= ev.index) {
                  steps.push({
                    description: `步骤 ${steps.length + 1}`,
                    status: "pending",
                  });
                }
                steps = steps.map((s, i) =>
                  i === ev.index
                    ? { ...s, status: "done", description: ev.description || s.description }
                    : s,
                );
                patchItem(pid, { steps }, sid);
              }
              break;
            }
          }
        });
    }

    // Hard safety: if bootstrap hangs, still unlock the UI.
    const safety = setTimeout(() => {
      entering = false;
      nav.markShellReady();
      dismissLoader();
      void finishStartup(get(theme) === "dark").catch(() => {});
      diag("safety reveal fired", "error");
    }, REVEAL_SAFETY_MS);

    void (async () => {
      try {
        await bootstrap();
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        diagError(e, "init failed");
        initState.set({ phase: "error", message: msg });
        try {
          await revealWindow();
        } catch {
          entering = false;
          dismissLoader();
        }
      } finally {
        clearTimeout(safety);
      }
    })();

    function onKey(e: KeyboardEvent) {
      const meta = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();
      if (meta && key === "k") {
        e.preventDefault();
        togglePalette();
        return;
      }
      if (meta && key === ",") {
        e.preventDefault();
        if (nav.view === "chat") nav.showSettings({ fromChat: true });
        else if (!nav.settingsFirstRun) nav.showChat("hotkey");
        return;
      }
      if (meta && key === "b") {
        e.preventDefault();
        toggleSidebar();
        return;
      }
      if (meta && key === "n") {
        e.preventDefault();
        if (stream.isStreaming && stream.streamSessionId) {
          void cancelGeneration().catch(() => {});
        }
        createSession();
        if (nav.view !== "chat" && !nav.settingsFirstRun) nav.showChat("hotkey");
        return;
      }
      if (meta && (key === "/" || key === "?")) {
        e.preventDefault();
        toggleShortcuts();
        return;
      }
      if (meta && key === "f") {
        e.preventDefault();
        if (nav.view === "chat") chatFindOpen.update((v) => !v);
        return;
      }
      if (meta && e.shiftKey && key === "c") {
        e.preventDefault();
        void compact.toggle();
        return;
      }
      if (e.key === "Escape") {
        if (get(chatFindOpen)) {
          chatFindOpen.set(false);
          return;
        }
        if (get(paletteOpen)) {
          paletteOpen.set(false);
          return;
        }
        if (get(shortcutsOpen)) {
          shortcutsOpen.set(false);
          return;
        }
        if (stream.isStreaming) {
          void cancelGeneration().catch(() => {});
        }
      }
    }
    window.addEventListener("keydown", onKey);

    return () => {
      uninstallContextMenu();
      uninstallWindowState();
      window.removeEventListener("keydown", onKey);
      unlisten?.();
    };
  });
</script>

<div class="app-frame" data-testid="app-frame">
  <div class="compact-bar" data-testid="compact-bar">
    <div class="compact-drag" data-tauri-drag-region>
      {#if compact.finished}
        <svg
          class="compact-done"
          data-testid="compact-done"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.75"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="9" />
          <path d="M8.5 12.5l2.5 2.5 4.5-5" />
        </svg>
      {:else if compact.tool || stream.isStreaming}
        <span class="compact-pulse" aria-hidden="true">
          <span class="compact-spinner"></span>
        </span>
      {:else}
        <!-- Manual (pinned) idle state: static dot, no pulse. -->
        <span class="compact-idle" aria-hidden="true"></span>
      {/if}
      <span class="compact-text" data-tauri-drag-region>
        <span class="compact-label" data-testid="compact-tool">{compactLabel()}</span>
      </span>
    </div>
    {#if compact.tool || stream.isStreaming}
      <button
        type="button"
        class="compact-stop"
        aria-label="停止生成"
        onclick={() => void cancelGeneration().catch(() => {})}
      >
        停止
      </button>
    {/if}
    <button
      type="button"
      class="compact-expand"
      data-testid="compact-expand"
      onclick={() => void compact.exit()}
    >
      展开
    </button>
  </div>
  <DiagBanner />

  {#if $initState.phase === "error"}
    <div
      class="app-shell items-center justify-center"
      style="color: var(--color-foreground);"
      data-testid="boot-error"
    >
      <div class="flex flex-col items-center gap-6 max-w-sm text-center px-6">
        <div class="text-sm font-semibold" style="color: var(--color-error);">注意</div>
        <div>
          <h1 class="text-lg font-bold text-[var(--color-foreground)]">启动失败</h1>
          <p class="mt-2 text-sm text-[var(--color-muted)] leading-relaxed">
            {$initState.message || "未知错误"}
          </p>
        </div>
        <button
          class="px-5 py-2 rounded-lg text-sm font-semibold transition-colors"
          style="background: var(--color-brand-accent); color: var(--color-on-accent);"
          onmouseenter={(e) => (e.currentTarget.style.opacity = "0.9")}
          onmouseleave={(e) => (e.currentTarget.style.opacity = "1")}
          onclick={() => {
            initState.set({ phase: "loading" });
            location.reload();
          }}
        >
          重试
        </button>
      </div>
    </div>
  {:else if nav.view === "settings"}
    <div class="app-shell" data-testid="settings-shell">
      <SettingsView />
    </div>
  {:else}
    <div class="app-shell" data-testid="chat-shell">
      <ChatView />
    </div>
  {/if}
</div>

<WorkDirDialog />
<CheckpointDialog />
<CommandPalette />
<ShortcutsDialog />
<ToastStack />
