<script lang="ts">
  import { detectOutputFormat, formatElapsed, parseListing, toolLabel } from "../output-format";

  interface Props {
    name: string;
    done: boolean;
    error: boolean;
    summary: string;
    detail: string;
    expanded?: boolean;
    /** Elapsed ms while running (optional, from parent tick). */
    elapsedMs?: number;
    /** Visual grouping with previous tool chip. */
    stacked?: boolean;
    /** Started while skill-recording mode was active. */
    recorded?: boolean;
    /** Per-tool benchmark metrics (duration_ms, …); exposed as data attr for tests/benchmark. */
    metrics?: Record<string, number>;
    /** 展开状态写回（用户手动折叠后虚拟化重建/视图重建不还原展开）。 */
    onToggle?: (open: boolean) => void;
  }

  let {
    name,
    done,
    error,
    summary,
    detail,
    expanded = false,
    elapsedMs = 0,
    stacked = false,
    recorded = false,
    metrics = undefined,
    onToggle = undefined,
  }: Props = $props();
  let open = $state(false);
  let copied = $state(false);
  let liveBodyEl = $state<HTMLElement | null>(null);

  $effect(() => {
    open = expanded;
  });

  // Keep the live output pinned to the latest line while it grows.
  $effect(() => {
    if (done || !liveBodyEl) return;
    detail;
    liveBodyEl.scrollTop = liveBodyEl.scrollHeight;
  });

  const title = $derived(toolLabel(name));
  const format = $derived(detectOutputFormat(detail || summary, { toolName: name }));
  const listing = $derived(
    format.kind === "listing" ? parseListing(detail || summary) : null,
  );
  const listingEntries = $derived(Array.isArray(listing?.entries) ? listing.entries : []);
  /** User cancelled while this tool was still running. */
  const interrupted = $derived(done && !error && /^已停止/.test((summary || "").trim()));

  /** 运行中单行实时尾巴——最新一行输出，卡片高度稳定不抢滚动。 */
  const liveTail = $derived.by(() => {
    if (!detail) return "";
    const lines = detail
      .split(/\r?\n/)
      .map((l) => l.trimEnd())
      .filter((l) => l.length > 0);
    const last = lines[lines.length - 1] ?? "";
    return last.length > 120 ? `${last.slice(0, 117)}…` : last;
  });

  const headline = $derived.by(() => {
    if (!done) return "运行中";
    if (interrupted) return "已停止";
    if (error) return firstMeaningfulLine(summary) || "失败";
    if (name === "run_command") {
      const m = summary.match(/^(?:完成[:：]?\s*)?(.+)$/s);
      const cmd = firstMeaningfulLine(m?.[1] || summary);
      if (cmd && cmd !== "完成" && cmd.length < 120) return cmd;
    }
    const pathHit =
      summary.match(/\bto\s+(.+)$/i) ||
      summary.match(/->\s*(.+)$/) ||
      summary.match(/Wrote\s+\d+\s+lines?\s+to\s+(.+)$/i) ||
      summary.match(/^(?:Wrote|Created|Edited|Deleted)\s+(.+)$/i);
    if (pathHit?.[1]) return firstMeaningfulLine(pathHit[1]);
    if (listing?.root) return listing.root.replace(/\/$/, "");
    if (summary === "完成" || /^完成/.test(summary)) return title;
    return firstMeaningfulLine(summary) || "完成";
  });

  const headlineTitle = $derived.by(() => {
    const src = (detail || summary || "").trim();
    if (!src) return headline;
    return src
      .split(/\r?\n/)
      .map((l) => l.trimEnd())
      .filter((l) => l.length > 0)
      .slice(0, 8)
      .join("\n");
  });

  function firstMeaningfulLine(text: string | undefined): string {
    if (!text) return "";
    const line = text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .find((l) => l.length > 0);
    return line || text.trim();
  }

  /** Success + collapsed → Cursor-like thin chip (less chrome). */
  const compact = $derived(done && !error && !interrupted && !open);

  const subline = $derived.by(() => {
    if (!done || error || compact) return "";
    const wrote = summary.match(/Wrote\s+(\d+)\s+lines/i);
    if (wrote) return `已写入 ${wrote[1]} 行`;
    if (listing) {
      const entries = Array.isArray(listing.entries) ? listing.entries : [];
      const dirs = entries.filter((e) => e.kind === "dir").length;
      const files = entries.filter((e) => e.kind === "file").length;
      if (dirs || files) return `${dirs} 个目录 · ${files} 个文件`;
    }
    if ((format.kind === "shell" || format.kind === "diff") && detail) {
      const n = detail.split("\n").filter(Boolean).length;
      if (n > 1) return `${n} 行`;
    }
    return "";
  });

  /** One-line preview under the chip when collapsed shell/diff (no auto-expand). */
  const peek = $derived.by(() => {
    if (!compact || !detail) return "";
    if (format.kind !== "shell" && format.kind !== "diff") return "";
    const lines = detail
      .split(/\r?\n/)
      .map((l) => l.trimEnd())
      .filter((l) => l.length > 0)
      .slice(0, 2);
    if (!lines.length) return "";
    const joined = lines.join(" · ");
    return joined.length > 140 ? `${joined.slice(0, 137)}…` : joined;
  });

  async function copyDetail(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    const text = detail || summary;
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
    copied = true;
    setTimeout(() => (copied = false), 1600);
  }
</script>

