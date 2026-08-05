/**
 * 时间线 UI 状态（模块级单例）——ChatView 卸载（切设置↔聊天）不丢，
 * 重启即重置（有意为之：长文展开/收起不跨重启，ChatGPT/Claude 同行为）。
 */

class TimelineUI {
  /** 长文展开状态（blockKey → 展开）——虚拟化重建与视图切换都保持。 */
  expandedBlocks = $state<Record<string, boolean>>({});

  /**
   * 上次清理的会话 id——哨兵必须也在模块级：ChatView 卸载重建后组件内
   * 变量归零，会把「视图重建」误判成「切会话」清空展开状态。
   */
  lastPrunedSession: string | null = null;

  /** 切会话清空（blockKey 是消息 id——历史会话键永远不再命中）。 */
  clearExpandedBlocks(sid: string | null) {
    if (sid === this.lastPrunedSession) return;
    this.lastPrunedSession = sid;
    this.expandedBlocks = {};
  }
}

export const timelineUI = new TimelineUI();
