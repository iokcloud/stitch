<script lang="ts">
  import { tick } from "svelte";
  import {
    config,
    workDir,
    sidebarCollapsed,
    toggleSidebar,
    lastUserMessage,
    lastSendSource,
    workDirDialogOpen,
    applyWorkDir,
    planMode,
    setPlanMode,
    sidebarTab,
    confirmOpen,
    confirmTool,
    clearConfirmSessionAllow,
    composerFillRequest,
    skillRecording,
    skillRecordStartTime,
    skillRecordSteps,
    matureSoftGate,
    muteSoftGate,
    autoContinueRequest,
    resetAutoContinue,
  } from "../stores/app";
import { stream } from "../stores/stream.svelte";
  import { pushToast } from "../stores/toasts";
  import { nav } from "../nav.svelte";
  import {
    currentSession,
    currentSessionId,
    createSession,
    switchSession,
    ensureSession,
    appendItem,
    removeItem,
    removeItemsAfter,
    removeItemsFrom,
    newMessage,
    newSediment,
    buildSedimentPayload,
    peekSedimentCandidate,
    clearSedimentCandidate,
    summarizeSessionTitle,
    historyForSend,
    deferSessionPersist,
    flushSessionPersist,
    setSessionLlm,
    peekLatestWorkspaceCheckpoint,
    applyLatestWorkspaceCheckpoint,
    ensureSessionLlm,
    defaultSessionLlm,
    sessionsData,
    renameSession,
    markUndoneToolsStopped,
    markActivePlanInterrupted,
  } from "../stores/sessions";
  import { themePreference, toggleTheme, themeAriaLabel } from "../stores/theme";
  import { PROVIDER_PRESETS, modelSupportsVision, type LlmProfileSnapshot } from "../types";
  import { formatElapsed } from "../output-format";
  import { usage, contextPct, formatTokenCount, resetTurnUsage } from "../stores/usage";
  import { compact } from "../stores/compact.svelte";
  import * as ipc from "../ipc";
  import { RECOMMENDED_SCENES } from "../scenes";
  import { WORKDIR_NUDGE_KEY } from "../types";
  import MessageBubble from "./MessageBubble.svelte";
  import ChatFindBar from "./ChatFindBar.svelte";
  import CapabilityRail from "./CapabilityRail.svelte";
  import ToolStatus from "./ToolStatus.svelte";
  import ToolGroup from "./ToolGroup.svelte";
  import { groupTimeline } from "../tool-timeline";
  import PlanCard from "./PlanCard.svelte";
  import ConfirmCard from "./ConfirmCard.svelte";
  import SedimentCard from "./SedimentCard.svelte";
  import LibraryPanel from "./LibraryPanel.svelte";
  import WorkspacePanel from "./WorkspacePanel.svelte";
  import TerminalPanel from "./TerminalPanel.svelte";
  import { terminalOpen, toggleTerminal } from "../terminal/store";
  import { get } from "svelte/store";
  import { loadComposerHistory, pushComposerHistory } from "../composer-history";
  import type { TimelineBlock } from "../tool-timeline";

  let input = $state("");
  /** ↑/↓ send-history navigation: -1 = not navigating. */
  let historyIdx = $state(-1);
  let draftBeforeNav = $state("");
  let modelMenuOpen = $state(false);
  let modelQuery = $state("");
  /** Edit-resend: the turn being replaced by the next send. */
  let editRewind = $state<{ itemId: string; original: string } | null>(null);
  /** Pasted image previews pending send (data URLs, in-memory only). */
  type PendingImage = { dataUrl: string; name: string; size: number };
  let pendingImages = $state<PendingImage[]>([]);
  // 与 Rust 侧校验一致（commands.rs send_message：单张 ≤6MB · 单条 ≤9 张）
  const MAX_IMAGE_BYTES = 6 * 1024 * 1024;
  const MAX_IMAGES_PER_MSG = 9;
  /** Guidance dialog when the current model cannot take images. */
  let visionGuidanceOpen = $state(false);
  let imageFileEl: HTMLInputElement | undefined = $state();
  /** Skill save dialog — shown when user stops recording. */
  let skillSaveDraft = $state<{
    open: boolean;
    name: string;
    title: string;
    desc: string;
  }>({ open: false, name: "", title: "", desc: "" });
  let messagesEl: HTMLDivElement | undefined = $state();
  /** 长文展开状态（blockKey → 展开）——虚拟化重建块时保持展开。
   * 整体替换赋值（$state proxy 明确拦截——Set mutation 有版本差异风险）。 */
  let expandedBlocks = $state<Record<string, boolean>>({});
  let inputEl: HTMLTextAreaElement | undefined = $state();
  let stickToBottom = $state(true);
  let nowMs = $state(Date.now());
  /** A2: optional prior checkpoint in this work dir (empty welcome only). */
  let prevCheckpoint = $state<{
    epoch: number;
    summary_preview: string;
  } | null>(null);
  let loadingPrevCheckpoint = $state(false);
  const COMPOSER_MAX_H = 120;
