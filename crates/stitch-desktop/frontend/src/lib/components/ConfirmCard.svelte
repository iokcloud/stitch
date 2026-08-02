<script lang="ts">
  import { tick } from "svelte";
  import {
    confirmOpen,
    confirmTool,
    confirmMessage,
    respondConfirm,
  } from "../stores/app";
  import { parseConfirm } from "../confirm-parse";
  import type { RememberRule } from "../types";

  let sessionAllow = $state(false);
  let rememberRule = $state(false);
  let rootEl: HTMLDivElement | undefined = $state();
  let allowBtn: HTMLButtonElement | undefined = $state();

  const parsed = $derived(parseConfirm($confirmTool, $confirmMessage));

  /**
   * Remember-able confirms carry a path or command scope. Deletes and
   * plain/desktop confirms never offer「记住此规则」.
   */
  const rememberable = $derived(
    (parsed.presentation === "shell" || parsed.presentation === "path") &&
      parsed.kind !== "delete_path",
  );

  function rememberOpts(): { remember?: RememberRule } {
    if (!rememberable || !rememberRule) return {};
    return {
      remember: {
        tool: $confirmTool,
        scope: parsed.presentation === "shell" ? "command" : "path",
        value: parsed.payload,
      },
    };
  }

  $effect(() => {
    if (!$confirmOpen) {
      sessionAllow = false;
      rememberRule = false;
      return;
    }
    void tick().then(() => {
      rootEl?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      allowBtn?.focus();
    });
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      // Esc here means「拒绝本次确认」only — do not let the window-level
      // handler also cancel the whole generation.
      e.stopPropagation();
      void respondConfirm(false);
    } else if (e.key === "Enter" && !e.isComposing) {
      const t = e.target as HTMLElement | null;
      if (t?.tagName === "INPUT") return;
      e.preventDefault();
      void respondConfirm(true, { sessionAllow, ...rememberOpts() });
    }
  }
</script>

{#if $confirmOpen}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="confirm-card"
    class:is-danger={parsed.kind === "delete_path"}
    role="dialog"
    aria-modal="false"
    aria-labelledby="confirm-title"
    tabindex="-1"
    bind:this={rootEl}
    data-testid="confirm-card"
    data-kind={parsed.kind}
    onkeydown={onKey}
  >
    <header class="confirm-head">
      <span class="confirm-icon" aria-hidden="true" data-kind={parsed.kind}>
        {#if parsed.kind === "run_command"}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
            <polyline points="4 17 10 11 4 5" />
            <line x1="12" y1="19" x2="20" y2="19" />
          </svg>
        {:else if parsed.kind === "delete_path"}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
            <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
          </svg>
        {:else}
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
            <path d="M14 3H6a2 2 0 00-2 2v14a2 2 0 002 2h12a2 2 0 002-2V9z" />
            <path d="M14 3v6h6" />
          </svg>
        {/if}
      </span>
      <div class="confirm-titles min-w-0">
        <p class="confirm-kicker">工具确认</p>
        <h2 id="confirm-title" class="confirm-title">{parsed.title}</h2>
        <p class="confirm-hint">{parsed.hint}{#if parsed.meta} · {parsed.meta}{/if}</p>
      </div>
      <span class="confirm-tool-chip" title={$confirmTool}>{$confirmTool}</span>
    </header>

    <div class="confirm-body">
      {#if parsed.presentation === "shell"}
        <div class="confirm-shell" data-testid="confirm-payload">
          <div class="confirm-shell-bar"><span>终端</span></div>
          <pre class="confirm-shell-code"><span class="confirm-prompt">$</span> {parsed.payload}</pre>
        </div>
      {:else if parsed.presentation === "path"}
        <div class="confirm-path" data-testid="confirm-payload">
          <span class="confirm-path-label">路径</span>
          <code class="confirm-path-value">{parsed.payload}</code>
        </div>
      {:else}
        <pre class="confirm-plain" data-testid="confirm-payload">{parsed.payload}</pre>
      {/if}
    </div>

    <div class="confirm-session">
      <label class="confirm-session-row">
        <input type="checkbox" bind:checked={sessionAllow} data-testid="confirm-session-allow" />
        <span>本次生成内允许同类操作</span>
      </label>
      {#if rememberable}
        <label class="confirm-session-row">
          <input type="checkbox" bind:checked={rememberRule} data-testid="confirm-remember" />
          <span>记住此规则（自动允许）</span>
        </label>
      {/if}
    </div>

    <footer class="confirm-actions" data-testid="confirm-dialog">
      <button
        type="button"
        class="confirm-btn confirm-btn-reject"
        data-testid="confirm-reject"
        onclick={() => respondConfirm(false)}>拒绝</button
      >
      <button
        type="button"
        class="confirm-btn confirm-btn-allow"
        bind:this={allowBtn}
        data-testid="confirm-allow"
        onclick={() => respondConfirm(true, { sessionAllow, ...rememberOpts() })}>允许</button
      >
    </footer>
  </div>
{/if}
