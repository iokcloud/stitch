import { writable, get } from "svelte/store";
import type { ConfigSnapshot, RememberRule } from "../types";
import { AUTO_CONTINUE_KEY, PLAN_MODE_KEY, RECENT_DIRS_KEY, SIDEBAR_KEY, SIDEBAR_TAB_KEY } from "../types";
import * as ipc from "../ipc";

export type InitState =
    { phase: "loading" } | { phase: "ready" } | { phase: "error"; message: string };

export const initState = writable<InitState>({ phase: "loading" });
export const config = writable<ConfigSnapshot | null>(null);
export const workDir = writable("");
export const sidebarCollapsed = writable(
    (() => {
        try {
            return localStorage.getItem(SIDEBAR_KEY) === "1";
        } catch {
            return false;
        }
    })(),
);

export const lastUserMessage = writable("");
/** Analytics: where the current turn started (`chat` | `scene`). */
export const lastSendSource = writable<"chat" | "scene">("chat");

export const confirmOpen = writable(false);
export const confirmId = writable<string | null>(null);
export const confirmTool = writable("");
export const confirmMessage = writable("");
/** When true, auto-approve write/run/delete confirms for the rest of this generation. */
export const confirmSessionAllow = writable(false);

let confirmCloseTimer: ReturnType<typeof setTimeout> | null = null;

function cancelConfirmClose() {
  if (confirmCloseTimer != null) {
    clearTimeout(confirmCloseTimer);
    confirmCloseTimer = null;
  }
}

export const workDirDialogOpen = writable(false);
export const workDirDialogError = writable("");
export const recentDirs = writable<string[]>(loadRecent());
export type PlanMode = "auto" | "on" | "off";

/** 三态：auto（模型自主判断是否规划）· on（强制规划）· off（直接执行）。 */
export const planMode = writable<PlanMode>(
  (() => {
    try {
      const v = localStorage.getItem(PLAN_MODE_KEY);
      if (v === "on" || v === "off") return v;
      return "auto";
    } catch {
      return "auto";
    }
  })(),
);
export const sidebarTab = writable<"sessions" | "library">(
    (() => {
        try {
            return localStorage.getItem(SIDEBAR_TAB_KEY) === "library" ? "library" : "sessions";
        } catch {
            return "sessions";
        }
    })(),
);

sidebarTab.subscribe((tab) => {
    try {
        localStorage.setItem(SIDEBAR_TAB_KEY, tab);
    } catch {
        /* ignore */
    }
});

/** Max chained auto-continues per session before falling back to the manual button. */
export const AUTO_CONTINUE_MAX = 3;

/** Iteration-cap auto-continue preference (default on). */
export const autoContinueEnabled = writable(
  (() => {
    try {
      return localStorage.getItem(AUTO_CONTINUE_KEY) !== "0";
    } catch {
      return true;
    }
  })(),
);

export function setAutoContinueEnabled(on: boolean) {
  autoContinueEnabled.set(on);
  try {
    localStorage.setItem(AUTO_CONTINUE_KEY, on ? "1" : "0");
  } catch {
    /* ignore */
  }
}

/** Per-session count of chained auto-continues (in-memory; resets on restart). */
const autoContinueCounts = new Map<string, number>();

/** Record a manual user send — breaks the auto-continue chain for that session. */
export function resetAutoContinue(sid: string) {
  autoContinueCounts.delete(sid);
}

/** Under the cap? Then consume one slot and allow one more auto-continue. */
export function shouldAutoContinue(sid: string): boolean {
  const n = autoContinueCounts.get(sid) ?? 0;
  if (n >= AUTO_CONTINUE_MAX) return false;
  autoContinueCounts.set(sid, n + 1);
  return true;
}

/** Ask ChatView to send「继续执行」for this session (consumed once). */
export const autoContinueRequest = writable<{ sid: string; nonce: number } | null>(null);

export function requestAutoContinue(sid: string) {
  autoContinueRequest.set({ sid, nonce: Date.now() });
}

/** Request ChatView to put text into the composer (fill, do not send). */
export const composerFillRequest = writable<{ text: string; nonce: number } | null>(null);

/** Skill workflow recording — tools started while true get `recorded` on the chip. */
export const skillRecording = writable(false);
export const skillRecordStartTime = writable<number | null>(null);
/** Number of tool calls captured during the current recording session. */
export const skillRecordSteps = writable(0);

/** G1 soft tip for paid_pool mature scenes (never blocks send). */
export type MatureSoftGate = {
  sceneId: string;
  kind: "need_token" | "need_member";
  pricingUrl: string;
} | null;

export const matureSoftGate = writable<MatureSoftGate>(null);

const SOFT_GATE_MUTE_KEY = "stitch-soft-gate-muted";

/** Session mute after user dismisses — don't re-show until restart. */
export function isSoftGateMuted(): boolean {
  try {
    return sessionStorage.getItem(SOFT_GATE_MUTE_KEY) === "1";
  } catch {
    return false;
  }
}

export function muteSoftGate() {
  try {
    sessionStorage.setItem(SOFT_GATE_MUTE_KEY, "1");
  } catch {
    /* ignore */
  }
  matureSoftGate.set(null);
}

