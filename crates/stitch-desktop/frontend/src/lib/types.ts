/** Resolved appearance applied to `data-theme`. */
export type Theme = "light" | "dark";
/** Stored preference; default `system` follows OS. */
export type ThemePreference = Theme | "system";

export interface LlmProfileSnapshot {
  id: string;
  label: string;
  provider: string;
  api_base: string;
  api_key_masked: string;
  api_key_set: boolean;
  model: string;
  /** Whether the model accepts image input (gate for the paste entry). */
  supports_images?: boolean;
}

/**
 * Whether a model id accepts image input — mirrors the Rust
 * `model_supports_vision` in tokens.rs; keep both in sync.
 */
export function modelSupportsVision(model: string): boolean {
  const m = model.toLowerCase();
  if (m.includes("deepseek")) return false;
  return (
    m.includes("gpt-4o") ||
    m.includes("gpt-4") ||
    m.includes("claude") ||
    m.includes("kimi") ||
    m.includes("moonshot") ||
    m.includes("qwen") ||
    m.includes("glm-4v") ||
    m.includes("gemini") ||
    m.includes("vision")
  );
}

export interface McpProfileSnapshot {
  id: string;
  label: string;
  api_base: string;
  api_token_masked: string;
  api_token_set: boolean;
}

export interface McpServerSnapshot {
  id: string;
  label: string;
  transport: string;
  enabled: boolean;
  command?: string | null;
  args: string[];
  env?: Record<string, string>;
  cwd?: string | null;
  url?: string | null;
  auth_set: boolean;
  auth_masked: string;
}

export interface ConfigSnapshot {
  api_base?: string;
  api_token_masked?: string;
  api_token_set: boolean;
  active_mcp_id?: string | null;
  mcp_profiles?: McpProfileSnapshot[];
  mcp_servers?: McpServerSnapshot[];
  llm_provider: string;
  llm_api_base: string;
  llm_api_key_masked: string;
  llm_api_key_set: boolean;
  llm_model: string;
  active_profile_id?: string | null;
  llm_profiles?: LlmProfileSnapshot[];
  max_iterations: number;
  /** `personal` | `explore` — default explore (ADR-033). */
  sediment_visibility?: string;
  /** Local vision describe layer (DeepSeek + Ollama qwen3-vl as the eyes). */
  local_vision?: LocalVisionSnapshot;
}

export interface Announcement {
  id: string;
  title: string;
  body: string;
  url?: string;
}

export interface UpdateStatus {
  available: boolean;
  current_version: string;
  latest_version?: string;
  release_notes?: string;
  download_url?: string;
}

export interface MembershipSnapshot {
  token_set: boolean;
  is_member: boolean;
  status: string;
  plan?: string | null;
  pricing_url: string;
}

export type PlanStepStatus = "pending" | "in_progress" | "done" | "skipped" | "failed";

export interface PlanStep {
  description: string;
  status: PlanStepStatus;
}

export interface PlanData {
  title?: string | null;
  steps: PlanStep[];
}

export interface LocalVisionSnapshot {
  enabled: boolean;
  api_base: string;
  model: string;
  timeout_secs: number;
}

export interface HistoryMessage {
  role: "user" | "assistant";
  content: string;
  /** Image data URLs on a user message (kept in memory only, not persisted). */
  images?: string[];
}

/** A remembered allow rule sent with a confirm response（记住此规则）. */
export type RememberRule = {
  tool: string;
  scope: "path" | "command";
  value: string;
};

/** A persisted allow rule (settings UI list; same shape as RememberRule). */
export interface AllowRule {
  tool: string;
  scope: string;
  value: string;
}

export interface SuiteSummary {
  id: string;
  title: string;
  description?: string | null;
  tags?: string[] | null;
  step_count: number;
  updated_at?: string | null;
}

export interface AgentSummary {
  id: string;
  name: string;
  task_suite_id: string;
  task_suite_title?: string | null;
  trigger_mode: string;
  file_write_permission: string;
  step_strategy: string;
  failure_policy: string;
  updated_at?: string | null;
}

