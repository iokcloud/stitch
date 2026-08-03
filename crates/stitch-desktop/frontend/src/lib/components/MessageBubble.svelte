<script lang="ts">
  import { tick } from "svelte";
  import { renderMarkdown } from "../markdown";

  interface Props {
    role: "user" | "assistant";
    content: string;
    /** Image data URLs on a user message (in-memory only). */
    images?: string[];
    /** True after a restart when the persisted copy had images stripped. */
    imagesStripped?: boolean;
    error?: boolean;
    streaming?: boolean;
    thinking?: boolean;
    onRetry?: () => void;
    /** Iteration budget exhausted — resume the same session with one tap. */
    onContinue?: () => void;
    /** Regenerate this answer (last assistant message only). */
    onRegenerate?: () => void;
    /** Edit this user message and resend (replaces the turn). */
    onEdit?: () => void;
    onSediment?: () => void;
    /** True when Done already prefilled a quiet draft (ADR-036). */
    sedimentReady?: boolean;
    /** 受控展开（虚拟化重建时保持——ChatView 全局 Record 管理）。 */
    expanded?: boolean;
    onToggleExpanded?: (v: boolean) => void;
  }

  let {
    role,
    content,
    images = [],
    imagesStripped = false,
    error = false,
    streaming = false,
    thinking = false,
    onRetry,
    onContinue,
    onRegenerate,
    onEdit,
    onSediment,
    sedimentReady = false,
    /** 受控展开（虚拟化重建时保持——ChatView 全局 Record 管理）。 */
    expanded = false,
    onToggleExpanded = (_v: boolean) => {},
  }: Props = $props();

  let copied = $state(false);
  let needsClamp = $state(false);
  let bodyEl: HTMLDivElement | undefined = $state();

  const html = $derived(
    role === "assistant" && !error && !streaming ? renderMarkdown(content || "") : "",
  );

  /** Match `.message-clamp` max-height in app.css (px). */
  const CLAMP_MAX_PX = 320;
  const COLLAPSE_CHARS = 900;

  $effect(() => {
    content;
    streaming;
    expanded;
    void measureClamp();
  });

  async function measureClamp() {
    await tick();
    if (streaming || thinking || !content) {
      needsClamp = false;
      return;
    }
    // 展开时同样计算：流式重新生成会把 needsClamp 置 false，若展开态提前返回，
    // 折叠控件在流式结束后永远不恢复。实际裁剪已由 class:message-clamp 的
    // {needsClamp && !expanded} 门控，展开中不会误裁。
    if (content.length > COLLAPSE_CHARS) {
      needsClamp = true;
      return;
    }
    needsClamp = bodyEl !== undefined && bodyEl.scrollHeight > CLAMP_MAX_PX;
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(content);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = content;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  async function onBodyClick(e: MouseEvent) {
    const t = e.target as HTMLElement | null;
    const btn = t?.closest?.(".md-code-copy") as HTMLButtonElement | null;
    if (!btn) return;
    e.preventDefault();
    const block = btn.closest(".md-code-block");
    const pre = block?.querySelector("pre");
    const text = pre?.textContent ?? "";
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    btn.setAttribute("title", "已复制");
    btn.classList.add("is-copied");
    setTimeout(() => {
      btn.setAttribute("title", "复制代码");
      btn.classList.remove("is-copied");
    }, 1600);
  }
</script>

<div
  class="message flex flex-col
    {streaming ? '' : 'animate-[message-in_160ms_ease]'}
    {role === 'user' ? 'self-end items-end max-w-[min(85%,36rem)]' : 'self-stretch items-stretch w-full max-w-none'}"
>
  {#if role === "assistant"}
    <div class="msg-role">Stitch</div>
  {/if}
  <div
    class="relative group text-[13px]
      {role === 'user'
      ? 'message-user px-3 py-2 rounded-lg bg-[var(--color-user-bubble)] text-[var(--color-user-text)]'
      : error
        ? 'msg-assistant is-error'
        : 'msg-assistant'}
"  >
    {#if thinking}
      <span class="inline-flex items-center gap-2" aria-label="思考中">
        <span class="inline-flex gap-1">
          <span class="w-1 h-1 rounded-full bg-[var(--color-muted)] animate-[thinking-dots_1.4s_infinite]"
          ></span>
          <span
            class="w-1 h-1 rounded-full bg-[var(--color-muted)] animate-[thinking-dots_1.4s_infinite]"
            style="animation-delay:0.2s"
          ></span>
          <span
            class="w-1 h-1 rounded-full bg-[var(--color-muted)] animate-[thinking-dots_1.4s_infinite]"
            style="animation-delay:0.4s"
          ></span>
        </span>
        <span class="text-[11px] text-[var(--color-muted)]">思考中</span>
      </span>
    {:else if role === "assistant" && !error && streaming}
      <!-- Plain text while streaming — avoid full Markdown remount every token. -->
      <div class="msg-body">
        <div
          class="msg-stream-text whitespace-pre-wrap break-words leading-[1.5]"
          data-testid="msg-stream-text"
        >
          {content}
          {#if streaming}
            <span class="stream-caret" aria-hidden="true"></span>
          {/if}
        </div>
      </div>
    {:else if role === "assistant" && !error}
      <div class="msg-body" class:is-clamped={needsClamp && !expanded}>
        <div
          bind:this={bodyEl}
          class="md-content"
          class:message-clamp={needsClamp && !expanded}
          onclick={onBodyClick}
          role="presentation"
        >
          {@html html}
        </div>
        {#if needsClamp}
          <div class="message-clamp-bar">
            <button
              type="button"
              class="message-expand"
              class:is-open={expanded}
              aria-expanded={expanded}
              aria-label={expanded ? "收起" : "展开全文"}
              title={expanded ? "收起" : "展开全文"}
              data-testid="message-expand"
              onclick={() => onToggleExpanded(!expanded)}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                <path d="M6 9l6 6 6-6" />
              </svg>
            </button>
          </div>
        {/if}
      </div>
    {:else}
      <div class="msg-body" class:is-clamped={needsClamp && !expanded}>
        {#if images.length > 0}
          <div class="msg-images">
            {#each images as url}
              <img data-testid="msg-image" src={url} alt="" />
            {/each}
          </div>
        {:else if imagesStripped}
          <div class="msg-images-stripped" data-testid="msg-image-placeholder">（图片）</div>
        {/if}
        <div
          bind:this={bodyEl}
          class="whitespace-pre-wrap break-words leading-[1.5]"
          class:message-clamp={needsClamp && !expanded}
        >
          {content}
        </div>
        {#if needsClamp}
          <div class="message-clamp-bar">
            <button
              type="button"
              class="message-expand"
              class:is-open={expanded}
              aria-expanded={expanded}
              aria-label={expanded ? "收起" : "展开全文"}
              title={expanded ? "收起" : "展开全文"}
              data-testid="message-expand"
              onclick={() => onToggleExpanded(!expanded)}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
                <path d="M6 9l6 6 6-6" />
              </svg>
            </button>
          </div>
        {/if}
      </div>
    {/if}

    {#if !streaming && content && role === "assistant"}
      <div class="msg-actions">
        <button
          type="button"
          class="msg-action"
          aria-label="复制"
          title={copied ? "已复制" : "复制"}
          onclick={copy}
        >
          {#if copied}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
              <path d="M5 12.5l5 5L19 7" />
            </svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
              <rect x="9" y="9" width="11" height="11" rx="1.5" />
              <path d="M5 15V5.5A1.5 1.5 0 016.5 4H15" />
            </svg>
          {/if}
        </button>
        {#if onRegenerate && !error}
          <button
            type="button"
            class="msg-action text-label"
            data-testid="msg-regenerate"
            aria-label="重新生成"
            title="重新生成"
            onclick={onRegenerate}>重新生成</button>
        {/if}
        {#if onSediment && !error}
          <button
            type="button"
            class="msg-action text-label"
            data-testid="msg-sediment"
            data-sediment-ready={sedimentReady ? "true" : "false"}
            aria-label="保存"
            title="存成提示词"
            onclick={onSediment}
          >保存</button>
        {/if}
        {#if onContinue && !error}
          <button
            type="button"
            class="msg-action text-label"
            data-testid="msg-continue"
            aria-label="继续执行"
            title="继续执行"
            onclick={onContinue}>继续执行</button>
        {/if}
        {#if error && onRetry}
          <button type="button" class="msg-action text-label" aria-label="重试" onclick={onRetry}
            >重试</button
          >
        {/if}
      </div>
    {/if}

    {#if !streaming && content && role === "user" && onEdit}
      <div class="msg-actions msg-actions-user">
        <button
          type="button"
          class="msg-action text-label"
          data-testid="msg-edit"
          aria-label="编辑"
          title="编辑并重发"
          onclick={onEdit}>编辑</button>
      </div>
    {/if}
  </div>
</div>
