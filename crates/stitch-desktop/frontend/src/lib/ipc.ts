import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
    AgentEvent,
    AgentSummary,
    AllowRule,
    ConfigSnapshot,
    HistoryMessage,
    RememberRule,
    SuiteSummary,
    MembershipSnapshot,
    UpdateStatus,
} from "./types";

export async function getConfig(): Promise<ConfigSnapshot> {
    return invoke("get_config");
}

export async function saveConfig(updates: Record<string, string>): Promise<ConfigSnapshot> {
    return invoke("save_config", { updates });
}

export type TestConnectionOverrides = {
    llm_api_key?: string;
    llm_api_base?: string;
    llm_model?: string;
    profile_id?: string;
};

export async function testConnection(overrides?: TestConnectionOverrides): Promise<boolean> {
    return invoke("test_connection", { args: overrides ?? null });
}

export type UpsertLlmProfileArgs = {
    id: string;
    label?: string;
    provider: string;
    api_base: string;
    api_key?: string;
    model: string;
};

export async function upsertLlmProfile(args: UpsertLlmProfileArgs): Promise<ConfigSnapshot> {
    return invoke("upsert_llm_profile", { args });
}

export async function deleteLlmProfile(id: string): Promise<ConfigSnapshot> {
    return invoke("delete_llm_profile", { id });
}

export async function setActiveLlmProfile(id: string): Promise<ConfigSnapshot> {
    return invoke("set_active_llm_profile", { id });
}

export type UpsertMcpProfileArgs = {
    id: string;
    label?: string;
    api_base: string;
    api_token?: string;
};

export async function upsertMcpProfile(args: UpsertMcpProfileArgs): Promise<ConfigSnapshot> {
    return invoke("upsert_mcp_profile", { args });
}

export async function deleteMcpProfile(id: string): Promise<ConfigSnapshot> {
    return invoke("delete_mcp_profile", { id });
}

export async function setActiveMcpProfile(id: string): Promise<ConfigSnapshot> {
    return invoke("set_active_mcp_profile", { id });
}

export async function clearMcpProfileToken(id: string): Promise<ConfigSnapshot> {
    return invoke("clear_mcp_profile_token", { id });
}

export type UpsertMcpServerArgs = {
    id: string;
    label?: string;
    transport: string;
    enabled?: boolean;
    command?: string;
    args?: string[];
    env?: Record<string, string>;
    cwd?: string;
    url?: string;
    auth_token?: string;
    headers?: Record<string, string>;
};

export async function upsertMcpServer(args: UpsertMcpServerArgs): Promise<ConfigSnapshot> {
    return invoke("upsert_mcp_server", { args });
}

/** Import Cursor / Claude Desktop `mcpServers` JSON (merge by id). */
export async function importMcpServers(json: string, replace = false): Promise<ConfigSnapshot> {
    return invoke("import_mcp_servers", { args: { json, replace } });
}

/** Seed PromptStdio HTTP MCP (enabled=false). Idempotent if id already exists. */
export async function addPromptstdioMcpPreset(): Promise<ConfigSnapshot> {
    return invoke("add_promptstdio_mcp_preset");
}

export async function deleteMcpServer(id: string): Promise<ConfigSnapshot> {
    return invoke("delete_mcp_server", { id });
}

export async function setMcpServerEnabled(id: string, enabled: boolean): Promise<ConfigSnapshot> {
    return invoke("set_mcp_server_enabled", { id, enabled });
}

export async function testMcpServer(id: string): Promise<string> {
    return invoke("test_mcp_server", { id });
}

export interface SkillSummary {
    slug: string;
    title: string;
    description: string;
    version: string;
    install_phrase: string;
    sync_phrase: string;
    price_display?: string | null;
}

export async function listSkills(): Promise<SkillSummary[]> {
    return invoke("list_skills");
}

export interface LocalSkillRow {
    slug: string;
    title: string;
    description: string;
    rel_path: string;
    /** `"workspace"` (project) or `"user"` (home install). */
    scope: "workspace" | "user" | string;
}

