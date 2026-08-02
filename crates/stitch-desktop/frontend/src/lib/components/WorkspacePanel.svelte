<script lang="ts">
  import { onMount } from "svelte";
  import {
    workspacesData,
    workspaceCollapse,
    activateWorkspace,
    groupExpanded,
    toggleWorkspaceCollapse,
    expandWorkspaceGroup,
    openWorkspaceFolder,
    removeWorkspaceAndActivate,
  } from "../stores/workspaces";
  import {
    sessionsByWorkspace,
    currentSessionId,
    deleteSession,
    renameSession,
    formatRelativeTime,
    openCheckpointDialog,
  } from "../stores/sessions";
  import {
    workDirDialogOpen,
    applyWorkDir,
    isStreaming,
    streamSessionId,
  } from "../stores/app";
  import {
    registerSessionContextHandlers,
    registerWorkspaceContextHandlers,
  } from "../native-context-menu";

  type Props = {
    onSwitchSession: (id: string) => void | Promise<void>;
    onNewSession: () => void;
    onCopyTitle: (title: string, e: MouseEvent) => void | Promise<void>;
  };

  let { onSwitchSession, onNewSession, onCopyTitle }: Props = $props();

  let busyId = $state<string | null>(null);
  let err = $state("");
  let menuSessionId = $state<string | null>(null);
  let menuWorkspaceId = $state<string | null>(null);
  let renamingSessionId = $state<string | null>(null);
  let renameDraft = $state("");
  let sessionQuery = $state("");
  /** Tick so sidebar relative times stay fresh while the app stays open. */
  let nowMs = $state(Date.now());

  const workspaceCount = $derived($workspacesData.items.length);
  const searching = $derived(sessionQuery.trim().length > 0);
  const displayGroups = $derived.by(() => {
    const q = sessionQuery.trim().toLowerCase();
    const groups = $sessionsByWorkspace;
    if (!q) return groups;
    return groups
      .map((g) => ({
        ...g,
        sessions: g.sessions.filter((s) =>
          (s.title || "").toLowerCase().includes(q),
        ),
      }))
      .filter((g) => g.sessions.length > 0);
  });

  $effect(() => {
    const id = setInterval(() => {
      nowMs = Date.now();
    }, 60_000);
    return () => clearInterval(id);
  });

  onMount(() => {
    registerSessionContextHandlers({
      copyTitle: (_sessionId, title) => {
        menuSessionId = null;
        void onCopyTitle(title, new MouseEvent("click"));
      },
      startRename: (sessionId, title) => {
        startRenameSession(sessionId, title);
      },
      rollback: (sessionId) => {
        menuSessionId = null;
        openCheckpointDialog(sessionId);
      },
      delete: (sessionId) => {
        menuSessionId = null;
        deleteSession(sessionId);
      },
    });
    registerWorkspaceContextHandlers({
      openFolder: (workspaceId) => {
        void onOpenFolder(workspaceId);
      },
      remove: (workspaceId) => {
        void onRemoveWorkspace(workspaceId);
      },
    });
    const closeHtmlMenus = () => {
      menuSessionId = null;
      menuWorkspaceId = null;
    };
    document.addEventListener("contextmenu", closeHtmlMenus, true);
    return () => {
      document.removeEventListener("contextmenu", closeHtmlMenus, true);
      registerSessionContextHandlers(null);
      registerWorkspaceContextHandlers(null);
    };
  });

  async function onActivate(id: string) {
    if (busyId) return;
    busyId = id;
    err = "";
    menuSessionId = null;
    menuWorkspaceId = null;
    try {
      await activateWorkspace(id);
    } catch (e) {
      err = String(e);
    } finally {
      busyId = null;
    }
  }

  /** Toolbar: add a workspace (pick / paste path in dialog). */
  function onAddWorkspace() {
    menuSessionId = null;
    menuWorkspaceId = null;
    workDirDialogOpen.set(true);
  }

  /** Row + : new chat under that workspace (activate first so bind path is correct). */
  async function onNewSessionInWorkspace(id: string) {
    if (busyId) return;
    busyId = id;
    err = "";
    menuSessionId = null;
    menuWorkspaceId = null;
    try {
      await activateWorkspace(id);
      onNewSession();
    } catch (e) {
      err = String(e);
    } finally {
      busyId = null;
    }
  }

  function onToggleCollapse(groupId: string, e: MouseEvent) {
    e.stopPropagation();
    menuSessionId = null;
    menuWorkspaceId = null;
    toggleWorkspaceCollapse(groupId);
  }

  /**
   * Directory row: inactive → activate + expand; already active → toggle fold.
   * Chevron stays clickable but shares the same row chrome (no separate hover block).
   */
  async function onWorkspaceMainClick(groupId: string, workspaceId: string | null) {
    if (busyId) return;
    menuSessionId = null;
    menuWorkspaceId = null;
    if (workspaceId && workspaceId !== $workspacesData.currentId) {
      await onActivate(workspaceId);
      expandWorkspaceGroup(groupId);
      return;
    }
    toggleWorkspaceCollapse(groupId);
  }

  function toggleSessionMenu(id: string, e: MouseEvent) {
    e.stopPropagation();
    menuWorkspaceId = null;
    menuSessionId = menuSessionId === id ? null : id;
  }

  function toggleWorkspaceMenu(id: string, e: MouseEvent) {
    e.stopPropagation();
    menuSessionId = null;
    menuWorkspaceId = menuWorkspaceId === id ? null : id;
  }

  function startRenameSession(id: string, title: string) {
    menuSessionId = null;
    renamingSessionId = id;
    renameDraft = title;
    requestAnimationFrame(() => {
      const el = document.querySelector(
        '[data-testid="session-rename"]',
      ) as HTMLInputElement | null;
      el?.focus();
      el?.select();
    });
  }

  function commitRenameSession() {
    if (!renamingSessionId) return;
    renameSession(renamingSessionId, renameDraft);
    renamingSessionId = null;
    renameDraft = "";
  }

  function cancelRename() {
    renamingSessionId = null;
    renameDraft = "";
  }

  async function onOpenFolder(id: string) {
    menuWorkspaceId = null;
    err = "";
    try {
      await openWorkspaceFolder(id);
    } catch (e) {
      err = String(e);
    }
  }

  async function onRemoveWorkspace(id: string) {
    menuWorkspaceId = null;
    if (busyId) return;
    busyId = id;
    err = "";
    try {
      await removeWorkspaceAndActivate(id);
    } catch (e) {
      err = String(e);
    } finally {
      busyId = null;
    }
  }

  function onDocPointerDown(e: PointerEvent) {
    const t = e.target as HTMLElement | null;
    if (!t?.closest?.("[data-session-menu]")) {
      menuSessionId = null;
    }
    if (!t?.closest?.("[data-workspace-menu]")) {
      menuWorkspaceId = null;
    }
  }

  $effect(() => {
    if (!menuSessionId && !menuWorkspaceId) return;
    document.addEventListener("pointerdown", onDocPointerDown, true);
    return () => document.removeEventListener("pointerdown", onDocPointerDown, true);
  });
