<script lang="ts">
  import { themePreference, applyTheme } from "../../stores/theme";
  import { autoContinueEnabled, setAutoContinueEnabled } from "../../stores/app";
  import type { ThemePreference } from "../../types";
  import { settingsState as s } from "./settings-state.svelte";

  const THEME_OPTIONS: { value: ThemePreference; label: string; testid: string }[] = [
    { value: "light", label: "浅色", testid: "settings-theme-light" },
    { value: "dark", label: "深色", testid: "settings-theme-dark" },
    { value: "system", label: "跟随系统", testid: "settings-theme-system" },
  ];

  // Two-step inline confirm for clearing allow rules (5s timeout to revert).
  let clearConfirm = $state(false);
  let clearTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    return () => {
      if (clearTimer) clearTimeout(clearTimer);
    };
  });
</script>

<p class="settings-pane-desc">Agent 行为与应用更新。</p>

<div class="pref-group" data-testid="settings-system">
  <div class="pref-row">
    <span class="pref-label">外观</span>
    <div
      class="side-seg theme-pref-seg"
      role="tablist"
      aria-label="外观"
      data-testid="settings-theme"
    >
      {#each THEME_OPTIONS as opt (opt.value)}
        <button
          type="button"
          role="tab"
          class="side-seg-btn"
          aria-selected={$themePreference === opt.value}
          data-testid={opt.testid}
          onclick={() => applyTheme(opt.value)}
        >
          {#if opt.value === "light"}
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
              <circle cx="12" cy="12" r="4" />
              <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
            </svg>
          {:else if opt.value === "dark"}
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
              <path d="M21 14.5A8.5 8.5 0 1110.5 3a7 7 0 0010.5 11.5z" />
            </svg>
          {:else}
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
              <rect x="3" y="4" width="18" height="14" rx="2" />
              <path d="M8 21h8M12 18v3" />
            </svg>
          {/if}
          <span>{opt.label}</span>
        </button>
      {/each}
    </div>
  </div>
  <label class="pref-row">
    <span class="pref-label">最大迭代</span>
    <input
      class="field pref-control-sm"
      type="number"
      min="1"
      max="100"
      bind:value={s.maxIterations}
      data-testid="settings-max-iterations"
    />
  </label>
  <div class="pref-row">
    <span class="pref-label">自动续跑</span>
    <div class="flex items-center gap-2 min-w-0">
      <span class="text-[12px] text-[var(--color-muted)] truncate"
        >迭代用满时自动继续，最多连 3 次</span
      >
      <button
        type="button"
        class="btn-ghost shrink-0"
        data-testid="settings-auto-continue"
        aria-pressed={$autoContinueEnabled}
        onclick={() => setAutoContinueEnabled(!$autoContinueEnabled)}
      >
        {$autoContinueEnabled ? "开" : "关"}
      </button>
    </div>
  </div>
  <div class="pref-row">
    <span class="pref-label">应用更新</span>
    <div class="flex items-center gap-2 min-w-0">
      <span class="text-[12px] text-[var(--color-muted)] truncate"
        >{s.updateText || "检查是否有新版本"}</span
      >
      <button
        type="button"
        class="btn-ghost shrink-0"
        data-testid="check-update"
        onclick={s.onUpdateClick}
      >
        {s.installMode ? "安装更新" : "检查更新"}
      </button>
    </div>
  </div>
</div>

<div class="pref-group" data-testid="allow-rules">
  <div class="pref-row">
    <span class="pref-label">允许规则</span>
    {#if s.allowRules.length > 0}
      <button
        type="button"
        class="btn-ghost shrink-0"
        data-testid={clearConfirm ? "allow-rules-clear-confirm" : "allow-rules-clear"}
        onclick={() => {
          if (clearConfirm) {
            clearConfirm = false;
            void s.clearAllowRules();
          } else {
            clearConfirm = true;
            clearTimer = setTimeout(() => (clearConfirm = false), 5000);
          }
        }}
      >
        {clearConfirm ? "确认清空" : "清空"}
      </button>
    {/if}
  </div>
  {#if s.allowRules.length > 0}
    {#each s.allowRules as r, i (r.tool + r.scope + r.value)}
      <div class="pref-row pref-row-allow" data-testid="allow-rule-row-{i}">
        <span class="allow-rule-tool font-mono text-[11px]">{r.tool}</span>
        <span class="allow-rule-scope">{r.scope}</span>
        <span class="allow-rule-value truncate" title={r.value}>{r.value}</span>
        <button
          type="button"
          class="icon-btn shrink-0"
          data-testid="allow-rule-remove-{i}"
          aria-label="删除规则"
          title="删除规则"
          onclick={() => void s.removeAllowRule(i)}
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
    {/each}
  {:else}
    <p class="pref-hint px-0.5" data-testid="allow-rules-empty">
      暂无允许规则——在对话确认卡勾选「记住此规则」后出现
    </p>
  {/if}
</div>

<p class="pref-hint mt-3 px-0.5">
  工作目录在左侧工作区选择（添加工作区或点名称切换），不在此设置。
</p>
