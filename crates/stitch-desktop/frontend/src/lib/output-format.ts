/** Detect display format for tool / assistant output blocks (L1 labels). */

export type OutputFormatKind =
  | "json"
  | "code"
  | "path"
  | "link"
  | "markdown"
  | "text"
  | "listing"
  | "shell"
  | "diff";

export type OutputFormat = {
  kind: OutputFormatKind;
  /** Short badge text shown in UI */
  label: string;
  /** Whether a one-click copy control should be offered */
  copyable: boolean;
};

/** Human-readable tool titles (L1). */
export const TOOL_LABELS: Record<string, string> = {
  list_directory: "列出目录",
  read_file: "读取文件",
  write_file: "写入文件",
  edit_file: "编辑文件",
  run_command: "运行命令",
  search_code: "搜索代码",
  git_status: "Git 状态",
  git_diff: "Git 差异",
  web_fetch: "拉取网页",
  find_path: "查找路径",
  create_directory: "创建目录",
  delete_path: "删除路径",
  copy_path: "复制路径",
  undo_last_edit: "撤销编辑",
  redo_last_edit: "重做编辑",
  desktop_screenshot: "截图",
  desktop_click: "点击",
  desktop_type: "打字",
  desktop_key: "按键",
  desktop_scroll: "滚动",
  desktop_hover: "悬停",
  desktop_window_list: "窗口列表",
  desktop_window_action: "窗口操作",
  desktop_browser: "浏览器",
  desktop_app_launch: "启动应用",
};

export function toolLabel(name: string): string {
  return TOOL_LABELS[name] || name;
}

export function detectOutputFormat(raw: string, hint?: { toolName?: string }): OutputFormat {
  const tool = hint?.toolName || "";
  const s = (raw || "").trim();

  if (tool === "run_command") {
    // Process noise — expand to read; no one-click copy (ADR-025 / DESIGN).
    return { kind: "shell", label: "终端", copyable: false };
  }
  if (tool === "list_directory" || /^\[(dir|file)\]/m.test(s)) {
    return { kind: "listing", label: "目录", copyable: !!s };
  }
  if (tool === "write_file" || tool === "edit_file" || tool === "create_directory") {
    return { kind: "path", label: "文件", copyable: !!s };
  }
  if (tool === "read_file") {
    const codeish =
      /^(def |fn |function |class |import |package |#include |SELECT |const |let |var |<!DOCTYPE|<html)/m.test(
        s,
      ) ||
      (s.includes("\n") && /[{;}]$/m.test(s) && s.split("\n").length >= 3);
    if (codeish) return { kind: "code", label: "代码", copyable: true };
    return { kind: "text", label: "文件", copyable: !!s };
  }
  if (tool === "search_code" || tool === "find_path") {
    return { kind: "path", label: "结果", copyable: !!s };
  }
  if (tool === "git_status" || tool === "git_diff") {
    return { kind: tool === "git_diff" ? "diff" : "shell", label: "Git", copyable: false };
  }
  if (tool === "delete_path" || tool === "copy_path") {
    return { kind: "path", label: "路径", copyable: !!s };
  }

  if (!s) return { kind: "text", label: "文本", copyable: false };

  if (/^https?:\/\/\S+$/i.test(s) || /^www\.\S+$/i.test(s)) {
    return { kind: "link", label: "链接", copyable: true };
  }

  if (
    /^[A-Za-z]:[\\/]/.test(s) ||
    /^\\\\/.test(s) ||
    /^(\.\/|\.\.\/|~\/|\/)[\w./\\-]+$/.test(s.split("\n")[0] || "")
  ) {
    const lines = s.split("\n").filter(Boolean);
    if (lines.length <= 3 || lines.every((l) => /[\\/]/.test(l) || /\.\w{1,8}$/.test(l))) {
      return { kind: "path", label: "路径", copyable: true };
    }
  }

  if ((s.startsWith("{") && s.endsWith("}")) || (s.startsWith("[") && s.endsWith("]"))) {
    try {
      JSON.parse(s);
      return { kind: "json", label: "JSON", copyable: true };
    } catch {
      /* fall through */
    }
  }

  const codeish =
    /^(def |fn |function |class |import |package |#include |SELECT |const |let |var )/m.test(s) ||
    (s.includes("\n") && /[{;}]$/m.test(s) && s.split("\n").length >= 3);
  if (codeish) return { kind: "code", label: "代码", copyable: true };

  if (/^```/.test(s) || /^#{1,6}\s/m.test(s)) {
    return { kind: "markdown", label: "Markdown", copyable: true };
  }

  return { kind: "text", label: "文本", copyable: s.length > 0 };
}

export type ListingEntry = { kind: "dir" | "file" | "other"; name: string; size: string };

/** Parse list_directory style lines: `[dir] name` / `[file] name (1.2 KB)`. */
export function parseListing(detail: string): { root: string; entries: ListingEntry[] } | null {
  let lines = (detail || "").split("\n");
  if (!lines.length) return null;
  const hasMarkers = lines.some((l) => /^\[(dir|file)\]/.test(l.trim()));
  if (!hasMarkers) return null;

  let root = "";
  const entries: ListingEntry[] = [];
  for (const line of lines) {
    const t = line.trimEnd();
    if (!t) continue;
    const m = t.match(/^\[(dir|file)\]\s+(\S+)(?:\s+(\(.*\)))?\s*$/);
    if (m) {
      entries.push({
        kind: m[1] as "dir" | "file",
        name: m[2],
        size: (m[3] || "").trim(),
      });
      continue;
    }
    if (!entries.length && /\/\s*$/.test(t)) {
      root = t
        .replace(/^\\\\\?\\/, "")
        .replace(/^\/\/\?\//, "")
        .replace(/\\/g, "/");
      continue;
    }
    if (/truncated/.test(t) || /empty/.test(t)) {
      entries.push({ kind: "other", name: t, size: "" });
    }
  }
  if (!entries.length && !root) return null;
  return { root, entries };
}

export function formatElapsed(sec: number): string {
  if (sec < 0 || !Number.isFinite(sec)) return "0s";
  const s = Math.floor(sec);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  return `${m}m ${r.toString().padStart(2, "0")}s`;
}