</script>

<div class="workspace-panel" data-testid="workspace-panel">
  <div class="side-toolbar">
    <span class="workspace-heading" data-testid="workspace-heading">
      工作区{#if workspaceCount > 0}
        <span class="workspace-heading-count">({workspaceCount})</span>
      {/if}
    </span>
    <div class="workspace-toolbar-actions">
      <button
        type="button"
        class="icon-btn"
        aria-label="添加工作区"
        title="添加工作区"
        data-testid="workspace-add"
        onclick={onAddWorkspace}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <path d="M3 7h6l2 2h8v9a1 1 0 01-1 1H4a1 1 0 01-1-1V7z" />
          <path d="M16 3v6M13 6h6" />
        </svg>
      </button>
    </div>
  </div>

  <div class="workspace-search">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
      <circle cx="11" cy="11" r="7" />
      <path d="M21 21l-4.3-4.3" />
    </svg>
    <input
      type="text"
      placeholder="搜索会话…"
      aria-label="搜索会话"
      data-testid="session-search"
      bind:value={sessionQuery}
    />
    {#if searching}
      <button
        type="button"
        class="workspace-search-clear"
        aria-label="清空搜索"
        onclick={() => (sessionQuery = "")}
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      </button>
    {/if}
  </div>

  {#if err}
    <p class="workspace-err" data-testid="workspace-err">{err}</p>
  {/if}

  <div class="workspace-tree" data-testid="workspace-list">
    {#each displayGroups as g (g.id)}
      {@const expanded = searching
        ? true
        : groupExpanded(
            $workspaceCollapse,
            g.id,
            $workspacesData.currentId,
            $workspacesData.items.length,
          )}
      {@const isActive =
        g.workspaceId != null && g.workspaceId === $workspacesData.currentId}
      <div
        class="workspace-group"
        data-testid="workspace-group"
        data-workspace-id={g.id}
        data-expanded={expanded ? "1" : "0"}
      >
        <div
          class="workspace-row"
          class:workspace-row-active={isActive}
          data-testid="workspace-row"
          data-workspace-id={g.id}
          data-active={isActive ? "1" : "0"}
          data-ctx={g.workspaceId ? "workspace" : undefined}
          data-workspace-entry-id={g.workspaceId ?? undefined}
        >
          {#if g.workspaceId}
            {@const wid = g.workspaceId}
            <button
              type="button"
              class="workspace-main"
              title={g.path ?? g.label}
              disabled={busyId === g.id}
              aria-expanded={expanded}
              aria-label={`${expanded ? "折叠" : "展开"} ${g.label}`}
              data-testid="workspace-main"
              onclick={() => void onWorkspaceMainClick(g.id, wid)}
            >
              <span
                class="workspace-chevron"
                data-testid="workspace-collapse"
                aria-hidden="true"
                onclick={(e) => onToggleCollapse(g.id, e)}
              >
                <svg
                  width="12"
                  height="12"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  class="workspace-chevron-icon"
                  class:workspace-chevron-open={expanded}
                >
                  <path d="M6 9l6 6 6-6" />
                </svg>
              </span>
              <svg
                class="workspace-folder-icon"
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.75"
                aria-hidden="true"
              >
                <path d="M3 7h6l2 2h10v10a1 1 0 01-1 1H4a1 1 0 01-1-1V7z" />
              </svg>
              <span class="workspace-label" data-testid="workspace-label">{g.label}</span>
            </button>
            <div class="workspace-row-trailing" data-workspace-menu>
              <button
                type="button"
                class="workspace-more icon-btn"
                aria-label="工作区操作"
                aria-expanded={menuWorkspaceId === wid}
                title="更多"
                data-testid="workspace-more"
                disabled={busyId === g.id}
                onclick={(e) => toggleWorkspaceMenu(wid, e)}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                  <circle cx="5" cy="12" r="1.6" />
                  <circle cx="12" cy="12" r="1.6" />
                  <circle cx="19" cy="12" r="1.6" />
                </svg>
              </button>
              <button
                type="button"
                class="icon-btn workspace-session-new"
                aria-label="新建会话"
                title="新建会话"
                data-testid="session-new"
                disabled={busyId === g.id}
                onclick={(e) => {
                  e.stopPropagation();
                  void onNewSessionInWorkspace(g.id);
                }}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                  <path d="M12 5v14M5 12h14" />
                </svg>
              </button>
              {#if menuWorkspaceId === wid}
                <div class="workspace-menu" role="menu" data-testid="workspace-menu">
                  <button
                    type="button"
                    class="session-menu-item"
                    role="menuitem"
                    data-testid="workspace-open-folder"
                    onclick={(e) => {
                      e.stopPropagation();
                      void onOpenFolder(wid);
                    }}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                      <path d="M3 7h6l2 2h10v10a1 1 0 01-1 1H4a1 1 0 01-1-1V7z" />
                      <path d="M14 11l4 3-4 3M10 14h8" />
                    </svg>
                    打开文件夹
                  </button>
                  <button
                    type="button"
                    class="session-menu-item session-menu-item-danger"
                    role="menuitem"
                    data-testid="workspace-remove-btn"
                    onclick={(e) => {
                      e.stopPropagation();
                      void onRemoveWorkspace(wid);
                    }}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                      <path d="M18 6L6 18M6 6l12 12" />
                    </svg>
                    移除工作区
                  </button>
                </div>
              {/if}
            </div>
          {:else}
            <button
              type="button"
              class="workspace-main"
              title={g.path ?? g.label}
              disabled={busyId === g.id}
              aria-expanded={expanded}
              aria-label={`${expanded ? "折叠" : "展开"} ${g.label}`}
              data-testid="workspace-main"
              onclick={() => {
                if (g.path) void applyWorkDir(g.path, { bindSession: false });
                toggleWorkspaceCollapse(g.id);
              }}
            >
              <span
                class="workspace-chevron"
                data-testid="workspace-collapse"
                aria-hidden="true"
                onclick={(e) => onToggleCollapse(g.id, e)}
              >
                <svg
                  width="12"
                  height="12"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  class="workspace-chevron-icon"
                  class:workspace-chevron-open={expanded}
                >
                  <path d="M6 9l6 6 6-6" />
                </svg>
              </span>
              <svg
                class="workspace-folder-icon"
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.75"
                aria-hidden="true"
              >
                <path d="M3 7h6l2 2h10v10a1 1 0 01-1 1H4a1 1 0 01-1-1V7z" />
              </svg>
              <span class="workspace-label" data-testid="workspace-label">{g.label}</span>
            </button>
          {/if}
        </div>

        {#if expanded}
          <div class="workspace-sessions" data-testid="workspace-sessions">
            {#each g.sessions as s (s.id)}
              {@const relative = formatRelativeTime(s.updatedAt, nowMs)}
              {@const streaming =
                $isStreaming && $streamSessionId != null && $streamSessionId === s.id}
              <div
                class="session-row"
                class:session-row-active={s.id === $currentSessionId}
                role="button"
                tabindex="0"
                data-testid="session-row"
                data-ctx="session"
                data-session-id={s.id}
                data-session-title={s.title}
                data-session-streaming={streaming ? "1" : "0"}
                data-workspace-id={g.id}
                data-active={s.id === $currentSessionId ? "true" : "false"}
                title={s.title}
                onclick={() => {
                  if (renamingSessionId === s.id) return;
                  expandWorkspaceGroup(g.id);
                  menuSessionId = null;
                  menuWorkspaceId = null;
                  void onSwitchSession(s.id);
                }}
                onkeydown={(e) => {
                  if (renamingSessionId === s.id) return;
                  if (e.key === "Enter") {
                    expandWorkspaceGroup(g.id);
                    menuSessionId = null;
                    menuWorkspaceId = null;
                    void onSwitchSession(s.id);
                  }
                }}
              >
                <div class="session-row-main">
                  {#if renamingSessionId === s.id}
                    <input
                      class="session-rename field"
                      type="text"
                      bind:value={renameDraft}
                      aria-label="会话名称"
                      data-testid="session-rename"
                      onclick={(e) => e.stopPropagation()}
                      onkeydown={(e) => {
                        e.stopPropagation();
                        if (e.key === "Enter") {
                          e.preventDefault();
                          commitRenameSession();
                        }
                        if (e.key === "Escape") {
                          e.preventDefault();
                          cancelRename();
                        }
                      }}
                      onblur={commitRenameSession}
                    />
                  {:else}
                    <div class="session-title" data-testid="session-title">{s.title}</div>
                    {#if relative}
                      <div class="session-meta" data-testid="session-time">{relative}</div>
                    {/if}
                  {/if}
                </div>
                <div class="session-row-trailing" data-session-menu>
                  {#if streaming}
                    <span class="session-busy" aria-label="生成中" title="生成中">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                        <path d="M12 3a9 9 0 109 9" stroke-linecap="round" />
                      </svg>
                    </span>
                  {:else}
                    <button
                      type="button"
                      class="session-more icon-btn"
                      aria-label="会话操作"
                      aria-expanded={menuSessionId === s.id}
                      title="更多"
                      data-testid="session-more"
                      onclick={(e) => toggleSessionMenu(s.id, e)}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                        <circle cx="5" cy="12" r="1.6" />
                        <circle cx="12" cy="12" r="1.6" />
                        <circle cx="19" cy="12" r="1.6" />
                      </svg>
                    </button>
                  {/if}
                  {#if menuSessionId === s.id}
                    <div class="session-menu" role="menu" data-testid="session-menu">
                      <button
                        type="button"
                        class="session-menu-item"
                        role="menuitem"
                        data-testid="session-copy-title"
                        onclick={(e) => {
                          e.stopPropagation();
                          menuSessionId = null;
                          void onCopyTitle(s.title, e);
                        }}
                      >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                          <rect x="9" y="9" width="11" height="11" rx="1.5" />
                          <path d="M5 15V5.5A1.5 1.5 0 016.5 4H15" />
                        </svg>
                        复制
                      </button>
                      <button
                        type="button"
                        class="session-menu-item"
                        role="menuitem"
                        data-testid="session-rename-btn"
                        onclick={(e) => {
                          e.stopPropagation();
                          startRenameSession(s.id, s.title);
                        }}
                      >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                          <path d="M12 20h9" />
                          <path d="M16.5 3.5a2.1 2.1 0 013 3L7 19l-4 1 1-4 12.5-12.5z" />
                        </svg>
                        重命名
                      </button>
                      <button
                        type="button"
                        class="session-menu-item"
                        role="menuitem"
                        data-testid="session-rollback-checkpoint"
                        onclick={(e) => {
                          e.stopPropagation();
                          menuSessionId = null;
                          openCheckpointDialog(s.id);
                        }}
                      >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                          <path d="M3 12a9 9 0 1 0 3-6.7" />
                          <path d="M3 4v5h5" />
                        </svg>
                        回退检查点
                      </button>
                      <button
                        type="button"
                        class="session-menu-item session-menu-item-danger"
                        role="menuitem"
                        data-testid="session-delete"
                        onclick={(e) => {
                          e.stopPropagation();
                          menuSessionId = null;
                          deleteSession(s.id);
                        }}
                      >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                          <path d="M3 6h18M8 6V4h8v2M9 10v8M15 10v8M6 6l1 14h10l1-14" />
                        </svg>
                        删除
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            {:else}
              <p class="workspace-empty-sessions">暂无会话</p>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <p class="workspace-empty" data-testid="workspace-empty">
        {searching ? "无匹配会话" : "添加项目目录后显示在此"}
      </p>
    {/each}
  </div>
</div>