export type ChatItem =
  | {
      id: string;
      type: "message";
      role: "user" | "assistant";
      content: string;
      /** Image data URLs on a user message (in-memory only; stripped before
       * localStorage persist to stay inside the 5MB quota). */
      images?: string[];
      /** Set when images were stripped at persist time — the bubble shows a
       * placeholder after a restart (the Rust session still has the images). */
      imagesStripped?: boolean;
      error?: boolean;
      /**
       * Stopped by user — this assistant bubble and its triggering user turn
       * are excluded from LLM history (avoid consecutive-user resume).
       */
      stopped?: boolean;
      /** Turn ended at the iteration budget; offer「继续执行」. */
      hitCap?: boolean;
    }
  | {
      id: string;
      type: "tool";
      name: string;
      done: boolean;
      error: boolean;
      summary: string;
      detail: string;
      expanded?: boolean;
      /** 所在工具组展开状态（存组内首工具上——虚拟化/视图重建不还原收起）。 */
      groupExpanded?: boolean;
      /** Wall-clock start of this tool (ms) for live elapsed display. */
      startedAt?: number;
      /** Started while skill-recording mode was active. */
      recorded?: boolean;
      /** Per-tool benchmark metrics (duration_ms, …) from ToolResult. */
      metrics?: Record<string, number>;
    }
  | {
      id: string;
      type: "plan";
      /** IPC plan approval id (respond_plan) */
      planId?: string;
      title: string;
      steps: PlanStep[];
      phase: "proposed" | "approved" | "rejected";
    }
  | {
      id: string;
      type: "sediment";
      title: string;
      content: string;
      status: "idle" | "saving" | "saved" | "error";
      errorText?: string;
      promptId?: string;
    };

/** Quiet draft after Done (ADR-036); not auto-inserted into the stream. */
export interface SedimentCandidate {
  title: string;
  content: string;
  updatedAt: number;
}

export interface Session {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: ChatItem[];
  /** Named LLM profile id from config; missing on legacy sessions. */
  llmProfileId?: string;
  /** Model id for this session (may differ from profile default). */
  llmModel?: string;
  /** Bound project directory (absolute); missing on legacy sessions. */
  workDirPath?: string;
  /** Prefill after successful Done;「保存」uses this without rebuilding. */
  sedimentCandidate?: SedimentCandidate;
}

/** Named project folder shown in the sidebar workspace list. */
export interface WorkspaceEntry {
  id: string;
  label: string;
  path: string;
  lastUsedAt: number;
}

export interface WorkspacesStore {
  currentId: string | null;
  items: WorkspaceEntry[];
}

export interface SessionsStore {
  current: string | null;
  sessions: Record<string, Session>;
}

/** Per-tier context breakdown (Rust layers::LayerStats). */
export type LayerStats = {
  hot_msgs: number;
  warm_entries: number;
  cold_entries: number;
  hot_tokens: number;
  warm_tokens: number;
  cold_tokens: number;
  total_tokens: number;
  limit: number;
};

export type AgentEvent =
  | { type: "token"; text: string }
  | {
      /** Thinking-process tokens (CLI /think on; desktop UI intentionally ignores). */
      type: "thinking";
      text: string;
    }
  | { type: "tool_start"; name: string; call_id?: string }
  | { type: "tool_output"; name: string; call_id?: string; text: string }
  | {
      type: "tool_done";
      name: string;
      call_id?: string;
      success: boolean;
      summary: string;
      /** Per-tool benchmark metrics (duration_ms, …) from ToolResult. */
      metrics?: Record<string, number>;
    }
  | {
      type: "usage";
      iteration: number;
      input_tokens: number;
      output_tokens: number;
      context_tokens: number;
      context_limit: number;
      compacted: boolean;
      /** Three-tier context breakdown; absent when layering is off. */
      layers?: LayerStats | null;
    }
  | {
      type: "done";
      response: string;
      iterations: number;
      input_tokens?: number;
      output_tokens?: number;
      context_tokens?: number;
      context_limit?: number;
      hit_iteration_cap?: boolean;
    }
  | { type: "confirm_request"; id: string; tool: string; message: string }
  | { type: "cancelled"; message: string }
  | { type: "error"; message: string }
  | { type: "notice"; message: string }
  | { type: "plan_proposed"; id: string; plan: PlanData }
  | { type: "plan_approved" }
  | { type: "plan_rejected" }
  | { type: "plan_step_start"; index: number; description: string }
  | { type: "plan_step_done"; index: number; description: string }
  | { type: "subagent_start"; name: string; description: string; tools?: string[] }
  | { type: "subagent_done"; name: string; success: boolean; summary: string };