<div
  class="tool-call"
  class:is-running={!done}
  class:is-error={done && error}
  class:is-done={done && !error && !interrupted}
  class:is-stopped={interrupted}
  class:is-compact={compact}
  class:is-stacked={stacked}
  data-testid="tool-status"
  data-running={!done ? "true" : "false"}
  data-stopped={interrupted ? "true" : "false"}
  data-format={format.kind}
  data-tool={name}
  data-recording={recorded ? "true" : undefined}
  data-metrics={metrics ? JSON.stringify(metrics) : undefined}
>
  <div class="tool-call-head">
    <button
      type="button"
      class="tool-call-main"
      aria-expanded={open}
      aria-label={open ? "收起" : "展开"}
      title={open ? "收起" : "展开"}
      onclick={() => {
        open = !open;
        onToggle?.(open);
      }}
    >
      <span class="tool-call-chevron" class:open aria-hidden="true">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M9 6l6 6-6 6" />
        </svg>
      </span>

      <span class="tool-call-state" aria-hidden="true">
        {#if !done}
          <span class="spin"></span>
        {:else if interrupted}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
            <circle cx="12" cy="12" r="9" />
            <path d="M9 9h6v6H9z" />
          </svg>
        {:else if error}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
            <circle cx="12" cy="12" r="9" />
            <path d="M12 8v5M12 16h.01" />
          </svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
            <circle cx="12" cy="12" r="9" />
            <path d="M8.5 12.5l2.5 2.5 4.5-5" />
          </svg>
        {/if}
      </span>

      <span class="tool-call-meta min-w-0 flex-1">
        {#if compact}
          <span class="tool-call-name-row is-inline">
            <span class="tool-call-name">{title}</span>
            {#if recorded}
              <span class="tool-record-dot" title="录制中捕获" aria-hidden="true"></span>
            {/if}
            <span class="tool-call-id" title={name}>{name}</span>
            {#if headline && headline !== title}
              <span class="tool-call-sep" aria-hidden="true">·</span>
              <span class="tool-call-headline truncate" title={headlineTitle}>{headline}</span>
            {/if}
          </span>
          {#if peek}
            <span class="tool-call-peek truncate" title={peek}>{peek}</span>
          {/if}
        {:else}
          <span class="tool-call-name-row">
            <span class="tool-call-name">{title}</span>
            {#if recorded}
              <span class="tool-record-dot" title="录制中捕获" aria-hidden="true"></span>
            {/if}
            <span class="tool-call-id" title={name}>{name}</span>
            <span class="format-badge" data-testid="tool-format">{format.label}</span>
            {#if !done && elapsedMs > 0}
              <span class="tool-call-elapsed" data-testid="tool-elapsed">{formatElapsed(elapsedMs / 1000)}</span>
            {/if}
          </span>
          <span class="tool-call-headline truncate" title={headlineTitle}>{headline}</span>
          {#if subline}
            <span class="tool-call-sub truncate">{subline}</span>
          {/if}
        {/if}
      </span>

    </button>

    {#if format.copyable && (detail || summary) && !compact}
      <button
        type="button"
        class="tool-call-copy"
        aria-label="复制输出"
        title={copied ? "已复制" : "复制输出"}
        onclick={copyDetail}
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
    {/if}
  </div>

  {#if !done}
    <div class="tool-call-progress" aria-hidden="true" data-testid="tool-progress">
      <i class="is-indeterminate"></i>
    </div>
    {#if detail}
      <!-- ADR-037: live stdout/stderr while the command runs. 默认只显示单行
           实时尾巴（高度稳定，不抢占聊天滚动）；点击展开看完整输出。 -->
      {#if open}
        <div class="tool-shell tool-shell-live" data-testid="tool-live-output">
          <pre class="tool-shell-body" bind:this={liveBodyEl}>{detail}</pre>
        </div>
      {:else}
        <div class="tool-live-tail" data-testid="tool-live-tail" title={liveTail}>
          <span class="tool-live-tail-mark" aria-hidden="true">▸</span>
          <span class="tool-live-tail-text truncate">{liveTail}</span>
        </div>
      {/if}
    {/if}
  {/if}

  {#if open && detail}
    {#if listing}
      <div class="tool-listing" data-testid="tool-listing">
        {#if listing.root}
          <div class="tool-listing-root">{listing.root}</div>
        {/if}
        <ul class="tool-listing-list">
          {#each listingEntries as entry, i (`${entry.kind}-${entry.name}-${i}`)}
            <li class="tool-listing-row" data-kind={entry.kind}>
              <span class="tool-listing-kind" aria-hidden="true">
                {#if entry.kind === "dir"}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
                    <path d="M3 7h6l2 2h10v10H3z" />
                  </svg>
                {:else if entry.kind === "file"}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
                    <path d="M14 3H6a2 2 0 00-2 2v14a2 2 0 002 2h12a2 2 0 002-2V9z" />
                    <path d="M14 3v6h6" />
                  </svg>
                {:else}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
                    <circle cx="12" cy="12" r="3" />
                  </svg>
                {/if}
              </span>
              <span class="tool-listing-name">{entry.name}</span>
              {#if entry.size}
                <span class="tool-listing-size">{entry.size}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {:else if format.kind === "shell" || format.kind === "diff"}
      <div class="tool-shell" data-testid="tool-shell" data-kind={format.kind}>
        <pre class="tool-shell-body">{detail}</pre>
      </div>
    {:else}
      <pre
        class="tool-call-detail"
        class:is-code={format.kind === "code" || format.kind === "json"}
        class:is-path={format.kind === "path"}
        class:is-link={format.kind === "link"}>{detail}</pre>
    {/if}
  {/if}
</div>
