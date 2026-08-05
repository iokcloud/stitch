/**
 * 新功能公告 — 启动后静默拉取官网公告 JSON（Rust fetch_announce IPC），
 * 未读则挂右下角可关闭横幅。已读记录 localStorage（按 id），
 * 失败/无公告静默不打扰（尽力而为）。
 */
import * as ipc from "../ipc";
import type { Announcement } from "../types";

const READ_KEY = "stitch-announce-read";

export type AnnouncePhase = "idle" | "checking" | "visible" | "dismissed" | "read";

class AnnounceStore {
  phase = $state<AnnouncePhase>("idle");
  item: Announcement | null = null;

  /** 启动后调用一次（phase 守卫，同次启动不重查）。 */
  async checkNow() {
    if (this.phase !== "idle") return;
    this.phase = "checking";
    try {
      const a = await ipc.fetchAnnounce();
      if (a && a.id && !isAnnounceRead(a.id)) {
        this.item = a;
        this.phase = "visible";
      } else {
        this.phase = "dismissed";
      }
    } catch {
      this.phase = "dismissed";
    }
  }

  /** 稍后：本次启动不再显示（未读，下次启动照常出现）。 */
  dismiss() {
    this.phase = "dismissed";
  }

  /** 知道了：记入已读，之后启动不再显示该公告。 */
  markRead() {
    if (this.item) {
      markAnnounceRead(this.item.id);
    }
    this.phase = "read";
  }
}

export const announceStore = new AnnounceStore();

function isAnnounceRead(id: string): boolean {
  try {
    const list: string[] = JSON.parse(localStorage.getItem(READ_KEY) ?? "[]");
    return Array.isArray(list) && list.includes(id);
  } catch {
    return false;
  }
}

function markAnnounceRead(id: string) {
  try {
    const list: string[] = JSON.parse(localStorage.getItem(READ_KEY) ?? "[]");
    if (!list.includes(id)) {
      localStorage.setItem(READ_KEY, JSON.stringify([...list.slice(-19), id]));
    }
  } catch {
    /* ignore */
  }
}