/** Skills from work dir + user-global (`~/.agents/skills` · `~/.cursor/skills`). */
export async function listLocalSkills(): Promise<LocalSkillRow[]> {
    return invoke("list_local_skills");
}

export type TestPromptstdioOverrides = {
    profile_id?: string;
    api_token?: string;
    api_base?: string;
};

export async function testPromptstdio(overrides?: TestPromptstdioOverrides): Promise<string> {
    return invoke("test_promptstdio", { args: overrides ?? null });
}

export type SendMessageOpts = {
    profileId?: string | null;
    model?: string | null;
    /** Frontend chat session id — keeps tool_calls history in Rust across turns. */
    chatSessionId?: string | null;
    /** Retry after error: resume cached Session without appending the user turn. */
    resume?: boolean;
    /** Regenerate / edit-resend: rewind Rust session back to this user turn. */
    rewindToUser?: string | null;
    /** Also drop the target user turn (edit-resend replaces it). */
    rewindDrop?: boolean;
    /** Skill recording mode — tools started while true get the recorded chip. */
    recording?: boolean;
    /** Image data URLs attached to this user message (vision models only). */
    images?: string[];
};

export async function sendMessage(
    message: string,
    history: HistoryMessage[] = [],
    planMode = false,
    opts: SendMessageOpts = {},
): Promise<void> {
    return invoke("send_message", {
        message,
        images: opts.images?.length ? opts.images : null,
        history,
        planMode,
        profileId: opts.profileId ?? null,
        model: opts.model ?? null,
        chatSessionId: opts.chatSessionId ?? null,
        resume: opts.resume ?? false,
        rewindToUser: opts.rewindToUser ?? null,
        rewindDrop: opts.rewindDrop ?? false,
        recording: opts.recording ?? null,
    });
}

export async function getAllowRules(): Promise<AllowRule[]> {
    return invoke("get_allow_rules");
}

export async function removeAllowRule(tool: string, scope: string, value: string): Promise<AllowRule[]> {
    return invoke("remove_allow_rule", { tool, scope, value });
}

export async function clearAllowRules(): Promise<AllowRule[]> {
    return invoke("clear_allow_rules");
}

export async function clearAgentSession(chatSessionId: string): Promise<void> {
    return invoke("clear_agent_session", { chatSessionId });
}

/** E2E / debug: drop Rust AgentSessionStore entry; keep `.stitch/sessions/` on disk. */
export async function dropAgentMemory(chatSessionId: string): Promise<void> {
    return invoke("drop_agent_memory", { chatSessionId });
}

export interface CheckpointSummaryDto {
    epoch: number;
    parent_epoch: number;
    compression_level: string;
    summary_preview: string;
    created_at: string;
}

export interface CheckpointDiffDto {
    from_epoch: number;
    to_epoch: number;
    summary_changed: boolean;
    text: string;
}

export interface RollbackResultDto {
    epoch: number;
    summary: string;
    resume_text: string;
}

export async function listSessionCheckpoints(
    chatSessionId: string,
): Promise<CheckpointSummaryDto[]> {
    return invoke("list_session_checkpoints", { chatSessionId });
}

export async function diffSessionCheckpoints(
    chatSessionId: string,
    fromEpoch: number,
    toEpoch: number,
): Promise<CheckpointDiffDto> {
    return invoke("diff_session_checkpoints", {
        chatSessionId,
        fromEpoch,
        toEpoch,
    });
}

export async function rollbackSessionEpoch(
    chatSessionId: string,
    targetEpoch: number,
): Promise<RollbackResultDto> {
    return invoke("rollback_session_epoch", { chatSessionId, targetEpoch });
}

export async function gcOrphanAgentSessions(workDir: string, keepIds: string[]): Promise<number> {
    return invoke("gc_orphan_agent_sessions", { workDir, keepIds });
}

