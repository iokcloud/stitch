<script lang="ts">
  import { tick } from "svelte";
  import {
    workDirDialogOpen,
    workDirDialogError,
    workDir,
    recentDirs,
    applyWorkDir,
    pickAndApplyWorkDir,
  } from "../stores/app";

  let path = $state("");
  let applying = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();

  $effect(() => {
    if ($workDirDialogOpen) {
      path = $workDir || "";
      workDirDialogError.set("");
      applying = false;
      void tick().then(() => inputEl?.focus());
    }
  });

  function close() {
    workDirDialogOpen.set(false);
  }

  async function browseAndApply() {
    applying = true;
    workDirDialogError.set("");
    try {
      const p = await pickAndApplyWorkDir();
      if (p) {
        path = p;
        close();
      }
    } catch (e) {
      workDirDialogError.set(String(e));
    } finally {
      applying = false;
    }
  }

  async function confirm() {
    const trimmed = path.trim();
    if (!trimmed) {
      workDirDialogError.set("请选择或输入项目目录");
      return;
    }
    applying = true;
    workDirDialogError.set("");
    try {
      await applyWorkDir(trimmed, { bindSession: true });
      close();
    } catch (e) {
      workDirDialogError.set(String(e));
    } finally {
      applying = false;
    }
  }

  async function pickRecent(d: string) {
    applying = true;
    workDirDialogError.set("");
    try {
      await applyWorkDir(d, { bindSession: true });
      close();
    } catch (e) {
      workDirDialogError.set(String(e));
    } finally {
      applying = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  function onInputKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void confirm();
    }
  }
</script>

{#if $workDirDialogOpen}
  <div
    class="workdir-overlay"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div
      class="workdir-panel"
      role="dialog"
      aria-modal="true"
      aria-labelledby="workdir-title"
      tabindex="-1"
      data-testid="workdir-dialog"
      onkeydown={onKey}
    >
      <div class="px-5 py-3 border-b border-[var(--color-border)]">
        <h2 id="workdir-title" class="text-[15px] font-semibold tracking-tight">工作目录</h2>
        <p class="text-[12px] text-[var(--color-muted)] mt-1 leading-relaxed">
          Agent 在此目录读写文件、运行命令。选择后自动保存。
        </p>
      </div>
      <div class="px-5 py-4">
        <label class="flex flex-col gap-1.5 mb-3">
          <span class="text-[11px] text-[var(--color-muted)]">项目路径</span>
          <div class="flex gap-2">
            <input
              class="field flex-1 font-mono text-[12px]"
              type="text"
              bind:this={inputEl}
              bind:value={path}
              placeholder="粘贴路径，或点浏览选择"
              aria-label="工作目录路径"
              data-testid="workdir-input"
              onkeydown={onInputKey}
            />
            <button
              type="button"
              class="btn-ghost shrink-0"
              data-testid="workdir-browse"
              disabled={applying}
              onclick={browseAndApply}>浏览</button
            >
          </div>
        </label>
        {#if $recentDirs.length}
          <p class="label-caps mb-1.5">最近使用</p>
          <div
            class="flex flex-col border border-[var(--color-border)] rounded-[var(--radius)] mb-3 max-h-32 overflow-y-auto"
          >
            {#each $recentDirs as d}
              <button
                type="button"
                class="text-left text-[11px] font-mono px-3 py-2 border-b border-[var(--color-border)] last:border-b-0 truncate hover:bg-[var(--color-rail)] hover:text-[var(--color-brand-accent)]"
                title={d}
                onclick={() => void pickRecent(d)}>{d}</button
              >
            {/each}
          </div>
        {/if}
        {#if $workDirDialogError}
          <p class="text-[12px] text-[var(--color-error)] mb-2" data-testid="workdir-error">
            {$workDirDialogError}
          </p>
        {/if}
      </div>
      <div class="flex border-t border-[var(--color-border)]">
        <button
          type="button"
          class="flex-1 py-2.5 text-[11px] font-semibold tracking-wider uppercase border-r border-[var(--color-border)]"
          onclick={close}>取消</button
        >
        <button
          type="button"
          class="flex-1 py-2.5 text-[11px] font-semibold tracking-wider uppercase text-[var(--color-brand-accent)] disabled:opacity-45"
          data-testid="workdir-confirm"
          disabled={applying}
          onclick={confirm}>{applying ? "应用中…" : "确定"}</button
        >
      </div>
    </div>
  </div>
{/if}
