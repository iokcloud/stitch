<script lang="ts">
  import {
    config,
    lastUserMessage,
    lastSendSource,
    planMode,
    fillComposer,
    matureSoftGate,
    clearMatureSoftGate,
    isSoftGateMuted,
  } from "../stores/app";
import { stream } from "../stores/stream.svelte";
  import { nav } from "../nav.svelte";
  import {
    appendItem,
    createSession,
    ensureSession,
    newMessage,
    newPlan,
    deferSessionPersist,
    flushSessionPersist,
    historyForSend,
    clearSedimentCandidate,
  } from "../stores/sessions";
  import { resetTurnUsage } from "../stores/usage";
  import type { AgentSummary, SuiteSummary } from "../types";
  import { RECOMMENDED_SCENES } from "../scenes";
  import { MATURE_SCENES, type MatureScene } from "../mature-scenes";
  import { fetchMembership } from "../membership";
  import * as ipc from "../ipc";
  import type { SkillSummary } from "../ipc";
  import { friendlyLibraryError, isAccountAuthError } from "../settings-errors";
  import { LIBRARY_KIND_KEY } from "../types";

  const LIBRARY_KINDS = ["scenes", "suites", "agents", "skills"] as const;
  type LibraryKind = (typeof LIBRARY_KINDS)[number];

  function loadLibraryKind(): LibraryKind {
    try {
      const raw = localStorage.getItem(LIBRARY_KIND_KEY);
      if (raw && (LIBRARY_KINDS as readonly string[]).includes(raw)) {
        return raw as LibraryKind;
      }
    } catch {
      /* ignore */
    }
    return "scenes";
  }

  let kind = $state<LibraryKind>(loadLibraryKind());

  function setKind(next: LibraryKind) {
    kind = next;
    try {
      localStorage.setItem(LIBRARY_KIND_KEY, next);
    } catch {
      /* ignore */
    }
  }
  let suites = $state<SuiteSummary[]>([]);
  let agents = $state<AgentSummary[]>([]);
  let skills = $state<SkillSummary[]>([]);
  let loading = $state(false);
  let error = $state("");
  /** Run-time banner (keep list visible). */
  let runError = $state("");
  let slugInput = $state("");

  const tokenReady = $derived(!!$config?.api_token_set);

  function openAccountSettings() {
    nav.showSettings({ fromChat: true, tab: "account" });
  }

  async function refresh() {
    if (kind === "scenes") {
      loading = false;
      error = "";
      return;
    }
    if (kind === "skills") {
      loading = true;
      error = "";
      try {
        skills = await ipc.listSkills();
      } catch (e) {
        error = friendlyLibraryError(e);
        skills = [];
      } finally {
        loading = false;
      }
      return;
    }
    if (!tokenReady) {
      error = "";
      return;
    }
    loading = true;
    error = "";
    try {
      if (kind === "suites") {
        suites = await ipc.listSuites();
      } else {
        agents = await ipc.listAgents();
      }
    } catch (e) {
      error = friendlyLibraryError(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    kind;
    tokenReady;
    runError = "";
    void refresh();
  });

  function useSkill(phrase: string) {
    if (stream.isStreaming || !phrase.trim()) return;
    void ipc.trackUsage("stitch_skill_use", { from: "library" });
    fillComposer(phrase);
  }

  async function runItem(
    id: string,
    title: string,
    asAgent: boolean,
    stepCount = 1,
  ) {
    if (stream.isStreaming) return;
    if (!tokenReady) {
      runError = "连接 PromptStdio 账号后即可运行";
      return;
    }
    runError = "";
    const sid = ensureSession();
    deferSessionPersist.set(true);
    clearSedimentCandidate(sid);
    appendItem(
      newMessage("user", asAgent ? `执行智能体：${title}` : `执行套件：${title}`),
      sid,
    );
    const n = Math.max(1, stepCount | 0);
    const steps = asAgent
      ? [{ description: "准备执行…", status: "pending" as const }]
      : Array.from({ length: n }, (_, i) => ({
          description: `步骤 ${i + 1}`,
          status: "pending" as const,
        }));
    appendItem(newPlan({ title, steps }, undefined, { phase: "approved" }), sid);
    stream.beginTurn(sid);
    try {
      if (asAgent) await ipc.runAgent(id);
      else await ipc.runSuite(id);
    } catch (e) {
      stream.endTurn();
      flushSessionPersist();
      const msg = friendlyLibraryError(e);
      runError = msg;
      // Auth: keep failure in the library panel (mainstream); never dump API JSON into chat.
      if (isAccountAuthError(e)) return;
      appendItem(newMessage("assistant", msg, true), sid);
    }
  }

  async function runSlug() {
    const id = slugInput.trim();
    if (!id || stream.isStreaming) return;
    await runItem(id, id, kind === "agents");
  }

  async function runScene(sceneId: string, prompt: string) {
    if (stream.isStreaming || !prompt.trim()) return;
    void ipc.trackUsage("stitch_scene_run", { scene: sceneId, from: "library" });
    lastSendSource.set("scene");
    const sid = ensureSession();
    const history = historyForSend(sid);
    lastUserMessage.set(prompt);
    deferSessionPersist.set(true);
    clearSedimentCandidate(sid);
    appendItem(newMessage("user", prompt), sid);
    stream.beginTurn(sid);
    resetTurnUsage();
    try {
      await ipc.sendMessage(prompt, history, $planMode, {
        chatSessionId: sid,
      });
    } catch (e) {
      stream.endTurn();
      flushSessionPersist();
      appendItem(newMessage("assistant", String(e), true), sid);
    }
  }

  /** Mature scenes: fill composer only (user reviews / edits goal, then sends). G1 soft tip for paid_pool. */
  async function fillMatureScene(scene: MatureScene) {
    if (stream.isStreaming || !scene.prompt.trim()) return;
    // Keep scene_run for product analytics later; no gate_* tracking while埋点冻结.
    void ipc.trackUsage("stitch_scene_run", { scene: scene.id, from: "library_mature" });
    fillComposer(scene.prompt);

    if (scene.tier !== "paid_pool") {
      clearMatureSoftGate();
      return;
    }

    const m = await fetchMembership();
    if (m.is_member || isSoftGateMuted()) {
      clearMatureSoftGate();
      return;
    }
    const tipKind = m.token_set ? "need_member" : "need_token";
    matureSoftGate.set({
      sceneId: scene.id,
      kind: tipKind,
      pricingUrl: m.pricing_url || "https://www.promptstdio.com/pricing",
    });
  }
</script>

<div class="flex flex-col h-full min-h-0" data-testid="library-panel">
  <div class="side-nav" style="border-top: 0; padding-top: 0">
    <div class="side-seg side-seg-library" role="tablist" aria-label="场景类型">
      <button
        type="button"
        role="tab"
        class="side-seg-btn"
        data-testid="library-tab-scenes"
        aria-selected={kind === "scenes"}
        title="精选"
        onclick={() => setKind("scenes")}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <rect x="3" y="3" width="7" height="7" rx="1" />
          <rect x="14" y="3" width="7" height="7" rx="1" />
          <rect x="3" y="14" width="7" height="7" rx="1" />
          <rect x="14" y="14" width="7" height="7" rx="1" />
        </svg>
        <span>精选</span>
      </button>
      <button
        type="button"
        role="tab"
        class="side-seg-btn"
        data-testid="library-tab-suites"
        aria-selected={kind === "suites"}
        title="套件"
        onclick={() => setKind("suites")}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <path d="M4 6h16v4H4zM4 14h10v4H4z" />
        </svg>
        <span>套件</span>
      </button>
      <button
        type="button"
        role="tab"
        class="side-seg-btn"
        data-testid="library-tab-agents"
        aria-selected={kind === "agents"}
        title="智能体"
        onclick={() => setKind("agents")}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <circle cx="12" cy="8" r="3" />
          <path d="M5 19a7 7 0 0 1 14 0" />
        </svg>
        <span>智能体</span>
      </button>
      <button
        type="button"
        role="tab"
        class="side-seg-btn"
        data-testid="library-tab-skills"
        aria-selected={kind === "skills"}
        title="Skill"
        onclick={() => setKind("skills")}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <path d="M12 3l2.2 4.5 5 .7-3.6 3.5.9 5.1L12 14.8 7.5 16.8l.9-5.1L4.8 8.2l5-.7z" />
        </svg>
        <span>Skill</span>
      </button>
    </div>
  </div>

  {#if (kind === "suites" || kind === "agents") && tokenReady}
    <div class="px-2 py-2 border-b border-[var(--color-border)] flex gap-1">
      <input
        class="field text-[12px]"
        type="text"
        placeholder={kind === "agents" ? "智能体 ID / 名称" : "套件 ID"}
        bind:value={slugInput}
        disabled={stream.isStreaming}
        onkeydown={(e) => e.key === "Enter" && void runSlug()}
      />
      <button type="button" class="btn-ghost shrink-0 px-2" disabled={stream.isStreaming} onclick={runSlug}
        >运行</button
      >
    </div>
  {/if}

  {#if runError && (kind === "suites" || kind === "agents")}
    <div
      class="px-3 py-2 border-b border-[var(--color-border)] text-[12px] leading-relaxed"
      data-testid="library-run-error"
    >
      <p class="mb-2 text-[var(--color-error)]">{runError}</p>
      <div class="flex gap-2 flex-wrap">
        <button type="button" class="btn-ghost" onclick={() => (runError = "")}>关闭</button>
        <button type="button" class="btn-ghost" data-testid="library-run-error-settings" onclick={openAccountSettings}
          >连接账号</button
        >
      </div>
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto">
    {#if kind === "scenes"}
      <div class="px-2 pt-2 pb-1" data-testid="library-mature-scenes">
        <p class="label-caps mb-1.5">官方成熟场景</p>
        <div class="flex flex-col border border-[var(--color-border)] rounded-[var(--radius)] overflow-hidden mb-2">
          {#each MATURE_SCENES as scene (scene.id)}
            <button
              type="button"
              class="text-left px-3 py-2 border-b border-[var(--color-border)] last:border-b-0 hover:bg-[var(--color-rail)] disabled:opacity-45"
              data-testid={`library-mature-${scene.id}`}
              disabled={stream.isStreaming}
              onclick={() => void fillMatureScene(scene)}
            >
              <span class="block text-[12px] font-medium text-[var(--color-foreground)]">{scene.title}</span>
              <span class="block text-[11px] text-[var(--color-muted)] mt-0.5 line-clamp-2">{scene.summary}</span>
            </button>
          {/each}
        </div>
      </div>

      <div class="px-2 pt-1 pb-1" data-testid="library-scenes">
        <p class="label-caps mb-1.5">推荐场景</p>
        <div class="flex flex-col border border-[var(--color-border)] rounded-[var(--radius)] overflow-hidden mb-2">
          {#each RECOMMENDED_SCENES as scene (scene.id)}
            <button
              type="button"
              class="text-left px-3 py-2 border-b border-[var(--color-border)] last:border-b-0 hover:bg-[var(--color-rail)] disabled:opacity-45"
              data-testid={`library-scene-${scene.id}`}
              disabled={stream.isStreaming}
              onclick={() => void runScene(scene.id, scene.prompt)}
            >
              <span class="block text-[12px] font-medium text-[var(--color-foreground)]">{scene.title}</span>
              <span class="block text-[11px] text-[var(--color-muted)] mt-0.5 line-clamp-2">{scene.summary}</span>
            </button>
          {/each}
        </div>
      </div>
    {:else if kind === "skills"}
      {#if loading}
        <p class="p-3 text-[12px] text-[var(--color-muted)]">加载中…</p>
      {:else if error}
        <div class="p-3 text-[12px]" data-testid="library-error">
          <p class="mb-2 text-[var(--color-error)]">{error}</p>
          <button type="button" class="btn-ghost" onclick={refresh}>重试</button>
        </div>
      {:else if skills.length === 0}
        <p class="p-3 text-[12px] text-[var(--color-muted)]">暂无官方 Skill。</p>
      {:else}
        <p class="px-3 pt-2 pb-1 text-[11px] text-[var(--color-muted)]" data-testid="library-skills-hint">
          点选填入安装话术，发送后由 Agent 写入工作区 Skill 目录。
        </p>
        {#each skills as sk (sk.slug)}
          <button
            type="button"
            class="w-full text-left px-3 py-2.5 border-b border-[var(--color-border)] hover:bg-[var(--color-rail)]"
            data-testid={`library-skill-${sk.slug}`}
            disabled={stream.isStreaming}
            onclick={() => useSkill(sk.install_phrase)}
          >
            <span class="flex items-center gap-2">
              <span class="block text-[13px] font-medium text-[var(--color-foreground)]">{sk.title}</span>
              {#if sk.price_display}
                <span class="text-[10px] text-[var(--color-muted)]">{sk.price_display}</span>
              {/if}
            </span>
            <span class="block text-[11px] text-[var(--color-muted)] mt-0.5 line-clamp-2">{sk.description}</span>
          </button>
        {/each}
      {/if}
    {:else if !tokenReady}
      <div class="p-3 text-[12px] text-[var(--color-muted)] leading-relaxed" data-testid="library-need-token">
        <p class="mb-2 text-[var(--color-foreground-secondary)] font-medium">
          {kind === "agents" ? "账号智能体" : "账号套件"}
        </p>
        <p class="mb-3">连接 PromptStdio 后，可在此运行网站上的套件与智能体。</p>
        <button
          type="button"
          class="btn-primary w-full"
          data-testid="library-open-account-settings"
          onclick={openAccountSettings}
          >打开账号设置</button
        >
      </div>
    {:else if loading}
      <p class="p-3 text-[12px] text-[var(--color-muted)]">加载中…</p>
    {:else if error}
      <div class="p-3 text-[12px]" data-testid="library-error">
        <p class="mb-2 text-[var(--color-error)]">{error}</p>
        <div class="flex gap-2 flex-wrap">
          <button type="button" class="btn-ghost" onclick={refresh}>重试</button>
          <button
            type="button"
            class="btn-ghost"
            onclick={openAccountSettings}
            >连接账号</button
          >
        </div>
      </div>
    {:else if kind === "suites"}
      {#if suites.length === 0}
        <p class="p-3 text-[12px] text-[var(--color-muted)]">暂无套件。可在 Web 创建后在此运行。</p>
      {:else}
        {#each suites as s (s.id)}
          <button
            type="button"
            class="w-full text-left px-3 py-2.5 border-b border-[var(--color-border)] hover:bg-[var(--color-rail)]"
            disabled={stream.isStreaming}
            onclick={() => {
              createSession();
              void runItem(s.id, s.title, false, s.step_count);
            }}
          >
            <span class="block text-[13px] font-medium text-[var(--color-foreground)]">{s.title}</span>
            {#if s.description}
              <span class="block text-[11px] text-[var(--color-muted)] mt-0.5 line-clamp-2">{s.description}</span>
            {/if}
          </button>
        {/each}
      {/if}
    {:else if agents.length === 0}
      <p class="p-3 text-[12px] text-[var(--color-muted)]">暂无智能体。可在 Web 创建后在此运行。</p>
    {:else}
      {#each agents as a (a.id)}
        <button
          type="button"
          class="w-full text-left px-3 py-2.5 border-b border-[var(--color-border)] hover:bg-[var(--color-rail)]"
          disabled={stream.isStreaming}
          onclick={() => {
            createSession();
            void runItem(a.id, a.name, true);
          }}
        >
          <span class="block text-[13px] font-medium text-[var(--color-foreground)]">{a.name}</span>
          {#if a.task_suite_title}
            <span class="block text-[11px] text-[var(--color-muted)] mt-0.5">{a.task_suite_title}</span>
          {/if}
        </button>
      {/each}
    {/if}
  </div>
</div>