const PLAN_MODE_LABELS: Record<string, string> = {
  auto: "计划模式：自动（复杂任务先规划）",
  on: "计划模式：强制（先规划再执行）",
  off: "计划模式：关闭",
};

  /** Last successful assistant message — the only one offering「重新生成」. */
  const lastAssistantId = $derived.by(() => {
    const msgs = $currentSession?.messages ?? [];
    for (let i = msgs.length - 1; i >= 0; i--) {
      const m = msgs[i];
      if (m.type === "message" && m.role === "assistant" && !m.error) {
        return m.id;
      }
    }
    return null;
  });

  $effect(() => {
    ensureSessionLlm();
  });

  $effect(() => {
    const sid = $currentSessionId;
    const empty = ($currentSession?.messages?.length ?? 0) === 0;
    const path = ($currentSession?.workDirPath || $workDir || "").trim();
    if (!sid || !empty || !path) {
      prevCheckpoint = null;
      return;
    }
    let cancelled = false;
    void peekLatestWorkspaceCheckpoint(sid).then((ref) => {
      if (cancelled) return;
      prevCheckpoint = ref
        ? { epoch: ref.epoch, summary_preview: ref.summary_preview }
        : null;
    });
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (!stream.isStreaming) return;
    nowMs = Date.now();
    const id = setInterval(() => {
      nowMs = Date.now();
    }, 1000);
    return () => clearInterval(id);
  });

  type ModelMenuItem = {
    profileId: string;
    label: string;
    model: string;
  };

  const sessionProfileId = $derived(
    $currentSession?.llmProfileId ||
      $config?.active_profile_id ||
      $config?.llm_profiles?.[0]?.id ||
      "",
  );
  const sessionModel = $derived(
    $currentSession?.llmModel || $config?.llm_model || "",
  );
  const sessionProfile = $derived.by((): LlmProfileSnapshot | undefined => {
    const profiles = $config?.llm_profiles ?? [];
    return (
      profiles.find((p) => p.id === sessionProfileId) ||
      profiles.find((p) => p.id === $config?.active_profile_id) ||
      profiles[0]
    );
  });
  const modelLabel = $derived.by(() => {
    const model = sessionModel || "模型";
    const name = sessionProfile?.label?.trim();
    if (name && name !== model) return `${name} · ${model}`;
    return model;
  });
  /**
   * Paste-entry gate: judge the model actually used for sending
   * (`sessionModel` — the topbar menu can differ from the profile default),
   * falling back to the profile's model only when the session has none.
   * Never trust `supports_images` here: it tracks the profile default model
   * and goes stale when the topbar switches models. The local vision
   * describe layer also opens the entry — the backend describes the images
   * locally and the text-only model receives the description text.
   */
  const visionEnabled = $derived(
    modelSupportsVision(sessionModel || sessionProfile?.model || "") ||
      !!$config?.local_vision?.enabled,
  );
  const modelOptions = $derived.by((): ModelMenuItem[] => {
    const profiles = $config?.llm_profiles ?? [];
    const items: ModelMenuItem[] = [];
    const seen = new Set<string>();
    const push = (profileId: string, label: string, model: string) => {
      const m = model.trim();
      if (!profileId || !m) return;
      const key = `${profileId}::${m}`;
      if (seen.has(key)) return;
      seen.add(key);
      items.push({ profileId, label: label || profileId, model: m });
    };
    if (profiles.length === 0) {
      const fallback = defaultSessionLlm($config);
      if (fallback.llmProfileId && fallback.llmModel) {
        push(
          fallback.llmProfileId,
          $config?.llm_provider || "模型",
          fallback.llmModel,
        );
      }
      return items;
    }
    for (const p of profiles) {
      const preset = PROVIDER_PRESETS[p.provider] || PROVIDER_PRESETS.custom;
      const models = [...preset.models];
      if (p.model && !models.includes(p.model)) models.unshift(p.model);
      if (
        sessionProfileId === p.id &&
        sessionModel &&
        !models.includes(sessionModel)
      ) {
        models.unshift(sessionModel);
      }
      for (const m of models) {
        push(p.id, p.label || p.id, m);
      }
    }
    return items;
  });

  const filteredModelOptions = $derived.by(() => {
    const q = modelQuery.trim().toLowerCase();
    if (!q) return modelOptions;
    return modelOptions.filter((o) =>
      `${o.label} ${o.model}`.toLowerCase().includes(q),
    );
  });

  const items = $derived($currentSession?.messages ?? []);
  /** While streaming, keep tools as singles to avoid group remount flicker. */
  const timelineRaw = $derived(
    stream.isStreaming
      ? items.map((item, index) => ({ kind: "single" as const, item, index }))
      : groupTimeline(items),
  );

  // Memoize timeline blocks so a single-item append / patch does not
  // recreate every block object (and therefore every keyed child) —
  // the dominant cost on long sessions.
  const timelineMemoCache = new Map<string, TimelineBlock>();
  let timelineMemoSource: TimelineBlock[] | null = null;
  let timelineMemoResult: TimelineBlock[] = [];

  function memoizeTimeline(raw: TimelineBlock[]): TimelineBlock[] {
    if (raw === timelineMemoSource) return timelineMemoResult;
    const nextCache = new Map<string, TimelineBlock>();
    const result = raw.map((block) => {
      const key = blockKey(block);
      const prev = timelineMemoCache.get(key);
      if (prev && timelineBlocksEqual(prev, block)) {
        nextCache.set(key, prev);
        return prev;
      }
      nextCache.set(key, block);
      return block;
    });
    timelineMemoCache.clear();
    for (const [k, v] of nextCache) timelineMemoCache.set(k, v);
    timelineMemoSource = raw;
    timelineMemoResult = result;
    return result;
  }

  function timelineBlocksEqual(a: TimelineBlock, b: TimelineBlock): boolean {
    if (a.kind !== b.kind) return false;
    if (a.kind === "single" && b.kind === "single") {
      return a.item === b.item && a.index === b.index;
    }
    if (a.kind === "tool_group" && b.kind === "tool_group") {
      if (a.startIndex !== b.startIndex) return false;
      if (a.items.length !== b.items.length) return false;
      for (let i = 0; i < a.items.length; i++) {
        if (a.items[i] !== b.items[i]) return false;
      }
      return true;
    }
    return false;
  }

  const timeline = $derived.by(() => memoizeTimeline(timelineRaw));

  // Timeline virtualization (chat-log window): render only blocks near the
  // viewport; keep an invisible top spacer so scrollbar/scrollIntoView stay
  // coherent. Item counts are small enough that height-estimation stays cheap.
  const EST_TOOL = 72;
  const EST_MSG = 180;
  const EST_PLAN = 260;
  const EST_SEDIMENT = 300;
  const BLOCK_GAP = 14;
  const OVERSCAN = 5;

  let scrollTop = $state(0);
  let viewportH = $state(720);
  let measuredHeights = $state<Record<string, number>>({});

  function blockKey(block: TimelineBlock): string {
    return block.kind === "single"
      ? block.item.id
      : `g-${block.items.map((t) => t.id).join("-")}`;
  }

  function estimateBlockHeight(block: TimelineBlock): number {
    if (block.kind === "tool_group") {
      return 44 + block.items.length * 30;
    }
    const item = block.item;
    if (item.type === "tool") return EST_TOOL;
    if (item.type === "plan") return EST_PLAN;
    if (item.type === "sediment") return EST_SEDIMENT;
    return EST_MSG;
  }

  const virtualizationActive = $derived(timeline.length > 40);

  const virtualWindow = $derived.by(() => {
    const blocks = timeline;
    if (!virtualizationActive) {
      return { start: 0, end: blocks.length, topSpacer: 0, bottomSpacer: 0 };
    }
    const heights = new Array<number>(blocks.length);
    let total = 0;
    for (let i = 0; i < blocks.length; i++) {
      const k = blockKey(blocks[i]);
      heights[i] = measuredHeights[k] ?? estimateBlockHeight(blocks[i]);
      total += heights[i];
    }
    total += Math.max(0, blocks.length - 1) * BLOCK_GAP;

    // stickToBottom is explicit pin intent: anchor the window to the tail
    // even while the real DOM height (and scrollTop state) still lags.
    const nearBottom = stickToBottom || total - scrollTop - viewportH < 120;
    const viewTop = nearBottom ? Math.max(0, total - viewportH) : scrollTop;

    const minTop = Math.max(0, viewTop - viewportH * 1.5);
    const maxBottom = viewTop + viewportH * 2.5;

    let start = 0;
    let accTop = 0;
    while (start < blocks.length && accTop + heights[start] + BLOCK_GAP < minTop) {
      accTop += heights[start] + BLOCK_GAP;
      start++;
    }
    start = Math.max(0, start - OVERSCAN);

    let end = blocks.length;
    let accBottom = total;
    while (end > start && accBottom - (heights[end - 1] + BLOCK_GAP) > maxBottom) {
      accBottom -= heights[end - 1] + BLOCK_GAP;
      end--;
    }
    end = Math.min(blocks.length, end + OVERSCAN);

    let topSpacer = 0;
    for (let i = 0; i < start; i++) topSpacer += heights[i] + BLOCK_GAP;
    let bottomSpacer = 0;
    for (let i = end; i < blocks.length; i++) bottomSpacer += heights[i] + BLOCK_GAP;

    return { start, end, topSpacer, bottomSpacer };
  });

  const visibleBlocks = $derived(
    virtualizationActive
      ? timeline.slice(virtualWindow.start, virtualWindow.end)
      : timeline,
  );

  /** 切会话清跨会话残留（blockKey 是消息 id——历史会话键永远不再命中）。 */
  let recordPrunedSession: string | null = null;
  /** 字体就绪后 +1，触发测量 effect 重跑一轮（写入时已按 status 过滤失真值）。 */
  let fontsReadyTick = $state(0);
  let fontsRemeasured = false;
  $effect(() => {
    const sid = $currentSessionId;
    if (sid === recordPrunedSession) return;
    recordPrunedSession = sid;
    expandedBlocks = {};
    measuredHeights = {};
    observedBlockKeys.clear();
    blockRO?.disconnect();
    blockRO = null;
  });

  /** 可见块尺寸异步变化（图片/代码块/流式输出加载）→ ResizeObserver 补测。
   * 曾长期按「渲染时测一次」——窗口滚动后新进入视口的块不重测，图片
   * 异步加载使 spacer 低估 → 向下滚动重叠闪烁（用户实测）。 */
  let blockRO: ResizeObserver | null = null;
  let observedBlockKeys = new Set<string>();
  function remeasureVisibleBlocks(fontsLoaded: boolean) {
    if (!messagesEl) return;
    const nodes = messagesEl.querySelectorAll<HTMLElement>("[data-block-key]");
    if (!nodes.length) return;
    const next: Record<string, number> = {};
    let changed = false;
    for (const el of nodes) {
      const key = el.getAttribute("data-block-key");
      if (!key) continue;
      const h = el.getBoundingClientRect().height;
      if (!fontsLoaded) continue; // 字体未就绪——高度失真，不入缓存
      if (h > 0 && Math.abs((measuredHeights[key] ?? 0) - h) > 2) {
        next[key] = h;
        changed = true;
      }
    }
    if (changed) {
      measuredHeights = { ...measuredHeights, ...next };
    }
  }
  function ensureBlockObservation() {
    if (!messagesEl || !virtualizationActive) return;
    if (!blockRO) {
      blockRO = new ResizeObserver((entries) => {
        const next: Record<string, number> = {};
        let changed = false;
        for (const e of entries) {
          const key = (e.target as HTMLElement).getAttribute("data-block-key");
          if (!key) continue;
          const h = (e.target as HTMLElement).getBoundingClientRect().height;
          if (h > 0 && Math.abs((measuredHeights[key] ?? 0) - h) > 2) {
            next[key] = h;
            changed = true;
          }
        }
        if (changed) measuredHeights = { ...measuredHeights, ...next };
      });
    }
    const nodes = messagesEl.querySelectorAll<HTMLElement>("[data-block-key]");
    for (const el of nodes) {
      const key = el.getAttribute("data-block-key");
      if (key && !observedBlockKeys.has(key)) {
        observedBlockKeys.add(key);
        blockRO.observe(el);
      }
    }
  }

  $effect(() => {
    // Measure rendered block heights after each paint; feed the estimates.
    // 依赖 expandedBlocks：展开/收起后立刻重测真实高度——否则窗口切片与
    // spacer 一直按折叠态估算走，滚动跨过展开块时会整段跳变。
    // 依赖 virtualWindow.start/end + renderTick：滚动使窗口移动后新进入
    // 视口的块立即重测（修复向下滚动重叠闪烁）。
    expandedBlocks;
    fontsReadyTick;
    virtualWindow.start;
    virtualWindow.end;
    if (!messagesEl || !virtualizationActive) return;
    // WebFont 未就绪时测量的高度失真（~23px vs 真实 80px）且不再重测——
    // 就绪前不写入缓存，就绪后 fontsReadyTick 触发一轮全窗口重测（只一次）。
    if (!fontsRemeasured) {
      fontsRemeasured = true;
      document.fonts?.ready
        .then(() => {
          fontsReadyTick++;
        })
        .catch(() => {});
    }
    const fontsLoaded = document.fonts?.status === "loaded";
    remeasureVisibleBlocks(fontsLoaded);
    ensureBlockObservation();
  });

  function syncScrollMetrics() {
    if (!messagesEl) return;
    scrollTop = messagesEl.scrollTop;
    viewportH = messagesEl.clientHeight;
  }

  $effect(() => {
    if (!messagesEl) return;
    syncScrollMetrics();
    const el = messagesEl;
    const ro = new ResizeObserver(() => syncScrollMetrics());
    ro.observe(el);
    return () => ro.disconnect();
  });
  const streamingId = $derived(stream.streamingBubbleId);

  /**
   * Layered context segments (hot/warm/cold) — only surfaced once a compact
   * actually archived turns into warm/cold. Widths share one scale: each tier
   * as a share of the context limit, capped together at 100%.
   */
  const layerData = $derived.by(() => {
    const l = $usage.layers;
    if (!l || l.warm_entries + l.cold_entries === 0) return null;
    const total = Math.max(l.total_tokens, 1);
    const cap = Math.min(100, (total * 100) / Math.max(l.limit, 1));
    return {
      hot: (l.hot_tokens / total) * cap,
      warm: (l.warm_tokens / total) * cap,
      cold: (l.cold_tokens / total) * cap,
      title: `热 ${formatTokenCount(l.hot_tokens)} · 温 ${formatTokenCount(l.warm_tokens)} · 冷 ${formatTokenCount(l.cold_tokens)} · 归档 ${l.warm_entries + l.cold_entries} 条`,
    };
  });

  /** Live task observability above the composer while generating. */
  const activity = $derived.by(() => {
    type Act = {
      phase: string;
      summary: string;
      format: string;
      elapsedLabel: string;
      progressPct: number | null;
      progressLabel: string;
      indeterminate: boolean;
      tokensLabel: string;
      contextLabel: string;
      iterLabel: string;
    };
    if (!stream.isStreaming) return null as Act | null;
    const started = stream.streamStartedAt ?? nowMs;
    const elapsedLabel = formatElapsed((nowMs - started) / 1000);
    const u = $usage;
    const turnTok = u.inputTokens + u.outputTokens;
    const tokensLabel = turnTok > 0 ? `${formatTokenCount(turnTok)} tok` : "估算中";
    const contextLabel = `Ctx ${formatTokenCount(u.contextTokens)}/${formatTokenCount(u.contextLimit)}`;
    const iterLabel = u.iterations > 0 ? `第 ${u.iterations} 轮` : "准备中";
    const base = {
      elapsedLabel,
      progressPct: null as number | null,
      progressLabel: "",
      indeterminate: false,
      format: "文本",
      tokensLabel,
      contextLabel,
      iterLabel,
    };

    if ($confirmOpen) {
      return {
        ...base,
        phase: "确认",
        summary: $confirmTool ? `等待确认 · ${$confirmTool}` : "等待确认",
        format: "确认请求",
        indeterminate: true,
      };
    }
    for (let i = items.length - 1; i >= 0; i--) {
      const m = items[i];
      if (m.type === "tool" && !m.done) {
        return {
          ...base,
          phase: "工具",
          summary: `正在调用 ${m.name}`,
          format: "工具输出",
          indeterminate: true,
          progressLabel: "执行中",
        };
      }
      if (m.type === "plan") {
        const planSteps = Array.isArray(m.steps) ? m.steps : [];
        const doneCount = planSteps.filter((s) => s.status === "done").length;
        const pct = planSteps.length ? Math.round((doneCount / planSteps.length) * 100) : 0;
        if (m.phase === "proposed") {
          return {
            ...base,
            phase: "计划",
            summary: "计划已提出，等待批准",
            format: "计划",
            progressPct: 0,
            progressLabel: `0/${planSteps.length} 步`,
          };
        }
        const stepIdx = planSteps.findIndex((s) => s.status === "in_progress");
        if (stepIdx >= 0) {
          return {
            ...base,
            phase: "执行",
            summary: `第 ${stepIdx + 1}/${planSteps.length} 步 · ${planSteps[stepIdx].description}`,
            format: "计划步骤",
            progressPct: pct,
            progressLabel: `${doneCount}/${planSteps.length} 步`,
          };
        }
        if (m.phase === "approved") {
          return {
            ...base,
            phase: "执行",
            summary: "按计划执行中",
            format: "计划步骤",
            progressPct: pct,
            progressLabel: `${doneCount}/${planSteps.length} 步`,
          };
        }
      }
    }
    if (stream.streamingBubbleId && !stream.streamingContent) {
      return {
        ...base,
        phase: "思考",
        summary: "正在思考…",
        format: "流式",
        indeterminate: true,
        progressLabel: "等待首 token",
      };
    }
    if (stream.streamingBubbleId) {
      const chars = stream.streamingContent.length;
      // Soft receive progress (no server total): asymptote toward 90%.
      const pct = Math.min(90, Math.round(Math.log10(chars + 10) * 28));
      return {
        ...base,
        phase: "回复",
        summary: "正在生成回复…",
        format: "Markdown",
        progressPct: pct,
        progressLabel: `已接收 ${chars} 字`,
      };
    }
    return {
      ...base,
      phase: "处理",
      summary: $planMode !== "off" ? "正在生成计划…" : "处理中…",
      format: $planMode === "on" ? "计划" : $planMode === "auto" ? "自动" : "流式",
      indeterminate: true,
    };
  });

  function onScroll() {
    if (!messagesEl) return;
    syncScrollMetrics();
    const gap = messagesEl.scrollHeight - messagesEl.scrollTop - messagesEl.clientHeight;
    stickToBottom = gap < 80;
  }

  async function scrollToBottom() {
    await tick();
    if (messagesEl && stickToBottom) {
      messagesEl.scrollTop = messagesEl.scrollHeight;
    }
  }

  /** After a turn ends, pin the final answer into view (not buried under tools). */
  async function scrollAnswerIntoView() {
    await tick();
    if (!messagesEl) return;
    const nodes = messagesEl.querySelectorAll(".msg-assistant:not(.is-error)");
    const last = nodes[nodes.length - 1] as HTMLElement | undefined;
    if (last) {
      last.scrollIntoView({ block: "nearest", behavior: "smooth" });
      return;
    }
    stickToBottom = true;
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  async function scrollSedimentIntoView() {
    await tick();
    if (!messagesEl) return;
    const card = messagesEl.querySelector('[data-testid="sediment-card"]') as HTMLElement | null;
    if (card) {
      card.scrollIntoView({ block: "nearest", behavior: "smooth" });
      return;
    }
    stickToBottom = true;
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  /** Reveal sediment card on demand (assistant「保存」); prefer Done prefill. */
  function offerSediment() {
    const sid = $currentSessionId;
    if (!sid) return;
    const session = $currentSession;
    const hasOpen = !!session?.messages.some(
      (m) => m.type === "sediment" && m.status !== "saved",
    );
    if (hasOpen) {
      void scrollSedimentIntoView();
      return;
    }
    const cand = peekSedimentCandidate(sid);
    if (cand) {
      appendItem(newSediment(cand.title, cand.content), sid);
      void scrollSedimentIntoView();
      return;
    }
    const built = buildSedimentPayload(sid);
    if (built) {
      appendItem(newSediment(built.title, built.content), sid);
      void scrollSedimentIntoView();
      return;
    }
    // Fallback when payload rules skip (short reply etc.): still offer last pair.
    let userText = "";
    let assistantText = "";
    for (let i = (session?.messages.length ?? 0) - 1; i >= 0; i--) {
      const m = session!.messages[i];
      if (m.type !== "message" || m.error || m.stopped) continue;
      if (!assistantText && m.role === "assistant" && m.content.trim()) {
        assistantText = m.content.trim();
        continue;
      }
      if (assistantText && m.role === "user" && m.content.trim()) {
        userText = m.content.trim();
        break;
      }
    }
    if (!userText || !assistantText || assistantText.length < 20) return;
    const title = summarizeSessionTitle(userText);
    const content = [
      "## 任务",
      userText.slice(0, 1200),
      "",
      "## 结果",
      assistantText.slice(0, 3500),
    ]
      .join("\n")
      .slice(0, 5000);
    appendItem(newSediment(title, content), sid);
    void scrollSedimentIntoView();
  }

  async function copySessionTitle(title: string, e: MouseEvent) {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(title);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = title;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
  }

  $effect(() => {
    items;
    streamingId;
    $confirmOpen;
    void scrollToBottom();
  });

  /** Follow growing text without tying scroll to every markdown reflow. */
  let streamScrollRaf: number | null = null;
  $effect(() => {
    if (!stream.isStreaming || !stickToBottom) return;
    stream.streamingContent;
    if (streamScrollRaf !== null) return;
    streamScrollRaf = requestAnimationFrame(() => {
      streamScrollRaf = null;
      void scrollToBottom();
    });
  });

  let wasStreaming = false;
  $effect(() => {
    const streaming = stream.isStreaming;
    if (wasStreaming && !streaming) {
      stickToBottom = true;
      void scrollAnswerIntoView();
    }
    wasStreaming = streaming;
  });

  // Iteration-cap auto-continue, scheduled by +page after a capped done.
  let lastAutoContinueNonce = 0;
  $effect(() => {
    const req = $autoContinueRequest;
    if (!req || req.nonce === lastAutoContinueNonce) return;
    lastAutoContinueNonce = req.nonce;
    if (stream.isStreaming) return;
    if ($currentSession?.id !== req.sid) return;
    if (input.trim()) {
      // User is typing — they're taking over; break the chain, keep the button.
      resetAutoContinue(req.sid);
      return;
    }
    void send("继续执行", { autoContinue: true });
  });

  async function send(
    text?: string,
    opts?: {
      skipUserAppend?: boolean;
      fromScene?: boolean;
      sessionTitle?: string;
      autoContinue?: boolean;
      rewindToUser?: string;
      rewindDrop?: boolean;
    },
  ) {
    const message = (text ?? input).trim();
    if ((!message && pendingImages.length === 0) || stream.isStreaming) return;
    // Send-time snapshot — cleared only after a successful IPC call.
    const images = pendingImages.map((p) => p.dataUrl);
    if (!opts?.skipUserAppend) {
      if (!opts?.autoContinue && message) pushComposerHistory(message);
      historyIdx = -1;
      draftBeforeNav = "";
      input = "";
      await tick();
      syncComposerHeight();
    }
    stickToBottom = true;
    void scrollToBottom();
    lastUserMessage.set(message);
    if (!opts?.fromScene) lastSendSource.set("chat");
    resetTurnUsage();
    clearConfirmSessionAllow();
    const sid = ensureSession();
    // Any user-initiated send breaks the iteration-cap auto-continue chain.
    if (!opts?.autoContinue) resetAutoContinue(sid);
    // Edit-resend: drop the original turn (and everything after) before
    // history is computed, and tell Rust to rewind its cached session.
    const rewind = editRewind;
    editRewind = null;
    let sendOpts = opts;
    if (rewind) {
      removeItemsFrom(rewind.itemId, sid);
      sendOpts = { ...opts, rewindToUser: rewind.original, rewindDrop: true };
    }
    deferSessionPersist.set(true);
    clearSedimentCandidate(sid);
    const history = historyForSend(sid);
    if (!opts?.skipUserAppend) {
      appendItem(newMessage("user", message, false, { images }), sid);
    }
    // Scene/explicit title wins; otherwise name「新会话」from first user text
    // (summarizeSessionTitle maps known scene prompts → short names).
    const explicit = opts?.sessionTitle?.trim() || "";
    const preferred = explicit || summarizeSessionTitle(message);
    const live = get(sessionsData).sessions[sid];
    if (live && preferred && preferred !== "新会话") {
      if (explicit || live.title === "新会话" || !live.title) {
        renameSession(sid, preferred);
      }
    }
    stream.beginTurn(sid);
    try {
      await ipc.sendMessage(message, history, $planMode, {
        profileId: sessionProfileId || null,
        model: sessionModel || null,
        chatSessionId: sid,
        resume: !!sendOpts?.skipUserAppend,
        rewindToUser: sendOpts?.rewindToUser ?? null,
        rewindDrop: sendOpts?.rewindDrop ?? false,
        recording: $skillRecording ? true : undefined,
        images,
      });
      pendingImages = [];
    } catch (e) {
      stream.softUnlockIfStale(sid);
      flushSessionPersist();
      appendItem(newMessage("assistant", String(e), true), sid);
    }
  }

  async function retryError(errorId: string) {
    const session = $currentSession;
    if (!session || stream.isStreaming) return;
    const idx = session.messages.findIndex((m) => m.id === errorId);
    if (idx < 0) return;
    let userContent = "";
    for (let i = idx - 1; i >= 0; i--) {
      const m = session.messages[i];
      if (m.type === "message" && m.role === "user") {
        userContent = m.content;
        break;
      }
    }
    if (!userContent) userContent = $lastUserMessage;
    if (!userContent) return;
    removeItem(errorId, session.id);
    await send(userContent, { skipUserAppend: true });
  }

  /** Regenerate an answer: rewind UI + Rust session back to the preceding
   *  user turn, then re-run it (the discarded turn never reaches the model). */
  async function regenerateFrom(itemId: string) {
    const session = $currentSession;
    if (!session || stream.isStreaming) return;
    const idx = session.messages.findIndex((m) => m.id === itemId);
    if (idx < 0) return;
    let userItem: { id: string; content: string } | null = null;
    for (let i = idx - 1; i >= 0; i--) {
      const m = session.messages[i];
      if (m.type === "message" && m.role === "user") {
        userItem = { id: m.id, content: m.content };
        break;
      }
    }
    if (!userItem) return;
    removeItemsAfter(userItem.id, session.id);
    await send(userItem.content, {
      skipUserAppend: true,
      rewindToUser: userItem.content,
    });
  }

  /** Edit a user message into the composer; the next send replaces the turn
   *  and everything after it. */
  function startEditMessage(itemId: string, content: string, images?: string[]) {
    if (stream.isStreaming) return;
    historyIdx = -1;
    editRewind = { itemId, original: content };
    input = content;
    pendingImages = (images ?? [])
      .filter((u) => u.startsWith("data:image/"))
      .map((dataUrl) => ({ dataUrl, name: "粘贴图片", size: dataUrl.length }));
    void tick().then(() => {
      syncComposerHeight();
      inputEl?.focus();
    });
  }

  function cancelEditRewind() {
    editRewind = null;
    input = "";
    void tick().then(() => syncComposerHeight());
  }

  function escapeSkillArg(value: string): string {
    return value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  }

  function toggleRecording() {
    if ($skillRecording) {
      // Stop recording → show inline save dialog
      skillRecording.set(false);
      skillRecordStartTime.set(null);
      skillSaveDraft = {
        open: true,
        name: "",
        title: "",
        desc: "",
      };
      // Focus name field after render
      void tick().then(() => {
        const el = document.getElementById("skill-save-name");
        el?.focus();
      });
    } else {
      // Start recording
      skillRecording.set(true);
      skillRecordStartTime.set(Date.now());
      skillRecordSteps.set(0);
    }
  }

  function confirmSkillSave() {
    const name = skillSaveDraft.name.trim();
    const title = skillSaveDraft.title.trim();
    const desc = skillSaveDraft.desc.trim();
    if (!name || !title || !desc) return;
    const cmd = `save_skill("${escapeSkillArg(name)}", "${escapeSkillArg(title)}", "${escapeSkillArg(desc)}")`;
    void send(cmd);
    skillSaveDraft = { open: false, name: "", title: "", desc: "" };
  }

  function cancelSkillSave() {
    skillSaveDraft = { open: false, name: "", title: "", desc: "" };
  }

  async function stop() {
    const sidAtStop = stream.streamSessionId;
    try {
      await ipc.cancelGeneration();
    } catch {
      /* ignore */
    }
    // Soft unlock if cancelled event is delayed (e.g. plan wait).
    setTimeout(() => {
      if (stream.isStreaming && stream.streamSessionId === sidAtStop) {
        markUndoneToolsStopped(sidAtStop);
        markActivePlanInterrupted(sidAtStop);
        stream.endTurn();
        flushSessionPersist();
      }
    }, 1200);
  }

  function cancelIfLeavingStream(nextId?: string | null) {
    if (stream.isStreaming && stream.streamSessionId && stream.streamSessionId !== nextId) {
      void ipc.cancelGeneration().catch(() => {});
    }
  }

  async function onSwitchSession(id: string) {
    cancelIfLeavingStream(id);
    historyIdx = -1;
    const prevDir = get(workDir);
    switchSession(id);
    const bound = get(sessionsData).sessions[id]?.workDirPath?.trim();
    if (bound && bound !== prevDir) {
      try {
        // Restore tools cwd only — do not rewrite this session's workspace binding.
        await applyWorkDir(bound, { bindSession: false });
      } catch {
        /* keep previous dir; sidebar path bar shows live workDir */
      }
    }
  }

  function onNewSession() {
    cancelIfLeavingStream(null);
    createSession();
  }

  function switchModel(profileId: string, m: string) {
    modelMenuOpen = false;
    setSessionLlm(profileId, m);
  }

  /** Shared image intake (paste + file picker): size/limit checks + preview. */
  function addImageFile(file: File) {
    if (file.size > MAX_IMAGE_BYTES) {
      pushToast("图片超过 6MB，未添加", "error");
      return;
    }
    if (pendingImages.length >= MAX_IMAGES_PER_MSG) return;
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string" && pendingImages.length < MAX_IMAGES_PER_MSG) {
        pendingImages.push({
          dataUrl: reader.result,
          name: file.name || "粘贴图片",
          size: file.size,
        });
      }
    };
    reader.readAsDataURL(file);
  }

  /** Paste entry: intercept images when the session model supports vision. */
  function handlePaste(e: ClipboardEvent) {
    if (visionEnabled) {
      const items = e.clipboardData?.items;
      if (items) {
        for (const it of items) {
          if (it.kind === "file" && it.type.startsWith("image/")) {
            const file = it.getAsFile();
            if (file) {
              e.preventDefault();
              addImageFile(file);
            }
          }
        }
      }
    }
    void tick().then(() => syncComposerHeight());
  }

  /** File picker intake (visible image entry). */
  function onImageFiles(e: Event) {
    const input = e.target as HTMLInputElement;
    const files = input.files ? Array.from(input.files) : [];
    for (const f of files) {
      if (f.type.startsWith("image/")) addImageFile(f);
    }
    input.value = ""; // same file can be picked again
  }

  /** Window-wide image drag & drop — reuses the shared image intake. */
  let dragDepth = $state(0);
  $effect(() => {
    const onDragOver = (e: DragEvent) => {
      if (!stream.isStreaming && e.dataTransfer?.types.includes("Files")) {
        e.preventDefault(); // stop the browser from opening the file
        if (e.type === "dragenter") dragDepth += 1;
      }
    };
    const onDragLeave = () => {
      dragDepth = Math.max(0, dragDepth - 1);
    };
    const onDrop = (e: DragEvent) => {
      dragDepth = 0;
      const files = e.dataTransfer?.files ? Array.from(e.dataTransfer.files) : [];
      const images = files.filter((f) => f.type.startsWith("image/"));
      if (images.length === 0) return;
      e.preventDefault();
      if (visionEnabled) {
        for (const f of images) addImageFile(f);
      } else {
        visionGuidanceOpen = true; // same guidance as the attach button
      }
    };
    window.addEventListener("dragenter", onDragOver);
    window.addEventListener("dragover", onDragOver);
    window.addEventListener("dragleave", onDragLeave);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragenter", onDragOver);
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("dragleave", onDragLeave);
      window.removeEventListener("drop", onDrop);
    };
  });

  function closeVisionGuidance() {
    visionGuidanceOpen = false;
  }

  function onKeydown(e: KeyboardEvent) {
    // IME composing: do not send on Enter
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
      return;
    }
    if (e.key === "ArrowUp") {
      recallHistory(1, e);
      return;
    }
    if (e.key === "ArrowDown") {
      recallHistory(-1, e);
      return;
    }
  }

  /** ↑ recalls sent messages when the composer is empty (shell-style);
   *  ↓ walks back towards the stashed draft. */
  function recallHistory(dir: 1 | -1, e: KeyboardEvent) {
    if (editRewind) return;
    const navigating = historyIdx >= 0;
    if (dir === 1) {
      if (!navigating && input.trim() !== "") return;
      const hist = loadComposerHistory();
      if (hist.length === 0) return;
      e.preventDefault();
      if (historyIdx === -1) draftBeforeNav = input;
      if (historyIdx < hist.length - 1) {
        historyIdx += 1;
        input = hist[historyIdx];
        void tick().then(() => syncComposerHeight());
      }
      return;
    }
    if (!navigating) return;
    e.preventDefault();
    historyIdx -= 1;
    input = historyIdx === -1 ? draftBeforeNav : loadComposerHistory()[historyIdx];
    void tick().then(() => syncComposerHeight());
  }

  /** Reset height to 0 before measuring so shrink works after delete. */
  function syncComposerHeight(el: HTMLTextAreaElement | undefined = inputEl) {
    if (!el) return;
    el.style.overflowY = "hidden";
    el.style.height = "0px";
    const next = Math.min(Math.max(el.scrollHeight, 0), COMPOSER_MAX_H);
    el.style.height = `${next}px`;
    el.style.overflowY = el.scrollHeight > COMPOSER_MAX_H ? "auto" : "hidden";
  }

  $effect(() => {
    input;
    inputEl;
    syncComposerHeight();
  });

  $effect(() => {
    const req = $composerFillRequest;
    if (!req?.text) return;
    input = req.text;
    void tick().then(() => {
      syncComposerHeight();
      inputEl?.focus();
      const len = input.length;
      try {
        inputEl?.setSelectionRange(len, len);
      } catch {
        /* ignore */
      }
    });
  });

  function onDocPointerDown(e: PointerEvent) {
    const t = e.target as HTMLElement | null;
    if (!t?.closest?.("[data-model-menu]")) {
      modelMenuOpen = false;
    }
  }

  $effect(() => {
    if (!modelMenuOpen) return;
    document.addEventListener("pointerdown", onDocPointerDown, true);
    return () => document.removeEventListener("pointerdown", onDocPointerDown, true);
  });

  $effect(() => {
    // One-shot after first-run: ask for a real project folder (skippable).
    try {
      if (sessionStorage.getItem(WORKDIR_NUDGE_KEY) !== "1") return;
      sessionStorage.removeItem(WORKDIR_NUDGE_KEY);
      workDirDialogOpen.set(true);
    } catch {
      /* ignore */
    }
  });
