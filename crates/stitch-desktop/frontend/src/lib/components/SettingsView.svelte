<script lang="ts">
  import { tick } from "svelte";
  import { config } from "../stores/app";
  import { nav } from "../nav.svelte";
  import { fuzzyBest } from "../fuzzy";
  import { settingsState as s } from "./settings/settings-state.svelte";
  import { SETTINGS_CATALOG, type SettingEntry } from "./settings/settings-catalog";
  import ModelPane from "./settings/ModelPane.svelte";
  import AccountPane from "./settings/AccountPane.svelte";
  import McpPane from "./settings/McpPane.svelte";
  import GeneralPane from "./settings/GeneralPane.svelte";

  $effect(() => {
    // Mount-time: adopt the tab requested by whoever opened settings.
    s.tab = nav.settingsFirstRun ? "model" : nav.settingsTab || "model";
  });

  $effect(() => {
    const cfg = $config;
    if (!cfg || s.hydratedFromConfig) return;
    s.syncFromConfig(cfg);
    s.tab = nav.settingsFirstRun ? "model" : nav.settingsTab || "model";
    s.hydratedFromConfig = true;
  });

  // Allow rules load lazily when the 通用 tab opens (rules change as the
  // agent remembers them mid-session — each entry fetches fresh).
  $effect(() => {
    if (s.tab === "system" && !s.allowRulesLoaded) {
      void s.loadAllowRules();
    }
  });

  const canGoChat = $derived(s.canGoChat($config));
  const saveLabel = $derived(nav.settingsFirstRun ? "保存并开始" : "保存");
  const backLabel = $derived(nav.settingsFromChat ? "返回聊天" : "进入聊天");
  const paneTitle = $derived(
    s.tab === "model"
      ? "模型"
      : s.tab === "account"
        ? "账号"
        : s.tab === "mcp"
          ? "MCP"
          : "通用",
  );

  let searchQuery = $state("");
  let searchActive = $state(0);

  const searchMatches = $derived.by(() => {
    const q = searchQuery.trim();
    if (!q) return [] as SettingEntry[];
    const scored: { entry: SettingEntry; score: number }[] = [];
    for (const entry of SETTINGS_CATALOG) {
      const score = fuzzyBest(q, entry.title, [...entry.keywords, entry.tabLabel]);
      if (score !== null) scored.push({ entry, score });
    }
    scored.sort((a, b) => b.score - a.score);
    return scored.map((x) => x.entry);
  });
  const searching = $derived(searchQuery.trim().length > 0);

  $effect(() => {
    searchQuery;
    searchActive = 0;
  });

  async function jumpToSetting(entry: SettingEntry) {
    searchQuery = "";
    s.tab = entry.tab;
    entry.before?.();
    await tick();
    const el = document.querySelector(`[data-testid="${entry.target}"]`);
    if (!el) return;
    const host =
      el.closest(".pref-row, .pref-group, .model-toolbar, .account-connect-banner") ?? el;
    el.scrollIntoView({ block: "center" });
    host.classList.add("settings-flash");
    setTimeout(() => host.classList.remove("settings-flash"), 1700);
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      searchActive = Math.min(searchActive + 1, searchMatches.length - 1);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      searchActive = Math.max(searchActive - 1, 0);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const entry = searchMatches[searchActive];
      if (entry) void jumpToSetting(entry);
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      searchQuery = "";
    }
  }
</script>

<main
  class="settings-shell flex flex-col flex-1 min-h-0 h-full w-full"
  data-testid="settings-view"
