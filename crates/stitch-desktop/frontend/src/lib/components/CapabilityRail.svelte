<script lang="ts">
  import { tick } from "svelte";
  import {
    config,
    fillComposer,
    isStreaming,
    workDir,
    sidebarTab,
  } from "../stores/app";
  import { nav } from "../nav.svelte";
  import { refocusComposerSoon } from "../stores/palette";
  import { pushToast } from "../stores/toasts";
  import * as ipc from "../ipc";
  import type { LocalSkillRow } from "../ipc";
  import type { McpServerSnapshot } from "../types";

  type Panel = "root" | "skills" | "mcp";

  let open = $state(false);
  let panel = $state<Panel>("root");
  let skills = $state<LocalSkillRow[]>([]);
  let loadingSkills = $state(false);
  let skillQuery = $state("");
  let mcpQuery = $state("");
  let togglingId = $state("");
  let rootEl: HTMLDivElement | undefined = $state();

  const mcpServers = $derived(($config?.mcp_servers ?? []) as McpServerSnapshot[]);

  const filteredSkills = $derived.by(() => {
    const q = skillQuery.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter(
      (s) =>
        s.title.toLowerCase().includes(q) ||
        s.slug.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q),
    );
  });

  const filteredMcp = $derived.by(() => {
    const q = mcpQuery.trim().toLowerCase();
    if (!q) return mcpServers;
    return mcpServers.filter(
      (s) =>
        (s.label || "").toLowerCase().includes(q) ||
        s.id.toLowerCase().includes(q) ||
        s.transport.toLowerCase().includes(q),
    );
  });

  async function refreshSkills() {
    loadingSkills = true;
    try {
      skills = await ipc.listLocalSkills();
    } catch {
      skills = [];
    } finally {
      loadingSkills = false;
    }
  }

  $effect(() => {
    void $workDir;
    if (open) void refreshSkills();
  });

  function closeMenu() {
    open = false;
    panel = "root";
    skillQuery = "";
    mcpQuery = "";
    refocusComposerSoon();
  }

  async function toggleMenu() {
    if ($isStreaming) return;
    open = !open;
    if (open) {
      panel = "root";
      await refreshSkills();
      await tick();
    } else {
      closeMenu();
    }
  }

  function onDocPointer(e: PointerEvent) {
    if (!open || !rootEl) return;
    const t = e.target;
    if (t instanceof Node && rootEl.contains(t)) return;
    closeMenu();
  }

  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Consume the key so the window-level handler does not also stop
        // an in-flight generation while the user only meant to dismiss this menu.
        e.stopPropagation();
        closeMenu();
      }
    };
    document.addEventListener("pointerdown", onDocPointer, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDocPointer, true);
      document.removeEventListener("keydown", onKey);
    };
  });

  async function exportSkillTo(slug: string, title: string) {
    try {
      const r = await ipc.exportSkill(slug);
      pushToast(`已导出 ${title}（${r.files} 个文件）到 ${r.path}`);
    } catch (e) {
      const msg = typeof e === "string" ? e : "导出失败";
      pushToast(msg, "error");
    }
  }

  function useSkill(sk: LocalSkillRow) {
    if ($isStreaming) return;
    const hint = sk.description.trim()
      ? `（${sk.description.trim().slice(0, 80)}）`
      : "";
    const where = sk.scope === "user" ? "本机" : "工作区";
    fillComposer(
      `请按${where} Skill「${sk.title}」执行${hint}。Skill 路径：${sk.rel_path}`,
    );
    closeMenu();
  }

  function scopeLabel(scope: string): string {
    return scope === "user" ? "本机" : "工作区";
  }

  async function toggleMcp(id: string, enabled: boolean) {
    if (togglingId) return;
    togglingId = id;
    try {
      const cfg = await ipc.setMcpServerEnabled(id, enabled);
      config.set(cfg);
    } catch {
      /* keep prior */
    } finally {
      togglingId = "";
    }
  }

  function openMcpSettings() {
    closeMenu();
    nav.showSettings({ fromChat: true, tab: "mcp" });
  }

  function openSkillLibrary() {
    closeMenu();
    sidebarTab.set("library");
  }

  function initialLetter(label: string): string {
    const t = label.trim();
    if (!t) return "?";
    return t.charAt(0).toUpperCase();
  }
</script>

