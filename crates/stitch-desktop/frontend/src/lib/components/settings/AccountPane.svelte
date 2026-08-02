<script lang="ts">
  import { PROD_API_BASE } from "../../types";
  import { settingsState as s } from "./settings-state.svelte";
</script>

{#if !s.apiTokenStored}
  <div class="account-connect-banner" data-testid="account-connect-banner">
    <p class="settings-pane-desc">
      连接 PromptStdio 账号后，可同步套件与沉淀。无需手抄 Token。
    </p>
    <button
      type="button"
      class="btn-primary"
      data-testid="account-connect-web"
      disabled={s.connectingAccount || s.saving}
      onclick={() => void s.onConnectAccountWeb()}
    >
      {s.connectingAccount ? "等待浏览器…" : "打开网站连接"}
    </button>
  </div>
{/if}
<div class="model-toolbar">
  <p class="settings-pane-desc model-toolbar-desc">
    可添加多套 PromptStdio 账号，侧栏「场景」使用当前默认账号。日常对话不需要此项。
  </p>
  <button
    type="button"
    class="btn-ghost model-add-btn"
    data-testid="settings-mcp-add"
    onclick={s.onAddMcpProfile}
  >
    添加账号
  </button>
</div>

<div class="profile-list" data-testid="settings-mcp-list">
  {#each s.mcpProfiles as p (p.id)}
    <button
      type="button"
      class="profile-chip"
      class:profile-chip-active={p.id === s.selectedMcpId}
      data-testid="settings-mcp-{p.id}"
      onclick={() => s.selectMcp(p.id)}
    >
      <span class="profile-chip-label">{p.label || "PromptStdio"}</span>
      <span
        class="profile-chip-meta"
        class:profile-chip-meta-warn={s.accountProbeOk[p.id] === false}
        data-testid="settings-mcp-chip-meta"
      >{s.mcpChipMeta(p)}</span>
      {#if p.id === s.activeMcpId}
        <span class="profile-chip-badge">默认</span>
      {/if}
    </button>
  {/each}
  {#if !s.mcpProfiles.some((p) => p.id === s.selectedMcpId)}
    <button type="button" class="profile-chip profile-chip-active" data-testid="settings-mcp-draft">
      <span class="profile-chip-label">{s.mcpLabel || "新账号"}</span>
      <span class="profile-chip-meta">未保存</span>
    </button>
  {/if}
</div>

<div class="pref-group" data-testid="settings-promptstdio">
  <label class="pref-row pref-row-stack">
    <span class="pref-label">名称</span>
    <input
      class="field pref-control"
      type="text"
      bind:value={s.mcpLabel}
      placeholder="PromptStdio"
      aria-label="账号名称"
      data-testid="mcp-label"
    />
  </label>

  <div class="pref-row pref-row-stack">
    <div class="pref-label flex items-center gap-2 flex-wrap">
      账号 Token
      {#if s.apiTokenStored}
        <span
          class="pref-badge-check"
          data-testid="api-token-stored"
          title="已保存"
          aria-label="已保存"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        </span>
        {#if s.apiTokenMasked}
          <span
            class="text-[11px] font-mono text-[var(--color-muted)]"
            data-testid="api-token-masked">{s.apiTokenMasked}</span
          >
        {/if}
      {/if}
    </div>
    <div class="relative flex items-center w-full">
      <input
        class="field pref-control pr-10 font-mono text-[12px]"
        type={s.apiTokenVisible ? "text" : "password"}
        bind:value={s.apiToken}
        oninput={s.markTokenDirty}
        placeholder={s.apiTokenPlaceholder}
        autocomplete="off"
        aria-label="账号 Token"
        data-testid="api-token-input"
      />
      <button
        type="button"
        class="absolute right-1 icon-btn z-10"
        title={s.apiTokenVisible ? "隐藏 Token" : "显示 Token"}
        aria-label="切换 Token 可见性"
        aria-pressed={s.apiTokenVisible}
        data-testid="api-token-toggle"
        onclick={s.toggleApiTokenVisible}
      >
        {#if s.apiTokenVisible}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
            <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
            <line x1="1" y1="1" x2="23" y2="23" />
          </svg>
        {:else}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        {/if}
      </button>
    </div>
    <div class="flex items-center justify-between gap-2 w-full flex-wrap">
      <span class="pref-hint">也可在下方粘贴网站个人设置中创建的 Token。</span>
      {#if s.apiTokenStored}
        <button
          type="button"
          class="pref-link"
          data-testid="api-token-clear"
          onclick={s.clearApiToken}>清除</button
        >
      {/if}
    </div>
  </div>

  <div class="pref-advanced">
    <button
      type="button"
      class="pref-advanced-toggle"
      data-testid="mcp-advanced-toggle"
      aria-expanded={s.showMcpAdvanced}
      onclick={() => (s.showMcpAdvanced = !s.showMcpAdvanced)}
    >
      {s.showMcpAdvanced ? "收起高级选项" : "高级选项"}
      {#if !s.showMcpAdvanced && s.usingCustomMcpBase}
        <span class="pref-hint ml-2">已使用自定义地址</span>
      {/if}
    </button>
    {#if s.showMcpAdvanced}
      <label class="pref-row pref-row-stack mt-2">
        <span class="pref-label">服务地址</span>
        <input
          class="field pref-control font-mono text-[12px]"
          type="text"
          bind:value={s.promptApiBase}
          placeholder={PROD_API_BASE}
          aria-label="PromptStdio 服务地址"
          data-testid="prompt-api-base"
        />
        <span class="pref-hint">一般无需修改。仅在使用自建或调试服务时填写。</span>
      </label>
    {/if}
  </div>
</div>

<div class="pref-group mt-4" data-testid="settings-sediment-visibility">
  <div class="pref-row pref-row-stack">
    <span class="pref-label">沉淀目标</span>
    <div class="choice-stack" role="radiogroup" aria-label="沉淀目标">
      <button
        type="button"
        class="choice-card"
        role="radio"
        aria-checked={s.sedimentVisibility === "personal"}
        aria-selected={s.sedimentVisibility === "personal"}
        data-testid="sediment-vis-personal"
        onclick={() => (s.sedimentVisibility = "personal")}
      >
        <span class="choice-card-title">仅个人库</span>
        <span class="choice-card-desc">只写入你的账号，不提交公开</span>
      </button>
      <button
        type="button"
        class="choice-card"
        role="radio"
        aria-checked={s.sedimentVisibility === "explore"}
        aria-selected={s.sedimentVisibility === "explore"}
        data-testid="sediment-vis-explore"
        onclick={() => (s.sedimentVisibility = "explore")}
      >
        <span class="choice-card-title">提交到公共库</span>
        <span class="choice-card-desc">先存个人库，再交审核；通过后进 Explore</span>
      </button>
    </div>
  </div>
</div>

<div class="profile-actions">
  {#if !s.isActiveMcp && s.mcpProfiles.some((p) => p.id === s.selectedMcpId)}
    <button
      type="button"
      class="btn-ghost"
      data-testid="settings-mcp-set-default"
      disabled={s.saving}
      onclick={s.onSetDefaultMcp}
    >
      设为默认
    </button>
  {/if}
  {#if s.mcpProfiles.length > 1 || !s.mcpProfiles.some((p) => p.id === s.selectedMcpId)}
    <button
      type="button"
      class="btn-ghost"
      data-testid="settings-mcp-delete"
      disabled={s.saving || (s.mcpProfiles.length <= 1 && s.mcpProfiles.some((p) => p.id === s.selectedMcpId))}
      onclick={() => {
        if (!s.mcpProfiles.some((p) => p.id === s.selectedMcpId)) {
          const preferred =
            s.mcpProfiles.find((p) => p.id === s.activeMcpId) || s.mcpProfiles[0];
          if (preferred) s.applyMcpToForm(preferred);
          else s.onAddMcpProfile();
          return;
        }
        void s.onDeleteMcpProfile();
      }}
    >
      {s.mcpProfiles.some((p) => p.id === s.selectedMcpId) ? "删除" : "取消"}
    </button>
  {/if}
</div>
