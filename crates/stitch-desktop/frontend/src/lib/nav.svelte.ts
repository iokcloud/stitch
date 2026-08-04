/**
 * App navigation — Svelte 5 rune singleton (.svelte.ts).
 * Avoid classic writable stores for view switching: split chunks can
 * accidentally duplicate store instances so Settings updates one copy
 * while +page reads another (click appears to do nothing).
 */

export type AppView = "settings" | "chat" | "error";
export type SettingsTab = "model" | "account" | "mcp" | "system";

import { diag } from "./diag";

class Nav {
  view = $state<AppView>("settings");
  /** 首次启动向导（无模型配置时全屏引导；完成后不再出现）。 */
  firstRunWizard = $state(false);
  settingsFirstRun = $state(true);
  settingsFromChat = $state(false);
  /** Which settings left-nav tab to open; default model when unset. */
  settingsTab = $state<SettingsTab>("model");
  shellReady = $state(false);

  showSettings(opts?: {
    firstRun?: boolean;
    fromChat?: boolean;
    tab?: SettingsTab;
  }) {
    const firstRun = !!opts?.firstRun;
    const fromChat = !!opts?.fromChat;
    this.settingsFirstRun = firstRun && !fromChat;
    this.settingsFromChat = fromChat;
    this.settingsTab = opts?.tab ?? "model";
    this.view = "settings";
    this.syncDom("settings");
    diag(
      `navigate → settings (firstRun=${this.settingsFirstRun}, fromChat=${fromChat}, tab=${this.settingsTab})`,
    );
  }

  markShellReady() {
    this.shellReady = true;
  }

  showFirstRun() {
    this.shellReady = true;
    this.firstRunWizard = true;
    this.syncDom("settings");
    diag("navigate → first-run wizard");
  }

  dismissFirstRun() {
    this.firstRunWizard = false;
  }

  showChat(reason = "showChat") {
    this.shellReady = true;
    this.firstRunWizard = false;
    this.view = "chat";
    this.settingsFirstRun = false;
    this.settingsFromChat = false;
    this.settingsTab = "model";
    this.syncDom("chat");
    diag(`navigate → chat (${reason})`);
  }

  private syncDom(view: AppView) {
    try {
      document.documentElement.dataset.appView = view;
      (
        window as unknown as { __STITCH_VIEW__?: string }
      ).__STITCH_VIEW__ = view;
    } catch {
      /* ignore */
    }
  }
}

export const nav = new Nav();

/** Test / WDIO hook — call from automation without relying on button hit-testing. */
export function installNavHooks() {
  const w = window as unknown as {
    __stitchShowChat?: (reason?: string) => void;
    __stitchShowSettings?: (opts?: { firstRun?: boolean; fromChat?: boolean }) => void;
    __stitchView?: () => AppView;
  };
  w.__stitchShowChat = (reason) => nav.showChat(reason ?? "hook");
  w.__stitchShowSettings = (opts) =>
    nav.showSettings(opts as { firstRun?: boolean; fromChat?: boolean; tab?: SettingsTab } | undefined);
  w.__stitchView = () => nav.view;
}