export function clearMatureSoftGate() {
  matureSoftGate.set(null);
}

export function fillComposer(text: string) {
  const t = text.trim();
  if (!t) return;
  composerFillRequest.set({ text: t, nonce: Date.now() });
}

export function setPlanMode(mode: PlanMode) {
  planMode.set(mode);
  try {
    localStorage.setItem(PLAN_MODE_KEY, mode);
  } catch {
    /* ignore */
  }
}

function loadRecent(): string[] {
    try {
        const raw = localStorage.getItem(RECENT_DIRS_KEY);
        if (raw) return JSON.parse(raw) as string[];
    } catch {
        /* ignore */
    }
    return [];
}

function saveRecent(dirs: string[]) {
    try {
        localStorage.setItem(RECENT_DIRS_KEY, JSON.stringify(dirs.slice(0, 8)));
    } catch {
        /* ignore */
    }
}

export function toggleSidebar() {
    sidebarCollapsed.update((v) => {
        const next = !v;
        try {
            localStorage.setItem(SIDEBAR_KEY, next ? "1" : "0");
        } catch {
            /* ignore */
        }
        return next;
    });
}

export async function refreshConfig(): Promise<ConfigSnapshot | null> {
    try {
        const cfg = await ipc.getConfig();
        config.set(cfg);
        return cfg;
    } catch (e) {
        console.error("get_config failed:", e);
        return null;
    }
}

export async function refreshWorkDir() {
    try {
        const path = await ipc.getWorkDir();
        workDir.set(path);
        return path;
    } catch {
        return "";
    }
}

export function addRecentDir(path: string) {
    recentDirs.update((dirs) => {
        const next = [path, ...dirs.filter((d) => d !== path)].slice(0, 8);
        saveRecent(next);
        return next;
    });
}

export type ApplyWorkDirOpts = {
  /**
   * Whether to rewrite the current session's workDirPath.
   * - true: always (user picked a folder for this chat)
   * - false: never (switching workspace / restoring a session)
   * - omit: only if the session has no path yet
   */
  bindSession?: boolean;
};

export async function applyWorkDir(
  path: string,
  opts?: ApplyWorkDirOpts,
): Promise<string> {
    const prev = get(workDir);
    const canonical = await ipc.setWorkDir(path);
    workDir.set(canonical);
    addRecentDir(canonical);
    try {
        const { afterWorkDirApplied } = await import("./workspaces");
        await afterWorkDirApplied(canonical, prev, opts);
    } catch (e) {
        console.warn("workspace sync failed:", e);
    }
    return canonical;
}

/** Test / WDIO hook — set workdir and keep UI store in sync. */
export function installWorkDirHooks() {
    const w = window as unknown as {
        __stitchSetWorkDir?: (path: string) => Promise<string>;
        __stitchGetWorkDir?: () => string;
        __stitchDropAgentMemory?: (id: string) => Promise<void>;
    };
    w.__stitchSetWorkDir = (path: string) =>
      applyWorkDir(path, { bindSession: true });
    w.__stitchGetWorkDir = () => get(workDir);
    w.__stitchDropAgentMemory = (id: string) =>
      import("../ipc").then((ipc) => ipc.dropAgentMemory(id));
}

/** Native folder picker → apply. Returns null if user cancelled. */
export async function pickAndApplyWorkDir(): Promise<string | null> {
    const picked = await ipc.browseWorkDir();
    if (!picked) return null;
    return applyWorkDir(picked, { bindSession: true });
}

export function clearConfirmSessionAllow() {
  confirmSessionAllow.set(false);
}

/**
 * Show (or refresh) the confirm panel. Consecutive confirms update in place
 * so the overlay does not flash closed/open between tool calls.
 * If session-allow is on, approve silently without mounting the dialog.
 */
export function openConfirm(id: string, tool: string, message: string) {
  cancelConfirmClose();

  if (get(confirmSessionAllow)) {
    void ipc.respondConfirmation(id, true).catch((e) => {
      console.warn("respond_confirmation failed:", e);
    });
    return;
  }

  confirmId.set(id);
  confirmTool.set(tool);
  confirmMessage.set(message);
  confirmOpen.set(true);
}

export async function respondConfirm(
  approved: boolean,
  opts?: { sessionAllow?: boolean; remember?: RememberRule },
) {
  const id = get(confirmId);
  if (opts?.sessionAllow && approved) {
    confirmSessionAllow.set(true);
  }
  confirmId.set(null);
  if (id) {
    try {
      await ipc.respondConfirmation(id, approved, opts?.remember ?? null);
    } catch (e) {
      console.warn("respond_confirmation failed:", e);
    }
  }
  // Soft-close: if another confirm arrives within the debounce window, keep the panel.
  cancelConfirmClose();
  confirmCloseTimer = setTimeout(() => {
    confirmCloseTimer = null;
    if (!get(confirmId)) {
      confirmOpen.set(false);
      confirmTool.set("");
      confirmMessage.set("");
    }
  }, 120);
}

/** Force-close confirm UI (stream end / cancel). */
export function dismissConfirm() {
  cancelConfirmClose();
  confirmOpen.set(false);
  confirmId.set(null);
  confirmTool.set("");
  confirmMessage.set("");
}
