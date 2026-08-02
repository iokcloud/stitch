<script lang="ts">
  import { onMount } from "svelte";
  import { diagEntries, diagLastError, clearDiagError, recentDiagText } from "../diag";
  import { nav } from "../nav.svelte";

  let open = $state(false);
  /** Opt-in always-on strip: localStorage stitch-diag=1（排障用） */
  let forceShow = $state(false);

  onMount(() => {
    try {
      forceShow = localStorage.getItem("stitch-diag") === "1";
    } catch {
      /* ignore */
    }
  });

  const showChrome = $derived(!!$diagLastError || open || forceShow);
</script>

<!-- Default: sr-only so chat UI stays clean; e2e still reads diag-view / diag-last.
     Visible when error, user expands log, or stitch-diag=1. -->
<div
  class={showChrome
    ? "shrink-0 border-b border-[var(--color-border)] bg-[var(--color-rail)] px-3 py-1 text-[11px] font-mono z-[100]"
    : "sr-only"}
  data-testid="diag-banner"
  role="status"
>
  <div class="flex items-center gap-2 min-h-[22px]">
    <span class="text-[var(--color-muted)] shrink-0" data-testid="diag-view"
      >view={nav.view}</span
    >
    {#if showChrome}
      <span class="text-[var(--color-border-strong)]">|</span>
    {/if}
    {#if $diagLastError}
      <span class="text-[var(--color-error)] flex-1 min-w-0 truncate" data-testid="diag-error">
        {$diagLastError}
      </span>
      <button
        type="button"
        class="btn-ghost text-[10px] py-0.5 px-2 shrink-0"
        data-testid="diag-dismiss"
        onclick={clearDiagError}>清除</button
      >
    {:else}
      <span
        class="text-[var(--color-muted)] flex-1 min-w-0 truncate"
        data-testid="diag-last"
        title={$diagEntries.at(-1)?.msg ?? ""}
      >
        {$diagEntries.at(-1)?.msg ?? "就绪"}
      </span>
    {/if}
    {#if showChrome}
      <button
        type="button"
        class="btn-ghost text-[10px] py-0.5 px-2 shrink-0"
        data-testid="diag-toggle"
        onclick={() => (open = !open)}>{open ? "收起" : "日志"}</button
      >
    {/if}
  </div>
  {#if open && showChrome}
    <pre
      class="mt-1 max-h-32 overflow-y-auto whitespace-pre-wrap text-[10px] text-[var(--color-foreground-secondary)] border-t border-[var(--color-border)] pt-1"
      data-testid="diag-log">{recentDiagText(16)}</pre
    >
  {/if}
</div>
