import { get } from "svelte/store";
import { config } from "../../stores/app";
import { nav, type SettingsTab } from "../../nav.svelte";
import { diag, diagError, clearDiagError, diagLastError } from "../../diag";
import {
    PROD_API_BASE,
    PROVIDER_PRESETS,
    WORKDIR_NUDGE_KEY,
    providerPresetLabel,
    type AllowRule,
    type ConfigSnapshot,
    type LlmProfileSnapshot,
    type McpProfileSnapshot,
    type McpServerSnapshot,
} from "../../types";
import { normalizeOpenAiCompatibleBase } from "../../llm-url";
import * as ipc from "../../ipc";
import {
    ensureSession,
    repairSessions,
    resetSessions,
    setSessionLlm,
} from "../../stores/sessions";
import { friendlyMcpError, friendlyModelError } from "../../settings-errors";
import { clearMembershipCache } from "../../membership";

const DEFAULT_SERVER_JSON = `{
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem"]
}`;

/**
 * Settings form state + actions, shared by SettingsView shell and the four
 * pane components. Mirrors nav.svelte.ts singleton-rune pattern so pane
 * components stay markup-only.
 */
class SettingsState {
    tab = $state<SettingsTab>("model");
    profiles = $state<LlmProfileSnapshot[]>([]);
    selectedProfileId = $state("default");
    profileLabel = $state("");
    activeProfileId = $state<string | null>(null);
    provider = $state("deepseek");
    apiBase = $state("");
    /** Empty when a key is already stored — never put mask chars in the value. */
    apiKey = $state("");
    apiKeyStored = $state(false);
    apiKeyVisible = $state(false);
    model = $state("");
    maxIterations = $state(25);
    localVisionEnabled = $state(true);
    localVisionApiBase = $state("http://127.0.0.1:11434/v1");
    localVisionModel = $state("qwen3-vl:8b");
    allowRules = $state<AllowRule[]>([]);
    allowRulesLoaded = $state(false);
    mcpProfiles = $state<McpProfileSnapshot[]>([]);
    selectedMcpId = $state("default");
    mcpLabel = $state("PromptStdio");
    activeMcpId = $state<string | null>(null);
    promptApiBase = $state(PROD_API_BASE);
    apiToken = $state("");
    apiTokenStored = $state(false);
    apiTokenMasked = $state("");
    sedimentVisibility = $state<"personal" | "explore">("explore");
    apiTokenVisible = $state(false);
    connectingAccount = $state(false);
    showMcpAdvanced = $state(false);
    mcpServers = $state<McpServerSnapshot[]>([]);
    selectedServerId = $state("");
    serverEnabled = $state(true);
    /** Single-server JSON editor (Cursor-shaped fields). */
    serverJsonText = $state("");
    mcpImportJson = $state("");
    showMcpImport = $state(false);
    status = $state("");
    statusError = $state(false);
    /** null = not tested this visit; false blocks enter/return chat. */
    lastTestOk = $state<boolean | null>(null);
    /** Per-profile account probe: true=可用, false=已失效; missing=已保存 only. */
    accountProbeOk = $state<Record<string, boolean>>({});
    saving = $state(false);
    testing = $state(false);
    testingPrompt = $state(false);
    testingServer = $state(false);
    updateText = $state("");
    installMode = $state(false);
    modelQuickOpen = $state(false);
    /** Avoid $effect clobbering what the user is typing after first hydrate. */
    hydratedFromConfig = $state(false);

    models = $derived(PROVIDER_PRESETS[this.provider]?.models ?? []);
    isActiveProfile = $derived(
        !!this.activeProfileId && this.selectedProfileId === this.activeProfileId,
    );
    isActiveMcp = $derived(
        !!this.activeMcpId && this.selectedMcpId === this.activeMcpId,
    );
    apiKeyPlaceholder = $derived(
        this.apiKeyStored ? "已保存 — 输入新密钥可覆盖" : "粘贴模型服务商提供的密钥",
    );
    apiTokenPlaceholder = $derived(
        this.apiTokenStored ? "已保存 — 输入新 Token 可覆盖" : "在 promptstdio.com 个人设置中创建",
    );
    usingCustomMcpBase = $derived(
        this.promptApiBase.trim().replace(/\/$/, "") !== PROD_API_BASE &&
            this.promptApiBase.trim().replace(/\/$/, "") !== "https://promptstdio.com",
    );
    showStatusCheck = $derived(
        !this.statusError &&
            !!this.status &&
            /^(已连接|账号已连接|账号可用|服务已连接|已保存|已是最新版本|已清除|已设为默认|已删除|已启用|已停用)/.test(
                this.status,
            ),
    );

