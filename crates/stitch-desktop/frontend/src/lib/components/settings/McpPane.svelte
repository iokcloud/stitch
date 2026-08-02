<script lang="ts">
  import { settingsState as s } from "./settings-state.svelte";
</script>

<div class="model-toolbar">
  <p class="settings-pane-desc model-toolbar-desc">
    列表里开关即可；点选后用 JSON 编辑（与 Cursor 同款字段）。
  </p>
  <div class="model-toolbar-actions">
    <button
      type="button"
      class="btn-ghost model-add-btn"
      data-testid="settings-server-import"
      disabled={s.saving}
      onclick={() => {
        s.showMcpImport = !s.showMcpImport;
        s.status = "";
        s.statusError = false;
      }}
    >
      导入
    </button>
    <button
      type="button"
      class="btn-ghost model-add-btn"
      data-testid="settings-server-add"
      onclick={s.onAddMcpServer}
    >
      添加
    </button>
  </div>
</div>

{#if s.showMcpImport}
  <div class="pref-group" data-testid="settings-mcp-import">
    <label class="pref-row pref-row-stack">
      <span class="pref-label">粘贴 mcpServers JSON</span>
      <textarea
        class="field pref-control font-mono text-[12px] min-h-[100px]"
        bind:value={s.mcpImportJson}
        placeholder={'{\n  "mcpServers": { … }\n}'}
        data-testid="server-import-json"
      ></textarea>
    </label>
    <div class="profile-actions">
      <button
        type="button"
        class="btn-primary"
        data-testid="settings-server-import-apply"
        disabled={s.saving}
        onclick={() => void s.onImportMcpJson()}
      >
        导入
      </button>
    </div>
  </div>
{/if}

<div class="profile-list" data-testid="settings-server-list">
  {#each s.mcpServers as p (p.id)}
    <button
      type="button"
      class="profile-chip"
      class:profile-chip-active={p.id === s.selectedServerId}
      data-testid="settings-server-{p.id}"
      onclick={() => s.selectServer(p.id)}
    >
      <span class="profile-chip-label">{p.label || p.id}</span>
      <span class="profile-chip-meta"
        >{p.transport}{p.enabled ? "" : " · 停用"}</span
      >
    </button>
  {/each}
  {#if s.selectedServerId && !s.mcpServers.some((p) => p.id === s.selectedServerId)}
    <button type="button" class="profile-chip profile-chip-active" data-testid="settings-server-draft">
      <span class="profile-chip-label">新服务</span>
      <span class="profile-chip-meta">未保存</span>
    </button>
  {/if}
</div>

{#if s.selectedServerId}
  <div class="pref-group" data-testid="settings-mcp-server">
    <label class="pref-row pref-row-stack">
      <span class="pref-label">配置 JSON</span>
      <textarea
        class="field pref-control font-mono text-[12px] min-h-[160px]"
        bind:value={s.serverJsonText}
        spellcheck="false"
        data-testid="server-json"
      ></textarea>
    </label>
  </div>
  <div class="profile-actions">
    {#if s.mcpServers.some((p) => p.id === s.selectedServerId)}
      <button
        type="button"
        class="btn-ghost"
        data-testid="settings-server-toggle"
        disabled={s.saving}
        onclick={s.onToggleServerEnabled}
      >
        {s.serverEnabled ? "停用" : "启用"}
      </button>
    {/if}
    <button
      type="button"
      class="btn-ghost"
      data-testid="settings-server-delete"
      disabled={s.saving}
      onclick={() => void s.onDeleteMcpServer()}
    >
      {s.mcpServers.some((p) => p.id === s.selectedServerId) ? "删除" : "取消"}
    </button>
  </div>
{:else}
  <p class="settings-pane-desc" data-testid="settings-mcp-empty">
    点「添加」，或「导入」粘贴 Cursor 同款 JSON。也可
    <button
      type="button"
      class="btn-ghost"
      data-testid="settings-server-add-promptstdio"
      disabled={s.saving}
      onclick={() => void s.onAddPromptstdioMcp()}
    >添加 PromptStdio</button
    >。
  </p>
{/if}
