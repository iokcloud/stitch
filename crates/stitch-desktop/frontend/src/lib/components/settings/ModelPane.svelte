<script lang="ts">
  import { nav } from "../../nav.svelte";
  import { PROVIDER_ORDER, PROVIDER_PRESETS, providerPresetLabel } from "../../types";
  import { normalizeOpenAiCompatibleBase } from "../../llm-url";
  import { settingsState as s } from "./settings-state.svelte";
</script>

<div class="model-toolbar">
  <p class="settings-pane-desc model-toolbar-desc">
    {nav.settingsFirstRun
      ? "填写密钥即可开始。也可添加其它 OpenAI 兼容基座。"
      : "添加 OpenAI 兼容基座，聊天中按会话切换。"}
  </p>
  <button
    type="button"
    class="btn-ghost model-add-btn"
    data-testid="settings-profile-add"
    onclick={s.onAddProfile}
  >
    添加模型
  </button>
</div>

<div class="profile-list" data-testid="settings-profile-list">
  {#each s.profiles as p (p.id)}
    <button
      type="button"
      class="profile-chip"
      class:profile-chip-active={p.id === s.selectedProfileId}
      data-testid="settings-profile-{p.id}"
      onclick={() => s.selectProfile(p.id)}
    >
      <span class="profile-chip-label">{p.label || providerPresetLabel(p.provider)}</span>
      <span class="profile-chip-meta">{p.model}</span>
      {#if p.id === s.activeProfileId}
        <span class="profile-chip-badge">默认</span>
      {/if}
    </button>
  {/each}
  {#if !s.profiles.some((p) => p.id === s.selectedProfileId)}
    <button type="button" class="profile-chip profile-chip-active" data-testid="settings-profile-draft">
      <span class="profile-chip-label">{s.profileLabel || "新模型"}</span>
      <span class="profile-chip-meta">未保存</span>
    </button>
  {/if}
</div>

<div class="pref-group" data-testid="settings-model">
  <div class="pref-grid-2">
    <label class="pref-row pref-row-stack">
      <span class="pref-label">提供商</span>
      <select
        class="field pref-control"
        bind:value={s.provider}
        onchange={s.onProviderChange}
        aria-label="选择模型提供商"
        data-testid="profile-provider"
      >
        {#each PROVIDER_ORDER as id}
          <option value={id}>{PROVIDER_PRESETS[id].label}</option>
        {/each}
      </select>
    </label>

    <label class="pref-row pref-row-stack">
      <span class="pref-label">模型名称</span>
      <div class="relative w-full">
        <input
          class="field pref-control font-mono text-[12px]"
          type="text"
          bind:value={s.model}
          oninput={s.markKeyDirty}
          placeholder="例如 gpt-4o 或 glm-4-flash"
          aria-label="模型名称"
          data-testid="profile-model"
          onfocus={() => (s.modelQuickOpen = s.models.length > 0)}
          onblur={() => setTimeout(() => (s.modelQuickOpen = false), 150)}
        />
        {#if s.modelQuickOpen && s.models.length}
          <div
            class="absolute top-full left-0 right-0 mt-1 bg-[var(--color-surface)] border border-[var(--color-border-strong)] rounded-[var(--radius)] z-50 max-h-40 overflow-y-auto"
          >
            {#each s.models as m}
              <button
                type="button"
                class="w-full text-left px-3 py-2 text-[12px] font-mono border-b border-[var(--color-border)] hover:bg-[var(--color-rail)]"
                onmousedown={() => {
                  s.model = m;
                  s.modelQuickOpen = false;
                }}>{m}</button
              >
            {/each}
          </div>
        {/if}
      </div>
    </label>
  </div>

  <label class="pref-row pref-row-stack">
    <span class="pref-label">接口地址</span>
    <input
      class="field pref-control font-mono text-[12px]"
      type="text"
      bind:value={s.apiBase}
      oninput={s.markKeyDirty}
      onblur={() => {
        const n = normalizeOpenAiCompatibleBase(s.apiBase);
        if (n !== s.apiBase) s.apiBase = n;
      }}
      placeholder="https://api.example.com/v1"
      aria-label="API 基础地址"
      data-testid="profile-api-base"
    />
    <span class="pref-hint">支持完整 chat/completions 地址，保存时自动归一为兼容根地址。</span>
  </label>

  <div class="pref-row pref-row-stack">
    <div class="pref-label flex items-center gap-2">
      API Key
      {#if s.apiKeyStored}
        <span
          class="pref-badge-check"
          data-testid="api-key-stored"
          title="已保存"
          aria-label="已保存"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        </span>
      {/if}
    </div>
    <div class="relative flex items-center w-full">
      <input
        class="field pref-control pr-10 font-mono text-[12px]"
        type={s.apiKeyVisible ? "text" : "password"}
        bind:value={s.apiKey}
        oninput={s.markKeyDirty}
        placeholder={s.apiKeyPlaceholder}
        autocomplete="off"
        aria-label="API 密钥"
        data-testid="api-key-input"
      />
      <button
        type="button"
        class="absolute right-1 icon-btn z-10"
        title={s.apiKeyVisible ? "隐藏密钥" : "显示密钥"}
        aria-label="切换密钥可见性"
        aria-pressed={s.apiKeyVisible}
        data-testid="api-key-toggle"
        onclick={s.toggleApiKeyVisible}
      >
        {#if s.apiKeyVisible}
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
    {#if s.apiKeyStored && !s.apiKey}
      <span class="pref-hint">已保存的密钥不会再次显示明文。</span>
    {/if}
  </div>
</div>

<div class="profile-actions">
  {#if !s.isActiveProfile && s.profiles.some((p) => p.id === s.selectedProfileId)}
    <button
      type="button"
      class="btn-ghost"
      data-testid="settings-profile-default"
      disabled={s.saving}
      onclick={s.onSetDefaultProfile}
    >
      设为默认
    </button>
  {/if}
  {#if s.profiles.length > 1 || !s.profiles.some((p) => p.id === s.selectedProfileId)}
    <button
      type="button"
      class="btn-ghost"
      data-testid="settings-profile-delete"
      disabled={s.saving || (s.profiles.length <= 1 && s.profiles.some((p) => p.id === s.selectedProfileId))}
      onclick={() => {
        if (!s.profiles.some((p) => p.id === s.selectedProfileId)) {
          const preferred =
            s.profiles.find((p) => p.id === s.activeProfileId) || s.profiles[0];
          if (preferred) s.applyProfileToForm(preferred);
          else s.onAddProfile();
          return;
        }
        void s.onDeleteProfile();
      }}
    >
      {s.profiles.some((p) => p.id === s.selectedProfileId) ? "删除" : "取消"}
    </button>
  {/if}
</div>

<div class="pref-group" data-testid="settings-model-local-vision">
  <label class="pref-row pref-row-stack">
    <span class="pref-label">本地视觉描述</span>
    <span class="pref-hint">无视觉模型发图时，用本机模型描述图片（如 Ollama qwen3-vl），图片不出本机</span>
    <input
      type="checkbox"
      class="local-vision-check"
      data-testid="local-vision-enabled"
      bind:checked={s.localVisionEnabled}
    />
  </label>
  {#if s.localVisionEnabled}
    <div class="pref-grid-2">
      <label class="pref-row pref-row-stack">
        <span class="pref-label">接口地址</span>
        <input
          class="field pref-control font-mono text-[12px]"
          type="text"
          data-testid="local-vision-api-base"
          bind:value={s.localVisionApiBase}
          placeholder="http://127.0.0.1:11434/v1"
        />
      </label>
      <label class="pref-row pref-row-stack">
        <span class="pref-label">模型名</span>
        <input
          class="field pref-control font-mono text-[12px]"
          type="text"
          data-testid="local-vision-model"
          bind:value={s.localVisionModel}
          placeholder="qwen3-vl:8b"
        />
      </label>
    </div>
  {/if}
</div>
