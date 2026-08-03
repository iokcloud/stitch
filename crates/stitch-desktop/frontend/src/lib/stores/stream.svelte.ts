/**
 * 流式状态机单例（Svelte 5 runes）——收敛 app.ts 的 5 个耦合 writable
 * （isStreaming/streamingContent/streamingBubbleId/streamSessionId/
 * streamStartedAt），把「回合开始 / token 追加 / 回合结束」封装为单一
 * 动作，杜绝「清 4 个 store」的散落重复与漏清。
 *
 * 参照 nav.svelte.ts 的 rune 单例先例；组件内响应式读取用
 * `stream.isStreaming` 等属性（$state 属性在组件上下文自动响应）。
 */
class StreamController {
  isStreaming = $state(false);
  streamingContent = $state("");
  streamingBubbleId = $state<string | null>(null);
  streamSessionId = $state<string | null>(null);
  streamStartedAt = $state<number | null>(null);

  /** 非组件模块（如 palette.ts）无法用 $effect——注册回合结束钩子。 */
  endHooks: Array<() => void> = [];

  /** 回合开始：绑定会话、清空状态、置 streaming。 */
  beginTurn(sid: string | null) {
    this.streamSessionId = sid;
    this.streamingContent = "";
    this.streamingBubbleId = null;
    this.streamStartedAt = Date.now();
    this.isStreaming = true;
  }

  /** token 追加（惰性 placeholder 由调用方创建后写入 bubbleId）。 */
  appendToken(text: string) {
    this.streamingContent += text;
  }

  /** 回合结束：清空全部状态并触发结束钩子。 */
  endTurn() {
    this.streamingBubbleId = null;
    this.streamingContent = "";
    this.streamSessionId = null;
    this.streamStartedAt = null;
    this.isStreaming = false;
    // 遍历副本不清空——钩子是一次性注册永久生效（splice 消费会在
    // 第二次 endTurn 时静默丢失，如 palette 的 pendingRefocus）
    for (const hook of [...this.endHooks]) hook();
  }

  /** 软解锁：仅当回合属于该会话（或未绑定）时才清理——stop 场景。 */
  softUnlockIfStale(sid: string | null) {
    if (sid != null && this.streamSessionId !== sid) return;
    this.endTurn();
  }
}

export const stream = new StreamController();

/** 注册回合结束钩子（palette 等非组件模块用——组件内请用 $effect）。 */
export function onStreamEnd(cb: () => void): void {
  stream.endHooks.push(cb);
}
