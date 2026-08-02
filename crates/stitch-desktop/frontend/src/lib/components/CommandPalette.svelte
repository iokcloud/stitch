<script lang="ts">
  import { tick } from "svelte";
  import { get } from "svelte/store";
  import { paletteOpen, shortcutsOpen, chatFindOpen } from "../stores/palette";
  import { nav } from "../nav.svelte";
  import {
    config,
    workDir,
    applyWorkDir,
    isStreaming,
    streamSessionId,
    sidebarCollapsed,
    toggleSidebar,
  } from "../stores/app";
  import {
    sessionsData,
    switchSession,
    createSession,
    deleteSession,
    setSessionLlm,
    formatRelativeTime,
  } from "../stores/sessions";
  import { themePreference, toggleTheme, themeAriaLabel } from "../stores/theme";
  import { terminalOpen, toggleTerminal } from "../terminal/store";
  import { compact } from "../stores/compact.svelte";
  import { PROVIDER_PRESETS } from "../types";
  import { fuzzyBest } from "../fuzzy";
  import * as ipc from "../ipc";

  type Command = {
    id: string;
    group: "动作" | "会话" | "模型";
    title: string;
    hint?: string;
    keywords?: string[];
    run: () => void | Promise<void>;
  };

  const RECENT_KEY = "stitch-palette-recent";

  let query = $state("");
  let activeIndex = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();
  let recentIds = $state<string[]>([]);

  function loadRecent(): string[] {
    try {
      const raw = localStorage.getItem(RECENT_KEY);
      const arr: unknown = raw ? JSON.parse(raw) : [];
      return Array.isArray(arr) ? arr.filter((x): x is string => typeof x === "string") : [];
    } catch {
      return [];
    }
  }

  function pushRecent(id: string) {
    const list = [id, ...loadRecent().filter((x) => x !== id)].slice(0, 12);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(list));
    } catch {
      /* storage full / private mode — recency is best-effort */
    }
  }

  async function switchToSession(id: string) {
    const live = get(streamSessionId);
    if (get(isStreaming) && live && live !== id) {
      void ipc.cancelGeneration().catch(() => {});
    }
    const prevDir = get(workDir);
    switchSession(id);
    const bound = get(sessionsData).sessions[id]?.workDirPath?.trim();
    if (bound && bound !== prevDir) {
      try {
        await applyWorkDir(bound, { bindSession: false });
      } catch {
        /* keep previous dir */
      }
    }
    if (nav.view !== "chat") nav.showChat("palette");
  }

  async function newSessionFlow() {
    if (get(isStreaming) && get(streamSessionId)) {
      void ipc.cancelGeneration().catch(() => {});
    }
    createSession();
    if (nav.view !== "chat") nav.showChat("palette");
  }

  const allCommands = $derived.by((): Command[] => {
    const list: Command[] = [
      {
        id: "act-new",
        group: "动作",
        title: "新建会话",
        hint: "Ctrl+N",
        keywords: ["new session", "xinjian"],
        run: () => void newSessionFlow(),
      },
      {
        id: "act-settings",
        group: "动作",
        title: "打开设置",
        hint: "Ctrl+,",
        keywords: ["settings", "preferences", "shezhi"],
        run: () => nav.showSettings({ fromChat: true }),
      },
      {
        id: "act-theme",
        group: "动作",
        title: `切换主题（当前：${themeAriaLabel($themePreference)}）`,
        keywords: ["theme", "dark", "light", "zhuti"],
        run: () => toggleTheme(),
      },
      {
        id: "act-sidebar",
        group: "动作",
        title: $sidebarCollapsed ? "展开侧栏" : "收起侧栏",
        hint: "Ctrl+B",
        keywords: ["sidebar", "celan"],
        run: () => toggleSidebar(),
      },
      {
        id: "act-compact",
        group: "动作",
        title: compact.mode ? "退出紧凑模式" : "进入紧凑模式",
        hint: "Ctrl+Shift+C",
        keywords: ["compact", "jinzou", "futiao", "浮条", "紧凑"],
        run: () => void compact.toggleWithMorph(),
      },
      {
        id: "act-terminal",
        group: "动作",
        title: $terminalOpen ? "关闭终端" : "打开终端",
        keywords: ["terminal", "zhongduan"],
        run: () => toggleTerminal(),
      },
      {
        id: "act-find",
        group: "动作",
        title: "在会话中查找",
        hint: "Ctrl+F",
        keywords: ["find", "search", "chazhao"],
        run: () => {
          if (nav.view !== "chat") nav.showChat("palette");
          chatFindOpen.set(true);
        },
      },
      {
        id: "act-focus",
        group: "动作",
        title: "聚焦输入框",
        keywords: ["focus", "input", "shurukuang"],
        run: () => document.getElementById("chat-input")?.focus(),
      },
      {
        id: "act-shortcuts",
        group: "动作",
        title: "快捷键帮助",
        hint: "Ctrl+/",
        keywords: ["shortcuts", "keyboard", "kuaijiejian"],
        run: () => shortcutsOpen.set(true),
      },
      {
        id: "act-set-account",
        group: "动作",
        title: "设置：账号",
        keywords: ["settings account", "account", "token", "zhanghao"],
        run: () => nav.showSettings({ fromChat: true, tab: "account" }),
      },
      {
        id: "act-set-mcp",
        group: "动作",
        title: "设置：MCP",
        keywords: ["settings mcp", "mcp"],
        run: () => nav.showSettings({ fromChat: true, tab: "mcp" }),
      },
      {
        id: "act-set-system",
        group: "动作",
        title: "设置：系统",
        keywords: ["settings system", "system", "xitong"],
        run: () => nav.showSettings({ fromChat: true, tab: "system" }),
      },
      {
        id: "act-del-session",
        group: "动作",
        title: "删除当前会话",
        keywords: ["delete session", "remove", "shanchu"],
        run: () => {
          const cur = get(sessionsData).current;
          if (cur) deleteSession(cur);
        },
      },
    ];
    const sessions = Object.values($sessionsData.sessions)
      .sort((a, b) => Number(b.updatedAt) - Number(a.updatedAt))
      .slice(0, 40);
    for (const s of sessions) {
      list.push({
        id: `ses-${s.id}`,
        group: "会话",
        title: s.title || "新会话",
        hint: formatRelativeTime(s.updatedAt, Date.now()),
        run: () => void switchToSession(s.id),
      });
    }
    for (const p of $config?.llm_profiles ?? []) {
      const preset = PROVIDER_PRESETS[p.provider] || PROVIDER_PRESETS.custom;
      const models = [...preset.models];
      if (p.model && !models.includes(p.model)) models.unshift(p.model);
      for (const m of models) {
        list.push({
          id: `mod-${p.id}-${m}`,
          group: "模型",
          title: `${p.label || p.id} · ${m}`,
          run: () => {
            setSessionLlm(p.id, m);
            if (nav.view !== "chat") nav.showChat("palette");
          },
        });
      }
    }
    return list;
  });

  const filtered = $derived.by(() => {
    const q = query.trim();
    if (!q) {
      if (recentIds.length === 0) return allCommands;
      const rank = new Map(recentIds.map((id, i) => [id, i]));
      return [...allCommands].sort((a, b) => {
        const ra = rank.get(a.id) ?? Number.MAX_SAFE_INTEGER;
        const rb = rank.get(b.id) ?? Number.MAX_SAFE_INTEGER;
        return ra - rb;
      });
    }
    const scored: { cmd: Command; score: number }[] = [];
    for (const c of allCommands) {
      const s = fuzzyBest(q, c.title, c.keywords ?? []);
      if (s !== null) scored.push({ cmd: c, score: s });
    }
    scored.sort((a, b) => b.score - a.score);
    return scored.map((x) => x.cmd);
  });

  const rows = $derived.by(() => {
    const searching = query.trim().length > 0;
    const out: { cmd: Command; first: boolean; index: number }[] = [];
    let lastGroup = "";
    filtered.forEach((cmd, index) => {
      const first = !searching && cmd.group !== lastGroup;
      out.push({ cmd, first, index });
      lastGroup = cmd.group;
    });
    return out;
  });

  $effect(() => {
    query;
    activeIndex = 0;
  });

  $effect(() => {
    if (!$paletteOpen) return;
    query = "";
    activeIndex = 0;
    recentIds = loadRecent();
    void tick().then(() => inputEl?.focus());
  });

  function close() {
    paletteOpen.set(false);
  }

  function runCommand(cmd: Command) {
    pushRecent(cmd.id);
    close();
    void cmd.run();
  }

  function scrollActiveIntoView() {
    void tick().then(() => {
      document
        .querySelector('[data-testid="command-palette"] .palette-item.is-active')
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  function onInputKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      activeIndex = Math.min(activeIndex + 1, filtered.length - 1);
      scrollActiveIntoView();
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      activeIndex = Math.max(activeIndex - 1, 0);
      scrollActiveIntoView();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      const cmd = filtered[activeIndex];
      if (cmd) runCommand(cmd);
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  }
</script>

{#if $paletteOpen}
  <div
    class="palette-overlay"
    data-testid="command-palette"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div class="palette-panel" role="dialog" aria-modal="true" aria-label="命令面板">
      <div class="palette-input-row">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <circle cx="11" cy="11" r="7" />
          <path d="M21 21l-4.3-4.3" />
        </svg>
        <input
          bind:this={inputEl}
          bind:value={query}
          type="text"
          placeholder="搜索会话、动作、模型…"
          aria-label="搜索命令"
          data-testid="palette-input"
          onkeydown={onInputKey}
        />
        <kbd>Esc</kbd>
      </div>
      <ul class="palette-list" role="listbox" aria-label="命令列表">
        {#each rows as row (row.cmd.id)}
          {#if row.first}
            <li class="palette-group" aria-hidden="true">{row.cmd.group}</li>
          {/if}
          <li role="presentation">
            <button
              type="button"
              class="palette-item"
              class:is-active={row.index === activeIndex}
              role="option"
              aria-selected={row.index === activeIndex}
              data-testid="palette-item"
              onclick={() => runCommand(row.cmd)}
              onmouseenter={() => (activeIndex = row.index)}
            >
              <span class="palette-item-title">{row.cmd.title}</span>
              {#if row.cmd.hint}
                <span class="palette-item-hint">{row.cmd.hint}</span>
              {/if}
            </button>
          </li>
        {:else}
          <li class="palette-empty" data-testid="palette-empty">无匹配结果</li>
        {/each}
      </ul>
    </div>
  </div>
{/if}
