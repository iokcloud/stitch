<script lang="ts">
  import { tick } from "svelte";
  import { chatFindOpen } from "../stores/palette";

  interface Props {
    containerEl: HTMLDivElement | undefined;
    sessionKey: string | null;
  }

  let { containerEl, sessionKey }: Props = $props();

  let query = $state("");
  let total = $state(0);
  /** 1-based current match for display; 0 when there is none. */
  let active = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();

  let ranges: Range[] = [];
  // CSS Custom Highlight API (Chromium/WebView2) — inline find-style marks
  // without touching the rendered Markdown DOM.
  const cssHighlights: Map<string, unknown> | undefined =
    typeof CSS !== "undefined" && "highlights" in CSS
      ? (CSS as unknown as { highlights: Map<string, unknown> }).highlights
      : undefined;
  const HighlightCtor: (new (...r: Range[]) => unknown) | undefined =
    typeof window !== "undefined" && "Highlight" in window
      ? (window as unknown as { Highlight: new (...r: Range[]) => unknown })
          .Highlight
      : undefined;
  const supportsHL = !!(cssHighlights && HighlightCtor);

  function clearHL() {
    cssHighlights?.delete("stitch-find");
    cssHighlights?.delete("stitch-find-current");
    ranges = [];
  }

  function recompute() {
    clearHL();
    total = 0;
    active = 0;
    const q = query.trim().toLowerCase();
    if (!q || !containerEl) return;
    const walker = document.createTreeWalker(containerEl, NodeFilter.SHOW_TEXT, {
      acceptNode(node) {
        const el = node.parentElement;
        if (!el?.closest(".msg-body")) return NodeFilter.FILTER_REJECT;
        return node.textContent?.toLowerCase().includes(q)
          ? NodeFilter.FILTER_ACCEPT
          : NodeFilter.FILTER_REJECT;
      },
    });
    const found: Range[] = [];
    let node = walker.nextNode();
    while (node) {
      const text = node.textContent ?? "";
      const lower = text.toLowerCase();
      let from = 0;
      let idx = lower.indexOf(q, from);
      while (idx >= 0) {
        const r = new Range();
        r.setStart(node, idx);
        r.setEnd(node, idx + q.length);
        found.push(r);
        from = idx + q.length;
        idx = lower.indexOf(q, from);
      }
      node = walker.nextNode();
    }
    ranges = found;
    total = found.length;
    if (supportsHL && found.length) {
      cssHighlights.set("stitch-find", new HighlightCtor(...found));
    }
    if (total > 0) setActive(1);
  }

  function setActive(n: number) {
    if (!ranges.length) {
      active = 0;
      return;
    }
    const clamped = ((n - 1) % ranges.length + ranges.length) % ranges.length + 1;
    active = clamped;
    const r = ranges[clamped - 1];
    if (supportsHL) {
      cssHighlights.set("stitch-find-current", new HighlightCtor(r));
    }
    const el =
      r.startContainer.parentElement?.closest(".chat-item") ??
      r.startContainer.parentElement;
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }

  function close() {
    chatFindOpen.set(false);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      setActive(active + (e.shiftKey ? -1 : 1));
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  }

  // Open → focus + scan; close → clear marks.
  $effect(() => {
    if ($chatFindOpen) {
      void tick().then(() => {
        inputEl?.focus();
        inputEl?.select();
        recompute();
      });
    } else {
      clearHL();
      total = 0;
      active = 0;
    }
  });

  // Session switch resets the query.
  let lastSessionKey = $state<string | null>(null);
  $effect(() => {
    if (sessionKey === lastSessionKey) return;
    lastSessionKey = sessionKey;
    query = "";
    if ($chatFindOpen) recompute();
  });

  // Keep matches fresh while messages stream in (debounced).
  $effect(() => {
    if (!$chatFindOpen || !containerEl) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const obs = new MutationObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        const keep = active;
        recompute();
        if (keep > 0 && total > 0) setActive(Math.min(keep, total));
      }, 250);
    });
    obs.observe(containerEl, {
      childList: true,
      characterData: true,
      subtree: true,
    });
    return () => {
      clearTimeout(timer);
      obs.disconnect();
    };
  });
</script>

{#if $chatFindOpen}
  <div class="find-bar" data-testid="find-bar">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <circle cx="11" cy="11" r="7" />
      <path d="M21 21l-4.3-4.3" />
    </svg>
    <input
      bind:this={inputEl}
      bind:value={query}
      oninput={() => recompute()}
      onkeydown={onKeydown}
      class="find-input"
      type="search"
      placeholder="在会话中查找"
      aria-label="在会话中查找"
      data-testid="find-input"
    />
    {#if query.trim()}
      <span class="find-count" data-testid="find-count">{active}/{total}</span>
    {/if}
    <button
      type="button"
      class="find-btn"
      aria-label="上一个"
      title="上一个（Shift+Enter）"
      data-testid="find-prev"
      disabled={total === 0}
      onclick={() => setActive(active - 1)}
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path d="M18 15l-6-6-6 6" />
      </svg>
    </button>
    <button
      type="button"
      class="find-btn"
      aria-label="下一个"
      title="下一个（Enter）"
      data-testid="find-next"
      disabled={total === 0}
      onclick={() => setActive(active + 1)}
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>
    <button
      type="button"
      class="find-btn"
      aria-label="关闭查找"
      title="关闭（Esc）"
      data-testid="find-close"
      onclick={close}
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path d="M18 6L6 18M6 6l12 12" />
      </svg>
    </button>
  </div>
{/if}