    migrateModelAlias(name: string): string {
        if (name === "deepseek-chat" || name === "deepseek-reasoner") {
            return "deepseek-v4-flash";
        }
        return name;
    }

    applyProfileToForm(p: LlmProfileSnapshot | undefined, cfgFallback = get(config)) {
        if (p) {
            this.selectedProfileId = p.id;
            this.profileLabel = p.label || p.id;
            this.provider = p.provider || "deepseek";
            this.apiBase = p.api_base || "";
            this.model = this.migrateModelAlias(p.model || "");
            this.apiKeyStored = !!p.api_key_set;
        } else if (cfgFallback) {
            this.selectedProfileId = cfgFallback.active_profile_id || "default";
            this.profileLabel = "DeepSeek";
            this.provider = cfgFallback.llm_provider || "deepseek";
            this.apiBase = cfgFallback.llm_api_base || "";
            this.model = this.migrateModelAlias(cfgFallback.llm_model || "");
            this.apiKeyStored = !!cfgFallback.llm_api_key_set;
        }
        this.apiKey = "";
        this.lastTestOk = null;
    }

    applyMcpToForm(p: McpProfileSnapshot | undefined, cfgFallback = get(config)) {
        if (p) {
            this.selectedMcpId = p.id;
            this.mcpLabel = p.label || "PromptStdio";
            this.promptApiBase = p.api_base || PROD_API_BASE;
            this.apiTokenStored = !!p.api_token_set;
            this.apiTokenMasked = p.api_token_masked || "";
        } else if (cfgFallback) {
            this.selectedMcpId = cfgFallback.active_mcp_id || "default";
            this.mcpLabel = "PromptStdio";
            this.promptApiBase = cfgFallback.api_base || PROD_API_BASE;
            this.apiTokenStored = !!cfgFallback.api_token_set;
            this.apiTokenMasked = cfgFallback.api_token_masked || "";
        }
        this.apiToken = "";
        const base = (this.promptApiBase || "").trim().replace(/\/$/, "");
        this.showMcpAdvanced =
            !!base && base !== PROD_API_BASE && base !== "https://promptstdio.com";
    }

    serverToEditorJson(p: McpServerSnapshot): string {
        const o: Record<string, unknown> = {};
        if (p.label && p.label !== p.id) o.name = p.label;
        if (p.transport === "sse" || p.transport === "http") {
            o.type = p.transport;
        }
        if (p.command) o.command = p.command;
        if (p.args?.length) o.args = p.args;
        if (p.env && Object.keys(p.env).length) o.env = { ...p.env };
        if (p.cwd) o.cwd = p.cwd;
        if (p.url) o.url = p.url;
        if (p.auth_set) {
            o.headers = { Authorization: "(已保存，改写请填新 Token)" };
        }
        return JSON.stringify(o, null, 2);
    }

    parseServerEditorJson(
        id: string,
        text: string,
        enabled: boolean,
    ): ipc.UpsertMcpServerArgs | null {
        let raw: unknown;
        try {
            raw = JSON.parse(text);
        } catch {
            this.status = "JSON 无效";
            this.statusError = true;
            this.tab = "mcp";
            return null;
        }
        if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
            this.status = "须为 JSON 对象";
            this.statusError = true;
            this.tab = "mcp";
            return null;
        }
        const obj = raw as Record<string, unknown>;
        const command =
            typeof obj.command === "string" && obj.command.trim()
                ? obj.command.trim()
                : undefined;
        const url =
            typeof obj.url === "string" && obj.url.trim()
                ? obj.url.trim()
                : typeof obj.serverUrl === "string" && obj.serverUrl.trim()
                  ? obj.serverUrl.trim()
                  : undefined;
        const typeHint =
            typeof obj.type === "string"
                ? obj.type.trim().toLowerCase()
                : typeof obj.transport === "string"
                  ? obj.transport.trim().toLowerCase()
                  : "";
        let transport = "stdio";
        if (typeHint === "sse" || typeHint === "http+sse") transport = "sse";
        else if (
            typeHint === "http" ||
            typeHint === "streamable-http" ||
            typeHint === "streamable_http"
        ) {
            transport = "http";
        } else if (url) transport = "http";
        else if (command) transport = "stdio";

        if (transport === "stdio" && !command) {
            this.status = "stdio 须有 command";
            this.statusError = true;
            this.tab = "mcp";
            return null;
        }
        if ((transport === "http" || transport === "sse") && !url) {
            this.status = "远程服务须有 url";
            this.statusError = true;
            this.tab = "mcp";
            return null;
        }

