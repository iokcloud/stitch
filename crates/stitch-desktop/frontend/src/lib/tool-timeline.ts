import type { ChatItem } from "./types";

/**
 * Quiet process tools — consecutive successes fold into one overview chip.
 * Writes / deletes / errors stay as individual cards.
 */
const PROCESS_TOOLS = new Set([
  "run_command",
  "git_status",
  "git_diff",
  "list_directory",
  "search_code",
  "find_path",
  "web_fetch",
  "read_file",
]);

export function isFoldableProcessTool(item: ChatItem): boolean {
  return (
    item.type === "tool" &&
    item.done &&
    !item.error &&
    PROCESS_TOOLS.has(item.name)
  );
}

export type TimelineBlock =
  | { kind: "single"; item: ChatItem; index: number }
  | { kind: "tool_group"; items: ChatItem[]; startIndex: number };

/** Collapse consecutive foldable process tools (2+) into one overview block. */
export function groupTimeline(items: ChatItem[]): TimelineBlock[] {
  const out: TimelineBlock[] = [];
  let i = 0;
  while (i < items.length) {
    const item = items[i];
    if (isFoldableProcessTool(item)) {
      let j = i + 1;
      while (j < items.length && isFoldableProcessTool(items[j])) j++;
      if (j - i >= 2) {
        out.push({ kind: "tool_group", items: items.slice(i, j), startIndex: i });
        i = j;
        continue;
      }
    }
    out.push({ kind: "single", item, index: i });
    i++;
  }
  return out;
}
