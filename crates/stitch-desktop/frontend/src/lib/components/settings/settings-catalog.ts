import type { SettingsTab } from "../../nav.svelte";
import { settingsState } from "./settings-state.svelte";

export type SettingEntry = {
    id: string;
    tab: SettingsTab;
    tabLabel: string;
    title: string;
    keywords: string[];
    /** data-testid of the control to scroll to and flash. */
    target: string;
    /** Optional state prep before the target renders (e.g. expand advanced). */
    before?: () => void;
};

export const SETTINGS_CATALOG: SettingEntry[] = [
    {
        id: "model-add",
        tab: "model",
        tabLabel: "模型",
        title: "添加模型",
        keywords: ["add model", "new", "tianjia", "xinzeng"],
        target: "settings-profile-add",
    },
    {
        id: "model-provider",
        tab: "model",
        tabLabel: "模型",
        title: "提供商",
        keywords: ["provider", "deepseek", "openai", "tigongshang", "jizuo", "基座"],
        target: "profile-provider",
    },
    {
        id: "model-name",
        tab: "model",
        tabLabel: "模型",
        title: "模型名称",
        keywords: ["model", "name", "moxing", "gpt", "glm"],
        target: "profile-model",
    },
    {
        id: "model-api-base",
        tab: "model",
        tabLabel: "模型",
        title: "接口地址",
        keywords: ["api base", "url", "jiekou", "dizhi"],
        target: "profile-api-base",
    },
    {
        id: "model-api-key",
        tab: "model",
        tabLabel: "模型",
        title: "API Key",
        keywords: ["api key", "key", "miyao", "密钥", "mima"],
        target: "api-key-input",
    },
    {
        id: "model-default",
        tab: "model",
        tabLabel: "模型",
        title: "设为默认",
        keywords: ["default", "moren"],
        target: "settings-profile-default",
    },
    {
        id: "model-local-vision",
        tab: "model",
        tabLabel: "模型",
        title: "本地视觉描述",
        keywords: ["local vision", "本地视觉", "图片", "描述", "ollama", "qwen"],
        target: "settings-model-local-vision",
    },
    {
        id: "account-connect",
        tab: "account",
        tabLabel: "账号",
        title: "连接账号",
        keywords: ["connect", "login", "lianjie", "denglu", "登录"],
        target: "account-connect-web",
    },
    {
        id: "account-add",
        tab: "account",
        tabLabel: "账号",
        title: "添加账号",
        keywords: ["add account", "tianjia"],
        target: "settings-mcp-add",
    },
    {
        id: "account-name",
        tab: "account",
        tabLabel: "账号",
        title: "账号名称",
        keywords: ["name", "label", "mingcheng"],
        target: "mcp-label",
    },
    {
        id: "account-token",
        tab: "account",
        tabLabel: "账号",
        title: "账号 Token",
        keywords: ["token", "account token"],
        target: "api-token-input",
    },
    {
        id: "account-api-base",
        tab: "account",
        tabLabel: "账号",
        title: "服务地址",
        keywords: ["server", "url", "fuwu", "dizhi", "gaoji"],
        target: "prompt-api-base",
        before: () => {
            settingsState.showMcpAdvanced = true;
        },
    },
    {
        id: "account-sediment",
        tab: "account",
        tabLabel: "账号",
        title: "沉淀目标",
        keywords: ["sediment", "explore", "gongkai", "chendian", "公共库", "个人库"],
        target: "settings-sediment-visibility",
    },
    {
        id: "mcp-add",
        tab: "mcp",
        tabLabel: "MCP",
        title: "添加服务",
        keywords: ["add server", "mcp", "fuwu"],
        target: "settings-server-add",
    },
    {
        id: "mcp-import",
        tab: "mcp",
        tabLabel: "MCP",
        title: "导入 JSON",
        keywords: ["import", "json", "daoru"],
        target: "settings-server-import",
    },
    {
        id: "mcp-json",
        tab: "mcp",
        tabLabel: "MCP",
        title: "服务配置 JSON",
        keywords: ["config", "json", "peizhi"],
        target: "server-json",
    },
    {
        id: "system-theme",
        tab: "system",
        tabLabel: "通用",
        title: "外观",
        keywords: ["theme", "dark", "light", "waiguan", "zhuti", "主题", "深色", "浅色"],
        target: "settings-theme",
    },
    {
        id: "system-max-iter",
        tab: "system",
        tabLabel: "通用",
        title: "最大迭代",
        keywords: ["iteration", "max", "diedai", "迭代"],
        target: "settings-max-iterations",
    },
    {
        id: "system-allow-rules",
        tab: "system",
        tabLabel: "通用",
        title: "允许规则",
        keywords: ["allow", "rules", "allowed", "quanxian", "yuze", "jizhu", "记住", "规则", "权限"],
        target: "allow-rules",
    },
    {
        id: "system-auto-continue",
        tab: "system",
        tabLabel: "通用",
        title: "自动续跑",
        keywords: ["continue", "auto", "xupao", "zidong"],
        target: "settings-auto-continue",
    },
    {
        id: "system-update",
        tab: "system",
        tabLabel: "通用",
        title: "应用更新",
        keywords: ["update", "version", "gengxin", "shengji", "banben", "更新", "版本"],
        target: "check-update",
    },
];
