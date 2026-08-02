/** Parse agent confirm_request messages into structured UI fields. */

export type ConfirmKind =
  | "run_command"
  | "write_file"
  | "edit_file"
  | "delete_path"
  | "read_file"
  | "other";

export type ParsedConfirm = {
  kind: ConfirmKind;
  /** Short L1 title */
  title: string;
  /** One-line risk / context */
  hint: string;
  /** Primary payload (command or path) */
  payload: string;
  /** Extra line (e.g. edit count) */
  meta: string;
  /** Visual density: shell vs path */
  presentation: "shell" | "path" | "plain";
};

const TOOL_KIND: Record<string, ConfirmKind> = {
  run_command: "run_command",
  write_file: "write_file",
  edit_file: "edit_file",
  delete_path: "delete_path",
  // Outside-workspace reads（工作区外按范围授权）
  read_file: "read_file",
  list_directory: "read_file",
  search_code: "read_file",
};

export function parseConfirm(tool: string, message: string): ParsedConfirm {
  const kind = TOOL_KIND[tool] ?? "other";
  const raw = (message || "").replace(/\nAllow\?\s*$/i, "").trim();

  if (kind === "run_command") {
    const cmd = raw.replace(/^Run command:\s*/i, "").trim() || raw;
    return {
      kind,
      title: "运行命令",
      hint: "将在当前工作目录执行",
      payload: cmd,
      meta: "",
      presentation: "shell",
    };
  }

  if (kind === "write_file") {
    const path = raw.replace(/^Write to file:\s*/i, "").trim() || raw;
    return {
      kind,
      title: "写入文件",
      hint: "已有文件会被覆盖",
      payload: path,
      meta: "",
      presentation: "path",
    };
  }

  if (kind === "edit_file") {
    const m = raw.match(/^Edit file:\s*(.+?)(?:\s*\((\d+)\s*change)/i);
    const path = m?.[1]?.trim() || raw.replace(/^Edit file:\s*/i, "").trim();
    const count = m?.[2];
    return {
      kind,
      title: "编辑文件",
      hint: "仅替换匹配片段",
      payload: path,
      meta: count ? `${count} 处修改` : "",
      presentation: "path",
    };
  }

  if (kind === "read_file") {
    const path = raw.replace(/^Read outside workspace:\s*/i, "").trim() || raw;
    return {
      kind,
      title: "读取文件",
      hint: "工作区外路径",
      payload: path,
      meta: "",
      presentation: "path",
    };
  }

  if (kind === "delete_path") {
    const path = raw
      .replace(/^Delete:\s*/i, "")
      .replace(/\nThis is irreversible\.?/i, "")
      .trim();
    return {
      kind,
      title: "删除路径",
      hint: "不可恢复",
      payload: path || raw,
      meta: "",
      presentation: "path",
    };
  }

  return {
    kind: "other",
    title: tool || "需要确认",
    hint: "可能改动文件或运行命令",
    payload: raw,
    meta: "",
    presentation: "plain",
  };
}