/** OpenAI-compatible provider presets (id → display + default base + model hints). */
export type ProviderPreset = {
  label: string;
  api_base: string;
  /** Suggested model ids (optional quick-picks; field stays free text). */
  models: string[];
};

/** Order used in the settings provider dropdown. */
export const PROVIDER_ORDER = [
  "custom",
  "deepseek",
  "openai",
  "zhipu",
  "kimi",
  "minimax",
  "ollama",
  "anthropic",
] as const;

export const PROVIDER_PRESETS: Record<string, ProviderPreset> = {
  custom: {
    label: "自定义",
    api_base: "",
    models: [],
  },
  deepseek: {
    label: "DeepSeek",
    api_base: "https://api.deepseek.com",
    models: ["deepseek-v4-flash", "deepseek-v4-pro"],
  },
  openai: {
    label: "OpenAI",
    api_base: "https://api.openai.com/v1",
    models: ["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "o1-mini"],
  },
  zhipu: {
    label: "智谱",
    api_base: "https://open.bigmodel.cn/api/paas/v4",
    models: ["glm-4-flash", "glm-4-plus", "glm-4.5"],
  },
  kimi: {
    label: "Kimi",
    api_base: "https://api.moonshot.cn/v1",
    models: ["kimi-k2.5", "moonshot-v1-auto", "moonshot-v1-8k"],
  },
  minimax: {
    label: "MiniMax",
    api_base: "https://api.minimax.chat/v1",
    models: ["MiniMax-M2.5", "abab6.5s-chat"],
  },
  ollama: {
    label: "Ollama 本地",
    api_base: "http://127.0.0.1:11434/v1",
    models: ["llama3.2", "qwen2.5", "deepseek-r1"],
  },
  anthropic: {
    label: "Anthropic",
    api_base: "https://api.anthropic.com/v1",
    models: [
      "claude-sonnet-4-20250514",
      "claude-3.5-sonnet",
      "claude-3-opus",
      "claude-3-haiku",
    ],
  },
};

export function providerPresetLabel(provider: string): string {
  return PROVIDER_PRESETS[provider]?.label || provider || "自定义";
}

export const LOCAL_API_BASE = "http://127.0.0.1:8090";
export const PROD_API_BASE = "https://www.promptstdio.com";
export const SESSIONS_KEY = "stitch-sessions";
export const THEME_KEY = "stitch-theme";
export const SIDEBAR_KEY = "stitch-sidebar-collapsed";
/** 侧栏宽度 px（可拖分割线调整，clamp 200–480）。 */
export const SIDEBAR_WIDTH_KEY = "stitch-sidebar-width";
export const RECENT_DIRS_KEY = "stitch-recent-dirs";
export const WORKSPACES_KEY = "stitch-workspaces";
/** Per-workspace sidebar collapse: `{ [workspaceId]: true }` means collapsed. */
export const WORKSPACE_COLLAPSE_KEY = "stitch-workspace-collapse";
export const PLAN_MODE_KEY = "stitch-plan-mode";
export const AUTO_CONTINUE_KEY = "stitch-auto-continue";
/** Sidebar partition last active tab: `sessions` | `library`. */
export const SIDEBAR_TAB_KEY = "stitch-sidebar-tab";
/** Library panel last active sub-tab: `scenes` | `suites` | `agents` | `skills`. */
export const LIBRARY_KIND_KEY = "stitch-library-kind";
/** After first-run save, nudge once to pick a work directory. */
export const WORKDIR_NUDGE_KEY = "stitch-workdir-nudge";

/** Default label from a path: last path segment. */
export function workspaceLabelFromPath(path: string): string {
  const normalized = (path || "").replace(/^\\\\\?\\/, "").replace(/\\/g, "/");
  const parts = normalized.split("/").filter(Boolean);
  return parts[parts.length - 1] || path || "工作区";
}