        const args: string[] = Array.isArray(obj.args)
            ? obj.args.filter((x): x is string => typeof x === "string")
            : [];
        const env: Record<string, string> = {};
        if (obj.env && typeof obj.env === "object" && !Array.isArray(obj.env)) {
            for (const [k, v] of Object.entries(obj.env as Record<string, unknown>)) {
                if (typeof v === "string") env[k] = v;
                else if (typeof v === "number" || typeof v === "boolean") env[k] = String(v);
            }
        }
        const cwd =
            typeof obj.cwd === "string"
                ? obj.cwd
                : typeof obj.workingDirectory === "string"
                  ? obj.workingDirectory
                  : "";
        const label =
            (typeof obj.name === "string" && obj.name.trim()) ||
            (typeof obj.label === "string" && obj.label.trim()) ||
            id;

        const out: ipc.UpsertMcpServerArgs = {
            id,
            label,
            transport,
            enabled,
            command,
            args,
            env,
            cwd,
            url,
        };

        const headers: Record<string, string> = {};
        if (obj.headers && typeof obj.headers === "object" && !Array.isArray(obj.headers)) {
            for (const [k, v] of Object.entries(obj.headers as Record<string, unknown>)) {
                if (typeof v === "string" && !v.includes("已保存")) headers[k] = v;
            }
        }
        if (Object.keys(headers).length) out.headers = headers;
        const authHdr = headers.Authorization || headers.authorization;
        if (authHdr) out.auth_token = authHdr;

