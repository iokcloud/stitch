<script lang="ts">
  import { terminalEntries, terminalOpen, toggleTerminal } from "../terminal/store";

  let copyFlash = $state<string | null>(null);

  async function copyText(id: string, text: string) {
    const t = text.trim();
    if (!t) return;
    try {
      await navigator.clipboard.writeText(t);
      copyFlash = id;
      setTimeout(() => {
        if (copyFlash === id) copyFlash = null;
      }, 1200);
    } catch {
      /* ignore */
    }
  }
</script>

{#if $terminalOpen}
  <section class="terminal-panel" data-testid="terminal-panel" aria-label="终端">
    <header class="terminal-panel-head">
      <span class="terminal-panel-title">终端</span>
      <span class="terminal-panel-meta">{$terminalEntries.length} 条命令</span>
      <button
        type="button"
        class="icon-btn shrink-0"
        aria-label="关闭终端"
        title="关闭"
        data-testid="terminal-close"
        onclick={toggleTerminal}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <path d="M6 6l12 12M18 6L6 18" />
        </svg>
      </button>
    </header>
    <div class="terminal-panel-body">
      {#if $terminalEntries.length === 0}
        <p class="terminal-panel-empty">运行命令后，输出会汇总到这里。</p>
      {:else}
        {#each $terminalEntries as entry (entry.id)}
          <article
            class="terminal-entry"
            class:is-error={entry.error}
            data-testid={`terminal-entry-${entry.id}`}
          >
            <div class="terminal-entry-bar">
              <span class="terminal-entry-cmd truncate" title={entry.summary}>{entry.summary || "run_command"}</span>
              <button
                type="button"
                class="icon-btn shrink-0"
                aria-label="复制输出"
                title={copyFlash === entry.id ? "已复制" : "复制"}
                onclick={() => void copyText(entry.id, entry.detail || entry.summary)}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                  <rect x="9" y="9" width="11" height="11" rx="1.5" />
                  <path d="M5 15V5a1.5 1.5 0 0 1 1.5-1.5H15" />
                </svg>
              </button>
            </div>
            {#if entry.detail}
              <pre class="terminal-entry-out">{entry.detail}</pre>
            {/if}
          </article>
        {/each}
      {/if}
    </div>
  </section>
{/if}