</script>

<div
  class="flex flex-1 min-h-0 h-full w-full bg-[var(--color-background)]"
  data-testid="chat-view"
  style="min-height: 100%;"
>
  <aside
    class="flex flex-col border-r border-[var(--color-border-strong)] bg-[var(--color-rail)] transition-[width,opacity] duration-150
      {$sidebarCollapsed ? 'w-0 opacity-0 overflow-hidden' : 'w-[var(--sidebar-w)] opacity-100'}"
    data-testid="chat-sidebar"
  >
    <div class="side-nav">
      <div class="side-seg" role="tablist" aria-label="侧栏分区">
        <button
          type="button"
          role="tab"
          class="side-seg-btn"
          aria-selected={$sidebarTab === "sessions"}
          onclick={() => sidebarTab.set("sessions")}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
          </svg>
          <span>会话</span>
        </button>
        <button
          type="button"
          role="tab"
          class="side-seg-btn"
          data-testid="library-tab"
          aria-selected={$sidebarTab === "library"}
          onclick={() => sidebarTab.set("library")}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
            <path d="M4 6h16M4 12h10M4 18h14" />
            <path d="M18 10l3 2-3 2" />
          </svg>
          <span>场景</span>
        </button>
      </div>
      <button
        type="button"
        class="icon-btn shrink-0"
        aria-label="收起侧栏"
        title="收起侧栏"
        onclick={toggleSidebar}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <path d="M9 4v16M14 9l-3 3 3 3" />
        </svg>
      </button>
    </div>

    {#if $sidebarTab === "library"}
      <LibraryPanel />
    {:else}
      <WorkspacePanel
        onSwitchSession={onSwitchSession}
        onNewSession={onNewSession}
        onCopyTitle={copySessionTitle}
      />
    {/if}
  </aside>

  <div class="flex flex-col flex-1 min-w-0 min-h-0 bg-[var(--color-chat-pane)]">
    <header
      class="h-11 flex items-center justify-between gap-2 px-3 border-b border-[var(--color-border)] shrink-0 bg-[var(--color-surface)]"
      data-testid="chat-topbar"
    >
      <div class="topbar-group">
        {#if $sidebarCollapsed}
          <button type="button" class="icon-btn" aria-label="展开侧栏" onclick={toggleSidebar}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
              <rect x="3" y="4" width="18" height="16" rx="2" />
              <path d="M9 4v16M11 9l3 3-3 3" />
            </svg>
          </button>
        {/if}

        <div class="relative" data-model-menu>
          <button
            type="button"
            class="chip font-mono model-chip"
            aria-haspopup="listbox"
            aria-expanded={modelMenuOpen}
            data-testid="model-menu-trigger"
            onclick={() => {
              modelMenuOpen = !modelMenuOpen;
              modelQuery = "";
            }}
          >
            {modelLabel}
          </button>
          {#if modelMenuOpen}
            <div
              class="model-menu"
              role="listbox"
              data-testid="model-menu"
            >
              {#if modelOptions.length > 6}
                <div class="model-menu-search">
                  <input
                    type="text"
                    placeholder="搜索模型…"
                    aria-label="搜索模型"
                    data-testid="model-menu-search"
                    bind:value={modelQuery}
                  />
                </div>
              {/if}
              <div class="model-menu-scroll">
                {#each filteredModelOptions as item, i (`${item.profileId}-${item.model}-${i}`)}
                  {@const selected =
                    item.profileId === sessionProfileId && item.model === sessionModel}
                  <button
                    type="button"
                    class="model-menu-item"
                    class:is-selected={selected}
                    role="option"
                    aria-selected={selected}
                    data-testid="model-menu-item"
                    onclick={() => switchModel(item.profileId, item.model)}
                  >
                    <span class="model-menu-label truncate">{item.label}</span>
                    <span class="model-menu-model truncate">{item.model}</span>
                  </button>
                {:else}
                  <p class="model-menu-empty">无匹配模型</p>
                {/each}
              </div>
              <button
                type="button"
                class="model-menu-footer"
                data-testid="model-menu-settings"
                onclick={() => {
                  modelMenuOpen = false;
                  nav.showSettings({ fromChat: true, tab: "model" });
                }}>在设置中自定义</button
              >
            </div>
          {/if}
        </div>

        <button
          type="button"
          class="icon-btn"
          class:is-active={$planMode !== "off"}
          data-testid="plan-mode-toggle"
          aria-label={PLAN_MODE_LABELS[$planMode]}
          aria-pressed={$planMode !== "off"}
          title={PLAN_MODE_LABELS[$planMode]}
          onclick={() =>
            setPlanMode($planMode === "auto" ? "on" : $planMode === "on" ? "off" : "auto")}
        >
          {#if $planMode === "on"}
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
              <rect x="3.5" y="4.5" width="17" height="16" rx="2" />
              <path d="M3.5 9h17M8 3v3M16 3v3M9 14l2 2 4-4" />
            </svg>
          {:else if $planMode === "auto"}
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
              <path d="M13 2L4 14h6l-1 8 9-12h-6l1-8z" />
            </svg>
          {:else}
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
              <rect x="3.5" y="4.5" width="17" height="16" rx="2" opacity="0.4" />
              <path d="M3.5 9h17M8 3v3M16 3v3" opacity="0.4" />
            </svg>
          {/if}
        </button>
      </div>

      <div class="topbar-group topbar-system">
        <div
          class="usage-meter"
          data-testid="usage-meter"
          title={layerData ? layerData.title : "Context 占用与本轮 tokens（估算）"}
        >
          {#if layerData}
            <div class="usage-meter-bar usage-meter-layered" aria-hidden="true" data-testid="usage-layers">
              <i style="width: {layerData.hot}%"></i>
              <i class="usage-meter-seg-warm" style="width: {layerData.warm}%"></i>
              <i class="usage-meter-seg-cold" style="width: {layerData.cold}%"></i>
            </div>
          {:else}
            <div class="usage-meter-bar" aria-hidden="true">
              <i style="width: {$contextPct}%"></i>
            </div>
          {/if}
          <span class="usage-meter-text" data-testid="usage-context">
            Ctx {formatTokenCount($usage.contextTokens)}/{formatTokenCount($usage.contextLimit)}
          </span>
          {#if $usage.inputTokens + $usage.outputTokens > 0}
            <span class="usage-meter-sep" aria-hidden="true">·</span>
            <span class="usage-meter-text" data-testid="usage-turn">
              {formatTokenCount($usage.inputTokens + $usage.outputTokens)}
            </span>
          {/if}
          {#if $usage.iterations > 0}
            <span class="usage-meter-sep" aria-hidden="true">·</span>
            <span class="usage-meter-text" data-testid="usage-iters">{$usage.iterations} 次</span>
          {/if}
          {#if $usage.compacted}
            <span class="usage-meter-badge">已压缩</span>
          {/if}
        </div>

        <button
          type="button"
          class="icon-btn"
          class:is-active={compact.mode}
          data-testid="topbar-compact-toggle"
          aria-label={compact.mode ? "退出紧凑模式" : "紧凑模式"}
          aria-pressed={compact.mode}
          title={compact.mode ? "退出紧凑模式" : "紧凑模式"}
          onclick={() => void compact.toggle()}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
            <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" />
          </svg>
        </button>
              <button
        type="button"
        class="icon-btn"
        data-testid="toggle-theme"
        data-theme-pref={$themePreference}
        aria-label={themeAriaLabel($themePreference)}
        title={themeAriaLabel($themePreference)}
        onclick={toggleTheme}
      >
        {#if $themePreference === "system"}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
            <rect x="3" y="4" width="18" height="14" rx="2" />
            <path d="M8 21h8M12 18v3" />
          </svg>
        {:else if $themePreference === "light"}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
            <circle cx="12" cy="12" r="4" /><path
              d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"
            />
          </svg>
        {:else}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
            <path d="M21 14.5A8.5 8.5 0 1110.5 3a7 7 0 0010.5 11.5z" />
          </svg>
        {/if}
      </button>
      <button
        type="button"
        class="icon-btn"
        data-testid="open-settings"
        aria-label={!$config?.api_token_set ? "打开设置（未连接账号）" : "打开设置"}
        onclick={() =>
          nav.showSettings({
            fromChat: true,
            tab: !$config?.api_token_set ? "account" : undefined,
          })
        }
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
          <circle cx="12" cy="12" r="3" />
          <path
            d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
          />
        </svg>
          {#if !$config?.api_token_set}
            <span class="account-ready-dot" data-testid="settings-account-dot" aria-hidden="true"></span>
          {/if}
        </button>
      </div>
    </header>

    <div class="chat-scroll-wrap">
      <ChatFindBar containerEl={messagesEl} sessionKey={$currentSessionId} />
      <div
        bind:this={messagesEl}
        class="chat-log flex-1 overflow-y-auto px-5 py-4 flex flex-col"
        role="log"
        aria-live="polite"
        onscroll={onScroll}
      >
      {#if items.length === 0}
        <div class="welcome-panel">
          <div class="welcome-mark" aria-hidden="true">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M4 7h12M4 12h8M4 17h5" />
              <path d="M16 14l4 2.5L16 19" />
            </svg>
          </div>
          <h2 class="welcome-title">Stitch</h2>
          <p class="welcome-sub">选一个场景开始，或直接输入任务</p>
          {#if prevCheckpoint}
            <div class="welcome-checkpoint mb-5" data-testid="welcome-checkpoint">
              <button
                type="button"
                class="welcome-checkpoint-btn"
                data-testid="load-prev-checkpoint"
                disabled={loadingPrevCheckpoint}
                onclick={() => {
                  const sid = $currentSessionId;
                  if (!sid || loadingPrevCheckpoint) return;
                  loadingPrevCheckpoint = true;
                  void applyLatestWorkspaceCheckpoint(sid).finally(() => {
                    loadingPrevCheckpoint = false;
                    prevCheckpoint = null;
                  });
                }}
              >
                {loadingPrevCheckpoint ? "载入中…" : "载入上一检查点"}
              </button>
              {#if prevCheckpoint.summary_preview}
                <p class="welcome-checkpoint-preview">{prevCheckpoint.summary_preview}</p>
              {/if}
            </div>
          {/if}
          <div class="welcome-hints" data-testid="welcome-scenes">
            {#each RECOMMENDED_SCENES as scene (scene.id)}
              <button
                type="button"
                class="welcome-hint"
                data-testid={`scene-${scene.id}`}
                onclick={() => {
                  lastSendSource.set("scene");
                  void ipc.trackUsage("stitch_scene_run", {
                    scene: scene.id,
                    from: "welcome",
                  });
                  void send(scene.prompt, { fromScene: true, sessionTitle: scene.title });
                }}
              >
                <span class="welcome-hint-title">{scene.title}</span>
                <span class="welcome-hint-prompt">{scene.summary}</span>
              </button>
            {/each}
          </div>
        </div>
      {:else}
        {#if virtualizationActive && virtualWindow.topSpacer > 0}
          <div style={`height: ${virtualWindow.topSpacer}px`} aria-hidden="true"></div>
        {/if}
        {#each visibleBlocks as block (blockKey(block))}
          {#if block.kind === "tool_group"}
            <div class="chat-item is-tool tool-stack-start tool-stack-end" data-block-key={blockKey(block)}>
              <ToolGroup tools={block.items} />
            </div>
          {:else}
            {@const item = block.item}
            {@const i = block.index}
            {@const prev = i > 0 ? items[i - 1] : null}
            {@const next = i < items.length - 1 ? items[i + 1] : null}
            {@const toolStacked = item.type === "tool" && !!prev && prev.type === "tool"}
            {@const toolContinues = item.type === "tool" && !!next && next.type === "tool"}
            {@const turnStart = item.type === "message" && item.role === "user" && i > 0}
            <div
              class="chat-item"
              class:is-turn-start={turnStart}
              class:is-tool={item.type === "tool"}
              class:tool-stack-start={item.type === "tool" && !toolStacked}
              class:tool-stack-mid={toolStacked && toolContinues}
              class:tool-stack-end={toolStacked && !toolContinues}
              data-block-key={blockKey(block)}
            >
              {#if item.type === "message"}
                <MessageBubble
                  role={item.role}
                  content={
                    item.id === streamingId && stream.isStreaming
                      ? stream.streamingContent || item.content
                      : item.content
                  }
                  error={!!item.error}
                  images={item.images}
                  imagesStripped={item.imagesStripped}
                  streaming={item.id === streamingId && stream.isStreaming}
                  expanded={!!expandedBlocks[blockKey(block)]}
                  onToggleExpanded={(v) => {
                    const key = blockKey(block);
                    expandedBlocks = { ...expandedBlocks, [key]: v };
                    // 视觉锚定：展开/收起改变块高，虚拟窗口按新高度重排（测量在
                    // 下一帧写入）。块顶部在展开/收起时不动（增长在块内部），但
                    // 贴窗口边缘的块可能在重排后超界被虚拟化裁出。等测量完成后
                    // 检查：块若被裁出窗口，滚到其估算位置（展开块可见是不变量）；
                    // 仍在窗口内则原位自然保持。主动操作=阅读意图，放弃钉底
                    // （stickToBottom 会把视口推到新底部，刚展开的消息头部滚出）。
                    const prevTop = messagesEl?.querySelector(
                      `[data-block-key="${CSS.escape(key)}"]`,
                    )?.getBoundingClientRect().top;
                    stickToBottom = false;
                    requestAnimationFrame(() =>
                      requestAnimationFrame(() => {
                        if (!messagesEl) return;
                        if (messagesEl.querySelector(`[data-block-key="${CSS.escape(key)}"]`)) {
                          return; // 窗口内——原位自然保持
                        }
                        // 被裁出：滚到估算位置并保持点击时的相对视图（prevTop 可负）
                        let acc = 0;
                        for (const b of timeline) {
                          if (blockKey(b) === key) break;
                          acc += estimateBlockHeight(b) + BLOCK_GAP;
                        }
                        messagesEl.scrollTop = Math.max(0, acc + (prevTop ?? 0));
                      }),
                    );
                  }}
                  thinking={
                    item.id === streamingId && stream.isStreaming && !stream.streamingContent
                  }
                  onRetry={item.error ? () => void retryError(item.id) : undefined}
                  onContinue={
                    item.role === "assistant" && item.hitCap && !stream.isStreaming
                      ? () => void send("继续执行")
                      : undefined
                  }
                  onRegenerate={
                    item.role === "assistant" &&
                    !item.error &&
                    !item.stopped &&
                    !stream.isStreaming &&
                    item.id === lastAssistantId
                      ? () => void regenerateFrom(item.id)
                      : undefined
                  }
                  onEdit={
                    item.role === "user" && !stream.isStreaming
                      ? () => startEditMessage(item.id, item.content, item.images)
                      : undefined
                  }
                  onSediment={
                    item.role === "assistant" && !item.error && !item.stopped
                      ? offerSediment
                      : undefined
                  }
                  sedimentReady={
                    item.role === "assistant" &&
                    !item.error &&
                    !item.stopped &&
                    !!$currentSession?.sedimentCandidate
                  }
                />
              {:else if item.type === "tool"}
                <ToolStatus
                  name={item.name}
                  done={item.done}
                  error={item.error}
                  summary={item.summary}
                  detail={item.detail}
                  expanded={!!item.expanded}
                  recorded={!!item.recorded}
                  stacked={toolStacked}
                  elapsedMs={!item.done
                    ? nowMs - (item.startedAt ?? stream.streamStartedAt ?? nowMs)
                    : 0}
                  metrics={item.metrics}
                />
              {:else if item.type === "plan"}
                <PlanCard
                  title={item.title}
                  steps={item.steps}
                  phase={item.phase}
                  planId={item.planId}
                />
              {:else if item.type === "sediment"}
                <SedimentCard
                  id={item.id}
                  title={item.title}
                  content={item.content}
                  status={item.status}
                  errorText={item.errorText}
                  promptId={item.promptId}
                />
              {/if}
            </div>
          {/if}
        {/each}
        {#if virtualizationActive && virtualWindow.bottomSpacer > 0}
          <div style={`height: ${virtualWindow.bottomSpacer}px`} aria-hidden="true"></div>
        {/if}
        {#if $confirmOpen}
          <div class="chat-item is-confirm">
            <ConfirmCard />
          </div>
        {/if}
      {/if}
      </div>
      {#if !stickToBottom && items.length > 0}
        <button
          type="button"
          class="scroll-bottom-pill"
          data-testid="scroll-bottom"
          aria-label="回到底部"
          onclick={() => {
            stickToBottom = true;
            void scrollToBottom();
          }}
        >
          {#if stream.isStreaming}
            <span class="pill-dot" aria-hidden="true"></span>
          {/if}
          <span>回到底部</span>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M6 9l6 6 6-6" />
          </svg>
        </button>
      {/if}
    </div>

    <footer class="shrink-0 border-t border-[var(--color-border)] bg-[var(--color-chat-pane)]">
      {#if activity}
        <div class="stream-rail" data-testid="stream-rail" aria-live="polite">
          <div class="stream-rail-top">
            <span class="stream-rail-dot" aria-hidden="true"></span>
            <span class="stream-rail-label" data-testid="stream-summary">{activity.summary}</span>
            <span class="stream-rail-meta">
              <span class="stream-rail-phase">{activity.phase}</span>
              <span class="stream-rail-format" data-testid="stream-format">{activity.format}</span>
              <span class="stream-rail-time" data-testid="stream-elapsed">{activity.elapsedLabel}</span>
            </span>
          </div>
          <div class="stream-rail-usage" data-testid="stream-usage">
            <span>{activity.iterLabel}</span>
            <span aria-hidden="true">·</span>
            <span data-testid="stream-tokens">{activity.tokensLabel}</span>
            <span aria-hidden="true">·</span>
            <span data-testid="stream-context">{activity.contextLabel}</span>
            {#if $usage.compacted}
              <span class="usage-meter-badge">已压缩</span>
            {/if}
          </div>
          <div class="stream-rail-progress" data-testid="stream-progress">
            <div
              class="stream-rail-bar"
              class:is-indeterminate={activity.indeterminate}
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={activity.progressPct ?? undefined}
            >
              {#if !activity.indeterminate && activity.progressPct != null}
                <i style="width: {activity.progressPct}%"></i>
              {:else}
                <i class="is-indeterminate"></i>
              {/if}
            </div>
            {#if activity.progressLabel}
              <span class="stream-rail-progress-label">{activity.progressLabel}</span>
            {/if}
          </div>
        </div>
      {/if}
      <TerminalPanel />
      {#if $matureSoftGate}
        <div class="mature-soft-gate" data-testid="mature-soft-gate" role="status">
          <span class="mature-soft-gate-text">
            {$matureSoftGate.kind === "need_token" ? "连接账号" : "会员方案"}
          </span>
          <div class="mature-soft-gate-actions">
            {#if $matureSoftGate.kind === "need_token"}
              <button
                type="button"
                class="btn-ghost"
                data-testid="mature-soft-gate-settings"
                onclick={() => {
                  muteSoftGate();
                  nav.showSettings({ fromChat: true, tab: "account" });
                }}
              >
                去设置
              </button>
            {:else}
              <button
                type="button"
                class="btn-ghost"
                data-testid="mature-soft-gate-pricing"
                onclick={() => {
                  const url = $matureSoftGate?.pricingUrl;
                  if (url) void ipc.openExternalUrl(url);
                }}
              >
                查看
              </button>
            {/if}
            <button
              type="button"
              class="mature-soft-gate-x"
              data-testid="mature-soft-gate-dismiss"
              aria-label="关闭"
              onclick={() => muteSoftGate()}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          </div>
        </div>
      {/if}
      {#if editRewind}
        <div class="edit-rewind-bar" data-testid="edit-rewind-bar">
          <span class="edit-rewind-label">编辑消息中，发送将替换原消息及其后内容</span>
          <button
            type="button"
            class="edit-rewind-x"
            aria-label="取消编辑"
            data-testid="edit-rewind-cancel"
            onclick={cancelEditRewind}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      {/if}
      <div class="composer-row">
                {#if skillSaveDraft.open}
          <div class="skill-save-dialog" data-testid="skill-save-dialog">
            <div class="skill-save-fields">
              <input
                id="skill-save-name"
                data-testid="skill-save-name"
                class="skill-save-input"
                type="text"
                placeholder="Skill slug（英文，如 excel-report）"
                bind:value={skillSaveDraft.name}
                onkeydown={(e: KeyboardEvent) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    document.getElementById("skill-save-title")?.focus();
                  }
                  if (e.key === "Escape") cancelSkillSave();
                }}
              />
              <input
                id="skill-save-title"
                data-testid="skill-save-title"
                class="skill-save-input"
                type="text"
                placeholder="标题（中文）"
                bind:value={skillSaveDraft.title}
                onkeydown={(e: KeyboardEvent) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    document.getElementById("skill-save-desc")?.focus();
                  }
                  if (e.key === "Escape") cancelSkillSave();
                }}
              />
              <input
                id="skill-save-desc"
                data-testid="skill-save-desc"
                class="skill-save-input"
                type="text"
                placeholder="一句话描述"
                bind:value={skillSaveDraft.desc}
                onkeydown={(e: KeyboardEvent) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    confirmSkillSave();
                  }
                  if (e.key === "Escape") cancelSkillSave();
                }}
              />
            </div>
            <div class="skill-save-actions">
              <button
                type="button"
                class="skill-save-cancel"
                data-testid="skill-save-cancel"
                onclick={() => cancelSkillSave()}
              >取消</button>
              <button
                type="button"
                class="skill-save-confirm"
                data-testid="skill-save-confirm"
                disabled={!skillSaveDraft.name.trim() || !skillSaveDraft.title.trim() || !skillSaveDraft.desc.trim()}
                onclick={() => confirmSkillSave()}
              >保存 Skill</button>
            </div>
          </div>
        {/if}
        <div class="composer-wrap" class:has-rail={!!activity}>
          <div class="composer">
            <CapabilityRail />
            {#if pendingImages.length > 0}
              <div class="pending-images" data-testid="pending-images">
                {#each pendingImages as img, i (img.dataUrl)}
                  <span class="pending-image">
                    <img src={img.dataUrl} alt="" />
                    <button
                      type="button"
                      class="icon-btn pending-image-remove"
                      data-testid="pending-image-remove-{i}"
                      aria-label="移除图片"
                      title="移除图片"
                      onclick={() => {
                        pendingImages.splice(i, 1);
                      }}
                    >
                      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                        <path d="M18 6L6 18M6 6l12 12" />
                      </svg>
                    </button>
                  </span>
                {/each}
              </div>
            {/if}
            <textarea
              id="chat-input"
              data-testid="chat-input"
              class="composer-input"
              rows="1"
              placeholder={$planMode ? "描述任务，先生成计划…" : "输入消息…"}
              bind:this={inputEl}
              bind:value={input}
              disabled={stream.isStreaming}
              onkeydown={onKeydown}
              oninput={() => syncComposerHeight()}
              onpaste={handlePaste}
            ></textarea>
            <button
              type="button"
              class="composer-image-btn"
              data-testid="image-attach"
              aria-label="添加图片"
              title="添加图片"
              disabled={stream.isStreaming}
              onclick={() =>
                (visionEnabled ? imageFileEl?.click() : (visionGuidanceOpen = true))}
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                <rect x="3" y="3" width="18" height="18" rx="2" />
                <circle cx="8.5" cy="8.5" r="1.5" />
                <path d="M21 15l-5-5L5 21" />
              </svg>
            </button>
            <input
              type="file"
              accept="image/*"
              multiple
              class="image-file-input"
              data-testid="image-file-input"
              bind:this={imageFileEl}
              onchange={onImageFiles}
            />
            <button
              type="button"
              class="composer-send {stream.isStreaming ? 'is-stop' : ''}"
              data-testid="chat-send"
              aria-label={stream.isStreaming ? "停止生成" : "发送"}
              title={stream.isStreaming ? "停止生成" : "发送"}
              disabled={!stream.isStreaming && !input.trim() && pendingImages.length === 0}
              onclick={() => (stream.isStreaming ? stop() : send())}
            >
              {#if stream.isStreaming}
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                  <rect x="6" y="6" width="12" height="12" rx="1.5" />
                </svg>
              {:else}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                  <path d="M12 19V5M5 12l7-7 7 7" />
                </svg>
              {/if}
            </button>
          </div>
        </div>
      </div>
      <p class="composer-hint">
        <span class="composer-hint-keys">
          <kbd>Enter</kbd> 发送 · <kbd>Shift+Enter</kbd> 换行
          {#if $planMode}
            · 计划模式已开
          {/if}
          {#if stream.isStreaming}
            · 生成中，结束后可发送
          {/if}
        </span>
        {#if input.length >= 200}
          <span class="composer-count">{input.length} 字</span>
        {/if}
      </p>
    </footer>
  </div>

  {#if dragDepth > 0 && !stream.isStreaming}
    <div class="image-drop-overlay" data-testid="image-drop-overlay">
      <div class="image-drop-hint">松开以添加图片</div>
    </div>
  {/if}

  {#if visionGuidanceOpen}
    <div
      class="modal-overlay"
      data-testid="image-guidance-dialog"
      role="presentation"
      onclick={(e) => {
        if (e.target === e.currentTarget) closeVisionGuidance();
      }}
      onkeydown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          closeVisionGuidance();
        }
      }}
    >
      <div class="modal-panel image-guidance-panel" role="dialog" aria-modal="true" aria-label="当前模型不支持图片">
        <div class="shortcuts-head">
          <h2>当前模型不支持图片</h2>
          <button type="button" class="icon-btn" aria-label="关闭" onclick={closeVisionGuidance}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
        <p class="image-guidance-body">
          装一个本地视觉模型即可发图（如 Ollama 的 qwen3-vl）。图片只在本机处理，不占云端 token。
        </p>
        <div class="image-guidance-actions">
          <button
            type="button"
            class="btn btn-primary"
            data-testid="image-guidance-open-settings"
            onclick={() => {
              visionGuidanceOpen = false;
              nav.showSettings({ tab: "model", fromChat: true });
            }}
          >打开模型设置</button>
        </div>
      </div>
    </div>
  {/if}
</div>
