/**
 * 时序常量单源——防抖/超时/延时魔数统一在此，可集中调参。
 * 审查发现：窗口持久化 400ms、confirm 关闭 120ms、persist 450ms、
 * auto-continue 800ms、auto-show 1500ms、安全揭示 2500ms 等散落各处。
 */

/** 窗口几何持久化防抖（resize/move 后落盘延迟）。 */
export const WINDOW_PERSIST_DEBOUNCE_MS = 400;
/** Confirm 卡关闭动画防抖。 */
export const CONFIRM_CLOSE_DEBOUNCE_MS = 120;
/** 会话/沉淀持久化防抖。 */
export const SESSION_PERSIST_DEBOUNCE_MS = 450;
/** 迭代撞顶后自动「继续执行」的延时。 */
export const AUTO_CONTINUE_DELAY_MS = 800;
/** 启动后自动显示主窗口的超时。 */
export const AUTO_SHOW_TIMEOUT_MS = 1500;
/** 启动安全揭示（loader 移除）延时。 */
export const REVEAL_SAFETY_MS = 2500;
/** 工具输出 rAF 批量刷新的最大间隔。 */
export const TOOL_OUTPUT_RAF_MS = 100;
