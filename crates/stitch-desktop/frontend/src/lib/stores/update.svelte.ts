/**
 * 启动更新检查 — 每次启动进入聊天视图后静默检查一次，
 * 有新版本时挂出可关闭的升级提醒横幅（升级提醒 + 版本说明）。
 * 失败静默（更新检查是尽力而为，绝不打扰用户）。
 */
import * as ipc from "../ipc";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "installing"
  | "dismissed";

class UpdateStore {
  phase = $state<UpdatePhase>("idle");
  latest = $state("");
  notes = $state("");

  /** 只跑一次（任何结果后 phase 离开 idle，effect 再触发也不会重查）。 */
  async checkNow() {
    if (this.phase !== "idle") return;
    this.phase = "checking";
    try {
      const u = await ipc.checkUpdate();
      if (u.available && u.latest_version) {
        this.latest = u.latest_version;
        this.notes = u.release_notes ?? "";
        this.phase = "available";
      } else {
        this.phase = "dismissed";
      }
    } catch {
      this.phase = "dismissed";
    }
  }

  dismiss() {
    this.phase = "dismissed";
  }

  async install() {
    if (this.phase !== "available") return;
    this.phase = "installing";
    try {
      await ipc.installUpdate();
    } catch {
      this.phase = "available";
    }
  }
}

export const updateStore = new UpdateStore();
