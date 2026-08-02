/** Single source of truth for the shortcuts help dialog (Ctrl+/). */
export interface ShortcutEntry {
    keys: string;
    description: string;
}

export const SHORTCUTS: ShortcutEntry[] = [
    { keys: "Ctrl+N", description: "新建会话" },
    { keys: "Ctrl+K", description: "命令面板" },
    { keys: "Ctrl+F", description: "在会话中查找" },
    { keys: "Ctrl+B", description: "收起 / 展开侧栏" },
    { keys: "Ctrl+,", description: "设置" },
    { keys: "Ctrl+Shift+C", description: "切换紧凑模式" },
    { keys: "Ctrl+/", description: "快捷键帮助" },
    { keys: "Esc", description: "停止生成 · 关闭浮层" },
    { keys: "Enter", description: "发送消息" },
    { keys: "Shift+Enter", description: "消息换行" },
];