export interface WorkspaceCheckpointDto {
    session_id: string;
    epoch: number;
    summary_preview: string;
    resume_text: string;
    created_at: string;
}

export async function latestWorkspaceCheckpoint(
    workDir: string,
    excludeSessionId?: string | null,
): Promise<WorkspaceCheckpointDto | null> {
    return invoke("latest_workspace_checkpoint", {
        workDir,
        excludeSessionId: excludeSessionId ?? null,
    });
}

export async function cancelGeneration(): Promise<void> {
    return invoke("cancel_generation");
}

export async function respondConfirmation(
    id: string,
    approved: boolean,
    remember?: RememberRule | null,
): Promise<void> {
    return invoke("respond_confirmation", { id, approved, remember: remember ?? null });
}

export async function respondPlan(id: string, approved: boolean): Promise<void> {
    return invoke("respond_plan", { id, approved });
}

export async function listSuites(): Promise<SuiteSummary[]> {
    return invoke("list_suites");
}

export async function listAgents(): Promise<AgentSummary[]> {
    return invoke("list_agents");
}

export async function createPrompt(args: {
    title: string;
    content: string;
    description?: string;
    tags?: string[];
}): Promise<{ id: string; title: string }> {
    return invoke("create_prompt", { args });
}

/** Submit personal prompt for Explore review (ADR-033). */
export async function submitExplore(promptId: string): Promise<{
    system_prompt_id: string;
    slug: string;
    status: string;
    already_submitted: boolean;
}> {
    return invoke("submit_explore", { args: { prompt_id: promptId } });
}

/** Best-effort product analytics; no-op without account Token. */
export async function trackUsage(
    action: string,
    context?: Record<string, string | number | boolean | null | undefined>,
): Promise<void> {
    try {
        const ctx: Record<string, string> = { client: "stitch-desktop" };
        if (context) {
            for (const [k, v] of Object.entries(context)) {
                if (v === undefined || v === null) continue;
                ctx[k] = String(v);
            }
        }
        await invoke("track_usage", { action, context: ctx });
    } catch {
        /* ignore */
    }
}

export async function runSuite(id: string): Promise<void> {
    return invoke("run_suite", { id });
}

export async function runAgent(id: string): Promise<void> {
    return invoke("run_agent", { id });
}

export async function getWorkDir(): Promise<string> {
    return invoke("get_work_dir");
}

export async function setWorkDir(path: string): Promise<string> {
    return invoke("set_work_dir", { path });
}

export async function browseWorkDir(): Promise<string | null> {
    return invoke("browse_work_dir");
}

export async function openFolderPath(path: string): Promise<void> {
    return invoke("open_folder_path", { path });
}

export async function setTitlebarTheme(dark: boolean): Promise<void> {
    return invoke("set_titlebar_theme", { dark });
}

export async function finishStartup(dark: boolean): Promise<void> {
    return invoke("finish_startup", { dark });
}

export async function clearTaskbarProgress(): Promise<void> {
    return invoke("clear_taskbar_progress");
}

export async function checkUpdate(): Promise<UpdateStatus> {
    return invoke("check_update");
}

export async function installUpdate(): Promise<void> {
    return invoke("install_update");
}

export async function getMembership(): Promise<MembershipSnapshot> {
    return invoke("get_membership");
}

export async function startAccountConnect(): Promise<ConfigSnapshot> {
    return invoke("start_account_connect");
}

export async function openExternalUrl(url: string): Promise<void> {
    return invoke("open_external_url", { url });
}

export async function setCompactMode(compact: boolean): Promise<void> {
    return invoke("set_compact_mode", { compact });
}

/** Persist current window geometry (size / position / maximized). */
export async function saveWindowState(maximized: boolean): Promise<void> {
    return invoke("save_window_state", { maximized });
}

export function listenAgentEvents(handler: (event: AgentEvent) => void): Promise<UnlistenFn> {
    return listen<AgentEvent>("agent-event", (e) => handler(e.payload));
}
