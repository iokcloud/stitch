<script lang="ts">
  import type { ChatItem } from "../types";
  import { toolLabel } from "../output-format";
  import ToolStatus from "./ToolStatus.svelte";

  interface Props {
    tools: ChatItem[];
    /** 组展开状态（受控——由 ChatView 持有，虚拟化重建不还原收起）。 */
    open: boolean;
    /** 组展开状态写回（ChatView 存会话级 map）。 */
    onToggleGroup?: (open: boolean) => void;
    /** 工具展开状态写回（透传给内部 ToolStatus；虚拟化重建不还原展开）。 */
    onToggleTool?: (toolId: string, open: boolean) => void;
  }

  let { tools, open, onToggleGroup = undefined, onToggleTool = undefined }: Props = $props();

  const count = $derived(tools.length);
  const labels = $derived(
    tools
      .map((t) => (t.type === "tool" ? toolLabel(t.name) : ""))
      .filter(Boolean),
  );
  const peek = $derived.by(() => {
    const uniq: string[] = [];
    for (const l of labels) {
      if (!uniq.includes(l)) uniq.push(l);
      if (uniq.length >= 4) break;
    }
    const joined = uniq.join(" · ");
    return labels.length > uniq.length ? `${joined}…` : joined;
  });
</script>

<div class="tool-group" data-testid="tool-group" data-count={count}>
  <button
    type="button"
    class="tool-group-head"
    aria-expanded={open}
    aria-label={open ? "收起" : "展开"}
    title={open ? "收起" : "展开"}
    data-testid="tool-group-toggle"
    onclick={() => onToggleGroup?.(!open)}
  >
    <span class="tool-call-chevron" class:open aria-hidden="true">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M9 6l6 6-6 6" />
      </svg>
    </span>
    <span class="tool-call-state" aria-hidden="true">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
        <circle cx="12" cy="12" r="9" />
        <path d="M8.5 12.5l2.5 2.5 4.5-5" />
      </svg>
    </span>
    <span class="tool-group-meta min-w-0 flex-1">
      <span class="tool-group-title">已执行 {count} 步</span>
      {#if peek}
        <span class="tool-group-peek truncate" title={peek}>{peek}</span>
      {/if}
    </span>
  </button>

  {#if open}
    <div class="tool-group-body" data-testid="tool-group-body">
      {#each tools as tool, i (tool.id)}
        {#if tool.type === "tool"}
          <ToolStatus
            name={tool.name}
            done={tool.done}
            error={tool.error}
            summary={tool.summary}
            detail={tool.detail}
            expanded={!!tool.expanded}
            recorded={!!tool.recorded}
            stacked={i > 0}
            metrics={tool.metrics}
            onToggle={(o) => onToggleTool?.(tool.id, o)}
          />
        {/if}
      {/each}
    </div>
  {/if}
</div>