>
  <header class="settings-topbar">
    <div class="flex items-center gap-2.5 min-w-0">
      <svg width="20" height="20" viewBox="0 0 32 32" fill="none" aria-hidden="true">
        <rect width="32" height="32" rx="6" fill="#1A2B4C" />
        <path d="M8 11h12M8 16h9M8 21h6" stroke="#FFFFFF" stroke-width="2" stroke-linecap="round" />
        <path
          d="M21 19l5 3-5 3"
          fill="none"
          stroke="var(--color-accent)"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      <span class="text-[13px] font-semibold tracking-tight">设置</span>
    </div>
    <div class="flex-1"></div>
    {#if canGoChat}
      <button type="button" class="btn-ghost" data-testid="settings-go-chat" onclick={s.goChat}>
        {backLabel}
      </button>
    {/if}
  </header>

  <div class="settings-body">
    <nav class="settings-nav" aria-label="设置分类">
      <div class="settings-search">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <circle cx="11" cy="11" r="7" />
          <path d="M21 21l-4.3-4.3" />
        </svg>
        <input
          type="text"
          bind:value={searchQuery}
          placeholder="搜索设置"
          aria-label="搜索设置"
          data-testid="settings-search"
          onkeydown={onSearchKey}
        />
      </div>
      {#if searching}
        {#each searchMatches as entry, i (entry.id)}
          <button
            type="button"
            class="settings-nav-item settings-nav-hit"
            class:is-active={i === searchActive}
            data-testid="settings-search-hit"
            onclick={() => void jumpToSetting(entry)}
            onmouseenter={() => (searchActive = i)}
          >
            <span class="settings-nav-hit-title">{entry.title}</span>
            <span class="settings-nav-hit-tab">{entry.tabLabel}</span>
          </button>
        {:else}
          <p class="settings-nav-empty" data-testid="settings-search-empty">无匹配设置</p>
        {/each}
      {:else}
      <button
        type="button"
        class="settings-nav-item"
        class:settings-nav-item-active={s.tab === "model"}
        data-testid="settings-tab-model"
        onclick={() => (s.tab = "model")}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <rect x="5" y="5" width="14" height="14" rx="2" />
          <path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" />
        </svg>
        模型
      </button>
      <button
        type="button"
        class="settings-nav-item"
        class:settings-nav-item-active={s.tab === "account"}
        data-testid="settings-tab-account"
        onclick={() => (s.tab = "account")}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <circle cx="12" cy="8" r="4" />
          <path d="M4 21c0-4 3.6-6.5 8-6.5s8 2.5 8 6.5" />
        </svg>
        账号
        {#if !s.apiTokenStored}
          <span class="settings-nav-dot" data-testid="settings-account-dot" aria-hidden="true"></span>
        {/if}
      </button>
      <button
        type="button"
        class="settings-nav-item"
        class:settings-nav-item-active={s.tab === "mcp"}
        data-testid="settings-tab-mcp"
        onclick={() => (s.tab = "mcp")}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <rect x="3" y="4" width="18" height="6" rx="2" />
          <rect x="3" y="14" width="18" height="6" rx="2" />
          <path d="M7 7h.01M7 17h.01" />
        </svg>
        MCP
      </button>
      <button
        type="button"
        class="settings-nav-item"
        class:settings-nav-item-active={s.tab === "system"}
        data-testid="settings-tab-system"
        onclick={() => (s.tab = "system")}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <path d="M4 8h10M18 8h2M4 16h2M10 16h10" />
          <circle cx="16" cy="8" r="2" />
          <circle cx="8" cy="16" r="2" />
        </svg>
        通用
      </button>
      {/if}
    </nav>

    <div class="settings-pane">
      <form
        id="settings-form"
        class="settings-pane-inner"
        onsubmit={(e) => {
          e.preventDefault();
          void s.save(true);
        }}
      >
        <h1 class="settings-pane-title">{paneTitle}</h1>

        {#if s.tab === "model"}
          <ModelPane />
        {:else if s.tab === "account"}
          <AccountPane />
        {:else if s.tab === "mcp"}
          <McpPane />
        {:else}
          <GeneralPane />
        {/if}
      </form>
    </div>
  </div>

  <footer class="settings-footer">
    {#if s.status}
      <p
        class="settings-footer-status"
        class:is-error={s.statusError}
        class:is-ok={!!s.status && !s.statusError}
        data-testid="settings-footer-status"
        title={s.status}
        role="status"
      >
        {#if s.showStatusCheck}
          <svg
            class="settings-status-check"
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            data-testid="settings-status-check"
          >
            <polyline points="20 6 9 17 4 12" />
          </svg>
        {/if}
        <span>{s.status}</span>
      </p>
    {:else}
      <span class="settings-footer-status"></span>
    {/if}
    {#if canGoChat}
      <button
        type="button"
        class="btn-ghost"
        data-testid="settings-back-chat"
        onclick={s.goChat}>{backLabel}</button
      >
    {/if}
    {#if s.tab === "model"}
      <button
        type="button"
        class="btn-ghost"
        data-testid="settings-test"
        disabled={s.testing}
        onclick={s.onTest}>测试连接</button
      >
    {:else if s.tab === "account"}
      <button
        type="button"
        class="btn-ghost"
        data-testid="settings-test-promptstdio"
        disabled={s.testingPrompt}
        onclick={s.onTestPromptstdio}>测试连接</button
      >
    {:else if s.tab === "mcp"}
      <button
        type="button"
        class="btn-ghost"
        data-testid="settings-test-mcp-server"
        disabled={s.testingServer || !s.selectedServerId}
        onclick={s.onTestMcpServer}>测试连接</button
      >
    {/if}
    <button
      type="submit"
      form="settings-form"
      class="btn-primary"
      data-testid="settings-save"
      disabled={s.saving}
    >
      {saveLabel}
    </button>
  </footer>
</main>
