<script lang="ts">
  import { tick } from "svelte";
  import type { CheckpointSummaryDto } from "../ipc";
  import {
    checkpointDialogSessionId,
    closeCheckpointDialog,
    rollbackSessionToCheckpoint,
  } from "../stores/sessions";

  let list = $state<CheckpointSummaryDto[]>([]);
  let selectedEpoch = $state<number | null>(null);
  let diffText = $state("");
  let loading = $state(false);
  let applying = $state(false);
  let error = $state("");
  let panelEl: HTMLDivElement | undefined = $state();

  const sid = $derived($checkpointDialogSessionId);
  const open = $derived(sid != null);
  /** Newest first from IPC; index 0 = current committed. */
  const currentEpoch = $derived(list[0]?.epoch ?? null);
  const canRollback = $derived(
    selectedEpoch != null &&
      currentEpoch != null &&
      selectedEpoch < currentEpoch &&
      !applying,
  );

  $effect(() => {
    if (!sid) {
      list = [];
      selectedEpoch = null;
      diffText = "";
      error = "";
      loading = false;
      applying = false;
      return;
    }
    void loadList(sid);
  });

  async function loadList(sessionId: string) {
    loading = true;
    error = "";
    diffText = "";
    selectedEpoch = null;
    try {
      const ipc = await import("../ipc");
      const rows = await ipc.listSessionCheckpoints(sessionId);
      list = rows;
      if (rows.length < 2) {
        error = "没有可回退的检查点";
      } else {
        // Default select previous generation
        selectedEpoch = rows[1].epoch;
        await loadDiff(sessionId, rows[1].epoch, rows[0].epoch);
      }
      await tick();
      panelEl?.focus();
    } catch (e) {
      error = String(e);
      list = [];
    } finally {
      loading = false;
    }
  }

  async function loadDiff(sessionId: string, from: number, to: number) {
    try {
      const ipc = await import("../ipc");
      const d = await ipc.diffSessionCheckpoints(sessionId, from, to);
      diffText = d.text.trim();
    } catch (e) {
      diffText = String(e);
    }
  }

  async function onSelect(epoch: number) {
    if (!sid || epoch === currentEpoch) return;
    selectedEpoch = epoch;
    if (currentEpoch != null) {
      await loadDiff(sid, epoch, currentEpoch);
    }
  }

  async function confirmRollback() {
    if (!sid || selectedEpoch == null || !canRollback) return;
    applying = true;
    error = "";
    try {
      const r = await rollbackSessionToCheckpoint(sid, selectedEpoch);
      if (!r.ok) {
        error = r.reason;
        return;
      }
      closeCheckpointDialog();
    } finally {
      applying = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      // Close dialog only — do not bubble into window Esc → cancelGeneration.
      e.stopPropagation();
      closeCheckpointDialog();
    }
  }
</script>

{#if open}
  <div
    class="workdir-overlay"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) closeCheckpointDialog();
    }}
  >
    <div
      class="workdir-panel checkpoint-panel"
      role="dialog"
      aria-modal="true"
      aria-labelledby="checkpoint-title"
      tabindex="-1"
      data-testid="checkpoint-dialog"
      bind:this={panelEl}
      onkeydown={onKey}
    >
      <div class="px-5 py-3 border-b border-[var(--color-border)]">
        <h2 id="checkpoint-title" class="text-[15px] font-semibold tracking-tight">
          检查点
        </h2>
      </div>

      <div class="px-5 py-4 checkpoint-body">
        {#if loading}
          <p class="text-[12px] text-[var(--color-muted)]">加载中…</p>
        {:else if list.length === 0}
          <p class="text-[12px] text-[var(--color-muted)]" data-testid="checkpoint-empty">
            {error || "暂无检查点"}
          </p>
        {:else}
          <ul class="checkpoint-list" data-testid="checkpoint-list" role="listbox" aria-label="检查点列表">
            {#each list as row, i}
              {@const isCurrent = i === 0}
              {@const selected = selectedEpoch === row.epoch}
              <li>
                <button
                  type="button"
                  class="checkpoint-row"
                  class:is-current={isCurrent}
                  class:is-selected={selected && !isCurrent}
                  role="option"
                  aria-selected={isCurrent ? false : selected}
                  disabled={isCurrent || applying}
                  data-testid={isCurrent ? "checkpoint-current" : `checkpoint-epoch-${row.epoch}`}
                  onclick={() => void onSelect(row.epoch)}
                >
                  <span class="checkpoint-row-meta">
                    <span class="checkpoint-epoch">#{row.epoch}</span>
                    {#if isCurrent}
                      <span class="checkpoint-badge">当前</span>
                    {/if}
                    <span class="checkpoint-level">{row.compression_level}</span>
                  </span>
                  <span class="checkpoint-preview" title={row.summary_preview}>
                    {row.summary_preview || "（无摘要）"}
                  </span>
                </button>
              </li>
            {/each}
          </ul>

          {#if selectedEpoch != null && currentEpoch != null && selectedEpoch < currentEpoch}
            <div class="checkpoint-diff" data-testid="checkpoint-diff">
              <p class="label-caps mb-1.5">与当前差异</p>
              <pre class="checkpoint-diff-text">{diffText || "…"}</pre>
            </div>
          {/if}
        {/if}

        {#if error && list.length > 0}
          <p class="text-[12px] text-[var(--color-error)] mt-2" data-testid="checkpoint-error">
            {error}
          </p>
        {/if}
      </div>

      <div class="flex border-t border-[var(--color-border)]">
        <button
          type="button"
          class="flex-1 py-2.5 text-[11px] font-semibold tracking-wider uppercase border-r border-[var(--color-border)]"
          data-testid="checkpoint-cancel"
          onclick={closeCheckpointDialog}>取消</button
        >
        <button
          type="button"
          class="flex-1 py-2.5 text-[11px] font-semibold tracking-wider uppercase text-[var(--color-brand-accent)] disabled:opacity-45"
          data-testid="checkpoint-confirm"
          disabled={!canRollback}
          onclick={() => void confirmRollback()}
          >{applying ? "回退中…" : "回退"}</button
        >
      </div>
    </div>
  </div>
{/if}