<div class="attach-menu" bind:this={rootEl} data-testid="capability-rail">
  <button
    type="button"
    class="attach-trigger"
    class:is-open={open}
    data-testid="attach-menu-trigger"
    aria-label="添加 Skill 或 MCP"
    aria-expanded={open}
    aria-haspopup="menu"
    disabled={$isStreaming}
    onclick={() => void toggleMenu()}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <path d="M12 5v14M5 12h14" />
    </svg>
  </button>

  {#if open}
    <div class="attach-pop" role="menu" data-testid="attach-menu-pop">
      {#if panel === "root"}
        <p class="attach-heading">添加 Skill、MCP…</p>
        <button
          type="button"
          class="attach-item"
          data-testid="attach-open-skills"
          role="menuitem"
          onclick={() => (panel = "skills")}
        >
          <span class="attach-item-icon" aria-hidden="true">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
              <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
              <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
            </svg>
          </span>
          <span class="attach-item-label">Skills</span>
          <span class="attach-item-chevron" aria-hidden="true">›</span>
        </button>
        <button
          type="button"
          class="attach-item"
          data-testid="attach-open-mcp"
          role="menuitem"
          onclick={() => (panel = "mcp")}
        >
          <span class="attach-item-icon" aria-hidden="true">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
              <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83" />
            </svg>
          </span>
          <span class="attach-item-label">MCP Servers</span>
          <span class="attach-item-chevron" aria-hidden="true">›</span>
        </button>
      {:else if panel === "skills"}
        <div class="attach-subhead">
          <button
            type="button"
            class="attach-back"
            data-testid="attach-skills-back"
            aria-label="返回"
            onclick={() => (panel = "root")}
          >‹</button>
          <span>Skills</span>
        </div>
        <input
          class="attach-search"
          type="search"
          placeholder="搜索 Skill…"
          data-testid="attach-skills-search"
          bind:value={skillQuery}
        />
        <div class="attach-list" data-testid="attach-skills-list">
          {#if loadingSkills && skills.length === 0}
            <p class="attach-empty">加载中…</p>
          {:else if filteredSkills.length === 0}
            <p class="attach-empty" data-testid="capability-skills-empty">暂无 Skill</p>
            <button
              type="button"
              class="attach-footer-btn"
              data-testid="capability-skills-browse"
              onclick={openSkillLibrary}
            >去场景安装</button>
          {:else}
            {#each filteredSkills as sk (sk.rel_path)}
              <div class="attach-skill" data-testid={`capability-skill-${sk.slug}`} data-scope={sk.scope}>
                <button
                  type="button"
                  class="attach-skill-main"
                  onclick={() => useSkill(sk)}
                >
                  <span class="attach-skill-head">
                    <span class="attach-skill-title">{sk.title}</span>
                    <span class="attach-skill-scope">{scopeLabel(sk.scope)}</span>
                  </span>
                  {#if sk.description}
                    <span class="attach-skill-desc">{sk.description}</span>
                  {/if}
                </button>
                <button
                  type="button"
                  class="attach-skill-export"
                  title="导出 Skill（复制到所选位置）"
                  aria-label={`导出 ${sk.title}`}
                  data-testid={`capability-skill-export-${sk.slug}`}
                  onclick={() => exportSkillTo(sk.slug, sk.title)}
                >
                  <svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                    <path d="M8 3v7M5 7l3 3 3-3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
                    <path d="M3 12.5h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
                  </svg>
                </button>
              </div>
            {/each}
          {/if}
        </div>
      {:else}
        <div class="attach-subhead">
          <button
            type="button"
            class="attach-back"
            data-testid="attach-mcp-back"
            aria-label="返回"
            onclick={() => (panel = "root")}
          >‹</button>
          <span>MCP Servers</span>
        </div>
        <input
          class="attach-search"
          type="search"
          placeholder="搜索 MCP…"
          data-testid="attach-mcp-search"
          bind:value={mcpQuery}
        />
        <div class="attach-list" data-testid="attach-mcp-list">
          {#if filteredMcp.length === 0}
            <p class="attach-empty" data-testid="capability-mcp-empty">暂无 MCP 服务</p>
          {:else}
            {#each filteredMcp as m (m.id)}
              <div class="attach-mcp-row" data-testid={`capability-mcp-${m.id}`}>
                <span class="attach-mcp-avatar" aria-hidden="true"
                  >{initialLetter(m.label || m.id)}</span
                >
                <div class="attach-mcp-meta">
                  <span class="attach-mcp-name">{m.label || m.id}</span>
                  <span class="attach-mcp-sub">{m.transport}{m.enabled ? "" : " · 停用"}</span>
                </div>
                <button
                  type="button"
                  class="attach-toggle"
                  class:is-on={m.enabled}
                  role="switch"
                  aria-checked={m.enabled}
                  data-testid={`attach-mcp-toggle-${m.id}`}
                  disabled={togglingId === m.id}
                  onclick={() => void toggleMcp(m.id, !m.enabled)}
                >
                  <span class="attach-toggle-knob"></span>
                </button>
              </div>
            {/each}
          {/if}
        </div>
        <button
          type="button"
          class="attach-footer-btn"
          data-testid="capability-mcp-settings"
          onclick={openMcpSettings}
        >打开 MCP 设置</button>
      {/if}
    </div>
  {/if}
</div>