        return out;
    }

    applyServerToForm(p: McpServerSnapshot | undefined) {
        if (!p) {
            this.selectedServerId = "";
            this.serverEnabled = true;
            this.serverJsonText = "";
            return;
        }
        this.selectedServerId = p.id;
        this.serverEnabled = !!p.enabled;
        this.serverJsonText = this.serverToEditorJson(p);
    }

    syncFromConfig(cfg: NonNullable<ConfigSnapshot>) {
        this.profiles = cfg.llm_profiles ?? [];
        this.activeProfileId = cfg.active_profile_id ?? this.profiles[0]?.id ?? null;
        this.maxIterations = cfg.max_iterations || 25;
        this.localVisionEnabled = cfg.local_vision?.enabled ?? true;
        this.localVisionApiBase = cfg.local_vision?.api_base ?? "http://127.0.0.1:11434/v1";
        this.localVisionModel = cfg.local_vision?.model ?? "qwen3-vl:8b";
        this.mcpProfiles = cfg.mcp_profiles ?? [];
        this.activeMcpId = cfg.active_mcp_id ?? this.mcpProfiles[0]?.id ?? null;
        this.mcpServers = cfg.mcp_servers ?? [];
        this.sedimentVisibility =
            (cfg.sediment_visibility ?? "explore") === "personal" ? "personal" : "explore";
        const preferred =
            this.profiles.find((p) => p.id === this.selectedProfileId) ||
            this.profiles.find((p) => p.id === this.activeProfileId) ||
            this.profiles[0];
        this.applyProfileToForm(preferred, cfg);
        const preferredMcp =
            this.mcpProfiles.find((p) => p.id === this.selectedMcpId) ||
            this.mcpProfiles.find((p) => p.id === this.activeMcpId) ||
            this.mcpProfiles[0];
        this.applyMcpToForm(preferredMcp, cfg);
        const preferredServer =
            this.mcpServers.find((p) => p.id === this.selectedServerId) || this.mcpServers[0];
        this.applyServerToForm(preferredServer);
    }

    uniqueLabel(base: string): string {
        const others = this.profiles.filter((p) => p.id !== this.selectedProfileId);
        const taken = new Set(others.map((p) => (p.label || p.id).trim()).filter(Boolean));
        if (!taken.has(base)) return base;
        let n = 2;
        while (taken.has(`${base} ${n}`)) n += 1;
        return `${base} ${n}`;
    }

    onProviderChange = () => {
        this.lastTestOk = null;
        const preset = PROVIDER_PRESETS[this.provider];
        if (!preset) return;
        if (preset.api_base) this.apiBase = preset.api_base;
        if (preset.models.length > 0 && !preset.models.includes(this.model)) {
            this.model = preset.models[0];
        }
        const presetLabels = new Set(
            Object.values(PROVIDER_PRESETS).map((p) => p.label),
        );
        if (!this.profileLabel.trim() || presetLabels.has(this.profileLabel.trim())) {
            this.profileLabel = this.uniqueLabel(preset.label);
        }
    };

    selectProfile(id: string) {
        const p = this.profiles.find((x) => x.id === id);
        if (!p) return;
        this.applyProfileToForm(p);
    }

    selectMcp(id: string) {
        const p = this.mcpProfiles.find((x) => x.id === id);
        if (!p) return;
        this.applyMcpToForm(p);
    }

    setAccountProbe(id: string, ok: boolean) {
        if (!id) return;
        this.accountProbeOk = { ...this.accountProbeOk, [id]: ok };
    }

    clearAccountProbe(id: string) {
        if (!id || !(id in this.accountProbeOk)) return;
        const next = { ...this.accountProbeOk };
        delete next[id];
        this.accountProbeOk = next;
    }

    markTokenDirty = () => {
        this.clearAccountProbe(this.selectedMcpId);
    };

    mcpChipMeta(p: McpProfileSnapshot): string {
        if (!p.api_token_set) return "未设置";
        const probe = this.accountProbeOk[p.id];
        if (probe === false) return "已失效";
        if (probe === true) return "可用";
        return "已保存";
    }

    selectServer(id: string) {
        const p = this.mcpServers.find((x) => x.id === id);
        if (!p) return;
        this.applyServerToForm(p);
    }

    newProfileId(): string {
        return "p" + Date.now().toString(36) + Math.random().toString(36).slice(2, 5);
    }

    uniqueMcpLabel(base: string): string {
        const others = this.mcpProfiles.filter((p) => p.id !== this.selectedMcpId);
        const taken = new Set(others.map((p) => (p.label || p.id).trim()).filter(Boolean));
        if (!taken.has(base)) return base;
        let n = 2;
        while (taken.has(`${base} ${n}`)) n += 1;
        return `${base} ${n}`;
    }

    onAddProfile = () => {
        const id = this.newProfileId();
        this.selectedProfileId = id;
        this.provider = "custom";
        this.profileLabel = this.uniqueLabel(providerPresetLabel("custom"));
        this.apiBase = "";
        this.model = "";
        this.apiKey = "";
        this.apiKeyStored = false;
        this.lastTestOk = null;
        this.status = "";
        this.statusError = false;
        this.tab = "model";
    };

    onAddMcpProfile = () => {
        const id = this.newProfileId();
        this.selectedMcpId = id;
        this.mcpLabel = this.uniqueMcpLabel("PromptStdio");
        this.promptApiBase = PROD_API_BASE;
        this.apiToken = "";
        this.apiTokenStored = false;
        this.apiTokenMasked = "";
        this.showMcpAdvanced = false;
        this.status = "";
        this.statusError = false;
        this.tab = "account";
    };

    onAddMcpServer = () => {
        const id = this.newProfileId();
        this.selectedServerId = id;
        this.serverEnabled = true;
        this.serverJsonText = DEFAULT_SERVER_JSON;
        this.showMcpImport = false;
        this.status = "";
        this.statusError = false;
        this.tab = "mcp";
    };

    onImportMcpJson = async () => {
        const raw = this.mcpImportJson.trim();
        if (!raw) {
            this.status = "请粘贴 mcpServers JSON";
            this.statusError = true;
            return;
        }
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            const cfg = await ipc.importMcpServers(raw, false);
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.showMcpImport = false;
            this.mcpImportJson = "";
            const n = (cfg.mcp_servers ?? []).length;
            this.status = `已导入，当前共 ${n} 个服务`;
            this.statusError = false;
            this.tab = "mcp";
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
            this.tab = "mcp";
        } finally {
            this.saving = false;
        }
    };

    onAddPromptstdioMcp = async () => {
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            const existed = this.mcpServers.some((p) => p.id === "promptstdio");
            const cfg = await ipc.addPromptstdioMcpPreset();
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.selectedServerId = "promptstdio";
            const p = (cfg.mcp_servers ?? []).find((x) => x.id === "promptstdio");
            this.applyServerToForm(p);
            this.status = existed
                ? "PromptStdio 已在列表中（默认停用）"
                : "已添加 PromptStdio（默认停用，可启用）";
            this.statusError = false;
            this.tab = "mcp";
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
            this.tab = "mcp";
        } finally {
            this.saving = false;
        }
    };

    onDeleteProfile = async () => {
        if (this.profiles.length <= 1) {
            this.status = "至少保留一套模型配置";
            this.statusError = true;
            return;
        }
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            const cfg = await ipc.deleteLlmProfile(this.selectedProfileId);
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.status = "已删除";
        } catch (e) {
            this.status = friendlyModelError(e);
            this.statusError = true;
        } finally {
            this.saving = false;
        }
    };

    onDeleteMcpProfile = async () => {
        if (this.mcpProfiles.length <= 1) {
            this.status = "至少保留一套账号配置";
            this.statusError = true;
            return;
        }
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            const deletedId = this.selectedMcpId;
            const cfg = await ipc.deleteMcpProfile(this.selectedMcpId);
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.clearAccountProbe(deletedId);
            clearMembershipCache();
            this.status = "已删除";
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
        } finally {
            this.saving = false;
        }
    };

    onDeleteMcpServer = async () => {
        if (!this.selectedServerId.trim()) return;
        if (!this.mcpServers.some((p) => p.id === this.selectedServerId)) {
            this.applyServerToForm(this.mcpServers[0]);
            return;
        }
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            const cfg = await ipc.deleteMcpServer(this.selectedServerId);
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.status = "已删除";
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
        } finally {
            this.saving = false;
        }
    };

    onToggleServerEnabled = async () => {
        if (!this.mcpServers.some((p) => p.id === this.selectedServerId)) {
            this.status = "请先保存服务";
            this.statusError = true;
            return;
        }
        const next = !this.serverEnabled;
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            const cfg = await ipc.setMcpServerEnabled(this.selectedServerId, next);
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.status = next ? "已启用" : "已停用";
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
        } finally {
            this.saving = false;
        }
    };

    async persistMcpServer(): Promise<boolean> {
        if (!this.selectedServerId.trim()) {
            this.status = "配置无效";
            this.statusError = true;
            this.tab = "mcp";
            return false;
        }
        const args = this.parseServerEditorJson(
            this.selectedServerId.trim(),
            this.serverJsonText,
            this.serverEnabled,
        );
        if (!args) return false;
        const cfg = await ipc.upsertMcpServer(args);
        config.set(cfg);
        this.mcpServers = cfg.mcp_servers ?? [];
        const p = this.mcpServers.find((x) => x.id === this.selectedServerId);
        this.applyServerToForm(p);
        return true;
    }

    onSetDefaultProfile = async () => {
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            // Persist current form first so default points at latest values.
            if (!(await this.persistModelProfile())) return;
            const cfg = await ipc.setActiveLlmProfile(this.selectedProfileId);
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.status = "已设为默认";
        } catch (e) {
            this.status = friendlyModelError(e);
            this.statusError = true;
        } finally {
            this.saving = false;
        }
    };

    onSetDefaultMcp = async () => {
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            if (!(await this.persistMcpProfile())) return;
            const cfg = await ipc.setActiveMcpProfile(this.selectedMcpId);
            config.set(cfg);
            this.syncFromConfig(cfg);
            clearMembershipCache();
            this.status = "已设为默认";
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
        } finally {
            this.saving = false;
        }
    };

    clearApiToken = async () => {
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            const id = this.selectedMcpId.trim() || this.activeMcpId || "default";
            const cfg = await ipc.clearMcpProfileToken(id);
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.clearAccountProbe(id);
            clearMembershipCache();
            this.status = "已清除账号 Token";
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
        } finally {
            this.saving = false;
        }
    };

    hasUsableApiKey(): boolean {
        if (this.apiKey.trim().length > 0) return true;
        if (this.apiKeyStored) return true;
        const p = this.profiles.find((x) => x.id === this.selectedProfileId);
        if (p?.api_key_set) return true;
        return !!get(config)?.llm_api_key_set;
    }

    markKeyDirty = () => {
        // Invalidate a prior success so chat requires retest. Keep failure until
        // a successful probe — clearing the field must not unblock return-to-chat.
        if (this.lastTestOk === true) this.lastTestOk = null;
    };

    /**
     * Enter chat only when key exists and last test did not fail.
     * Typed new key (non-empty field) requires a successful test this visit.
     * Takes cfg so the shell can pass the live $config store value.
     */
    canGoChat(cfg: ConfigSnapshot | null): boolean {
        if (this.lastTestOk === false) return false;
        const typed = this.apiKey.trim().length > 0;
        if (typed) return this.lastTestOk === true;
        const stored =
            this.apiKeyStored ||
            this.profiles.some((p) => p.api_key_set) ||
            !!cfg?.llm_api_key_set;
        if (!stored) return false;
        return true;
    }

    /** Save the form as an LLM profile (does not write MCP / iterations). */
    async persistModelProfile(): Promise<boolean> {
        if (!this.model.trim()) {
            this.status = "请填写模型名称";
            this.statusError = true;
            this.tab = "model";
            return false;
        }
        if (!this.selectedProfileId.trim()) {
            this.status = "配置无效";
            this.statusError = true;
            return false;
        }
        const args: ipc.UpsertLlmProfileArgs = {
            id: this.selectedProfileId.trim(),
            label: this.profileLabel.trim() || providerPresetLabel(this.provider),
            provider: this.provider,
            api_base: normalizeOpenAiCompatibleBase(this.apiBase.trim()),
            model: this.migrateModelAlias(this.model.trim()),
        };
        const keyVal = this.apiKey.trim();
        if (keyVal) args.api_key = keyVal;
        const cfg = await ipc.upsertLlmProfile(args);
        config.set(cfg);
        this.profiles = cfg.llm_profiles ?? [];
        this.activeProfileId = cfg.active_profile_id ?? null;
        const p = this.profiles.find((x) => x.id === this.selectedProfileId);
        this.apiKeyStored = !!p?.api_key_set || !!cfg.llm_api_key_set;
        if (p) {
            this.apiBase = p.api_base || args.api_base;
            this.profileLabel = p.label || args.label || this.profileLabel;
        } else {
            this.apiBase = args.api_base;
        }
        if (keyVal) this.apiKey = "";
        return true;
    }

    /** Save the form as a PromptStdio account profile. */
    async persistMcpProfile(): Promise<boolean> {
        if (!this.selectedMcpId.trim()) {
            this.status = "配置无效";
            this.statusError = true;
            return false;
        }
        const args: ipc.UpsertMcpProfileArgs = {
            id: this.selectedMcpId.trim(),
            label: this.mcpLabel.trim() || "PromptStdio",
            api_base: this.promptApiBase.trim() || PROD_API_BASE,
        };
        const tokenVal = this.apiToken.trim();
        if (tokenVal) args.api_token = tokenVal;
        const cfg = await ipc.upsertMcpProfile(args);
        config.set(cfg);
        this.mcpProfiles = cfg.mcp_profiles ?? [];
        this.activeMcpId = cfg.active_mcp_id ?? null;
        const p = this.mcpProfiles.find((x) => x.id === this.selectedMcpId);
        this.applyMcpToForm(p, cfg);
        return true;
    }

    async persistConfig(): Promise<boolean> {
        if (this.tab === "model" || !this.hydratedFromConfig) {
            if (!(await this.persistModelProfile())) return false;
        }
        if (this.tab === "account") {
            if (!(await this.persistMcpProfile())) return false;
        }
        if (this.tab === "mcp") {
            if (!(await this.persistMcpServer())) return false;
        }
        const updates: Record<string, string> = {
            max_iterations: String(this.maxIterations),
            sediment_visibility: this.sedimentVisibility,
            local_vision_enabled: String(this.localVisionEnabled),
            local_vision_api_base: this.localVisionApiBase,
            local_vision_model: this.localVisionModel,
        };

        const cfg = await ipc.saveConfig(updates);
        config.set(cfg);
        this.profiles = cfg.llm_profiles ?? [];
        this.activeProfileId = cfg.active_profile_id ?? null;
        this.mcpProfiles = cfg.mcp_profiles ?? [];
        this.activeMcpId = cfg.active_mcp_id ?? null;
        this.mcpServers = cfg.mcp_servers ?? [];
        this.sedimentVisibility =
            (cfg.sediment_visibility ?? "explore") === "personal" ? "personal" : "explore";
        const p = this.mcpProfiles.find((x) => x.id === this.selectedMcpId);
        if (p) this.applyMcpToForm(p, cfg);
        else {
            this.apiTokenStored = !!cfg.api_token_set;
            this.apiTokenMasked = cfg.api_token_masked || "";
        }
        const s = this.mcpServers.find((x) => x.id === this.selectedServerId);
        if (s) this.applyServerToForm(s);
        return true;
    }

    connectionOverrides(): ipc.TestConnectionOverrides | undefined {
        const o: ipc.TestConnectionOverrides = {
            profile_id: this.selectedProfileId,
        };
        if (this.apiKey.trim()) o.llm_api_key = this.apiKey.trim();
        const base = normalizeOpenAiCompatibleBase(this.apiBase.trim());
        if (base) o.llm_api_base = base;
        if (this.model.trim()) o.llm_model = this.migrateModelAlias(this.model.trim());
        return o;
    }

    /** Probe model with form values; does not write config. */
    async verifyModelConnection(): Promise<boolean> {
        try {
            const ok = await ipc.testConnection(this.connectionOverrides());
            this.lastTestOk = !!ok;
            if (ok) {
                this.status = "已连接";
                this.statusError = false;
                return true;
            }
            this.status = "模型连接失败";
            this.statusError = true;
            return false;
        } catch (e) {
            this.lastTestOk = false;
            this.status = friendlyModelError(e);
            this.statusError = true;
            return false;
        }
    }

    save = async (thenChat: boolean) => {
        this.saving = true;
        this.status = "";
        this.statusError = false;
        try {
            if (thenChat && !this.hasUsableApiKey()) {
                this.status = "请先填写 API Key";
                this.statusError = true;
                this.tab = "model";
                diag("save blocked: missing API key", "error");
                return;
            }
            // Verify before persist so a bad typed key never overwrites a good stored key.
            if (thenChat || this.apiKey.trim()) {
                this.status = "正在验证模型…";
                const ok = await this.verifyModelConnection();
                if (!ok) {
                    this.tab = "model";
                    diag("save blocked: model test failed", "error");
                    return;
                }
            }
            if (!(await this.persistConfig())) return;
            this.status = "已保存";
            if (thenChat) {
                if (nav.settingsFirstRun) {
                    try {
                        sessionStorage.setItem(WORKDIR_NUDGE_KEY, "1");
                    } catch {
                        /* ignore */
                    }
                }
                this.navigateToChat("save-and-start");
            }
        } catch (e) {
            this.status = friendlyModelError(e);
            this.statusError = true;
            diagError(e, "save");
        } finally {
            this.saving = false;
        }
    };

    navigateToChat(reason: string) {
        this.status = "正在进入聊天…";
        this.statusError = false;
        clearDiagError();
        // Repair corrupt localStorage (e.g. plan without steps) before ChatView mounts.
        repairSessions();
        ensureSession();
        if (this.selectedProfileId.trim() && this.model.trim()) {
            setSessionLlm(
                this.selectedProfileId.trim(),
                this.migrateModelAlias(this.model.trim()),
            );
        }
        diag(
            `goChat click (${reason}) key=${this.hasUsableApiKey()} fromChat=${nav.settingsFromChat}`,
        );
        try {
            nav.showChat(reason);
            this.status = "";
            queueMicrotask(() => {
                if (nav.view !== "chat") {
                    const msg = `导航失败：期望 chat，实际 ${nav.view}`;
                    this.status = msg;
                    this.statusError = true;
                    diag(msg, "error");
                    return;
                }
                const err = get(diagLastError);
                if (err && /length|undefined/i.test(err)) {
                    // Last resort: wipe history so a broken plan card cannot block entry.
                    diag("goChat: chat mount error — resetting sessions", "error");
                    resetSessions();
                    clearDiagError();
                    nav.showChat(`${reason}-recovered`);
                    this.status = "聊天记录已修复，已进入新会话";
                    this.statusError = false;
                }
            });
        } catch (e) {
            this.status = String(e);
            this.statusError = true;
            diagError(e, "goChat");
        }
    }

    goChat = (e?: Event) => {
        e?.preventDefault?.();
        e?.stopPropagation?.();
        if (!this.hasUsableApiKey()) {
            this.status = "请先填写 API Key，并测试连接";
            this.statusError = true;
            this.tab = "model";
            diag("goChat blocked: no API key", "error");
            return;
        }
        if (this.lastTestOk === false) {
            this.status = "模型未连通，请先测试连接";
            this.statusError = true;
            this.tab = "model";
            diag("goChat blocked: last test failed", "error");
            return;
        }
        if (this.apiKey.trim() && this.lastTestOk !== true) {
            this.status = "请先测试连接，确认密钥可用";
            this.statusError = true;
            this.tab = "model";
            diag("goChat blocked: typed key not tested", "error");
            return;
        }
        this.navigateToChat(
            e?.currentTarget instanceof HTMLElement
                ? (e.currentTarget.dataset.testid ?? "goChat")
                : "goChat",
        );
    };

    toggleApiKeyVisible = (e?: Event) => {
        e?.preventDefault?.();
        e?.stopPropagation?.();
        this.apiKeyVisible = !this.apiKeyVisible;
    };

    toggleApiTokenVisible = (e?: Event) => {
        e?.preventDefault?.();
        e?.stopPropagation?.();
        this.apiTokenVisible = !this.apiTokenVisible;
    };

    onTest = async () => {
        this.testing = true;
        this.status = "正在测试模型…";
        this.statusError = false;
        try {
            if (!this.hasUsableApiKey()) {
                this.status = "请先填写 API Key";
                this.statusError = true;
                this.lastTestOk = false;
                return;
            }
            const ok = await this.verifyModelConnection();
            // Persist only after a successful probe (avoids saving invalid keys).
            if (ok) await this.persistConfig();
        } catch (e) {
            this.lastTestOk = false;
            this.status = friendlyModelError(e);
            this.statusError = true;
        } finally {
            this.testing = false;
        }
    };

    onConnectAccountWeb = async () => {
        this.connectingAccount = true;
        this.status = "请在浏览器中登录并确认连接…";
        this.statusError = false;
        try {
            const cfg = await ipc.startAccountConnect();
            config.set(cfg);
            this.syncFromConfig(cfg);
            this.setAccountProbe(this.selectedMcpId || cfg.active_mcp_id || "default", true);
            this.status = "账号已连接";
            this.statusError = false;
            clearMembershipCache();
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
        } finally {
            this.connectingAccount = false;
        }
    };

    onTestPromptstdio = async () => {
        this.testingPrompt = true;
        this.status = "正在测试账号连接…";
        this.statusError = false;
        try {
            if (!(await this.persistConfig())) return;
            const overrides: ipc.TestPromptstdioOverrides = {
                profile_id: this.selectedMcpId,
            };
            if (this.apiToken.trim()) overrides.api_token = this.apiToken.trim();
            const base = this.promptApiBase.trim();
            if (base) overrides.api_base = base;
            const msg = await ipc.testPromptstdio(overrides);
            this.setAccountProbe(this.selectedMcpId, true);
            this.status = msg.includes("成功") ? "账号可用" : msg;
            this.statusError = false;
            clearMembershipCache();
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
            if (this.apiTokenStored || this.apiToken.trim()) {
                this.setAccountProbe(this.selectedMcpId, false);
            }
        } finally {
            this.testingPrompt = false;
        }
    };

    onTestMcpServer = async () => {
        this.testingServer = true;
        this.status = "正在测试 MCP 服务…";
        this.statusError = false;
        try {
            if (!(await this.persistMcpServer())) return;
            const msg = await ipc.testMcpServer(this.selectedServerId);
            this.status = msg.includes("成功") ? "服务已连接" : msg;
            this.statusError = false;
        } catch (e) {
            this.status = friendlyMcpError(e);
            this.statusError = true;
        } finally {
            this.testingServer = false;
        }
    };

    onUpdateClick = async () => {
        if (this.installMode) {
            this.status = "正在安装更新…";
            this.statusError = false;
            try {
                await ipc.installUpdate();
            } catch (e) {
                const raw = String(e ?? "").replace(/^Error:\s*/i, "");
                this.status =
                    raw.length > 100 ? "安装更新失败，请稍后重试" : raw || "安装更新失败";
                this.statusError = true;
            }
            return;
        }
        try {
            this.status = "正在检查更新…";
            this.statusError = false;
            const u = await ipc.checkUpdate();
            this.updateText = `当前 ${u.current_version}`;
            if (u.available && u.latest_version) {
                this.updateText += ` · 可更新至 ${u.latest_version}`;
                this.installMode = true;
                this.status = "发现新版本";
                this.statusError = false;
            } else {
                this.status = "已是最新版本";
                this.statusError = false;
            }
        } catch (e) {
            const raw = String(e ?? "");
            this.status =
                raw.length > 120
                    ? "检查更新失败，请稍后重试"
                    : raw.replace(/^Error:\s*/i, "") || "检查更新失败";
            this.statusError = true;
            this.installMode = false;
        }
    };

    loadAllowRules = async () => {
        try {
            this.allowRules = await ipc.getAllowRules();
            this.allowRulesLoaded = true;
        } catch (e) {
            this.status = friendlyModelError(e);
            this.statusError = true;
        }
    };

    removeAllowRule = async (i: number) => {
        const rule = this.allowRules[i];
        if (!rule) return;
        try {
            this.allowRules = await ipc.removeAllowRule(rule.tool, rule.scope, rule.value);
            this.status = "已删除";
        } catch (e) {
            this.status = friendlyModelError(e);
            this.statusError = true;
        }
    };

    clearAllowRules = async () => {
        try {
            this.allowRules = await ipc.clearAllowRules();
            this.status = "已清除";
        } catch (e) {
            this.status = friendlyModelError(e);
            this.statusError = true;
        }
    };
}

export const settingsState = new SettingsState();
