import { writable, get } from "svelte/store";

export type DiagLevel = "info" | "error";

export type DiagEntry = {
  id: number;
  t: number;
  level: DiagLevel;
  msg: string;
};

let seq = 0;

export const diagEntries = writable<DiagEntry[]>([]);
export const diagLastError = writable<string | null>(null);

function mirrorToRust(level: DiagLevel, message: string) {
  // Dynamic import — avoid hard-failing module init outside Tauri.
  void import("@tauri-apps/api/core")
    .then(({ invoke }) => invoke("frontend_log", { level, message }))
    .catch(() => {
      /* browser mock / IPC not ready */
    });
}

/** Always-on UI diagnostics for desktop WebView (no DevTools required). */
export function diag(msg: string, level: DiagLevel = "info") {
  const entry: DiagEntry = { id: ++seq, t: Date.now(), level, msg };
  const line = `[stitch] ${msg}`;
  if (level === "error") {
    console.error(line);
    diagLastError.set(msg);
  } else {
    console.info(line);
  }
  diagEntries.update((list) => [...list.slice(-30), entry]);
  try {
    document.documentElement.dataset.lastDiag = msg.slice(0, 240);
    document.documentElement.dataset.lastDiagLevel = level;
  } catch {
    /* ignore */
  }
  mirrorToRust(level, msg);
}

export function diagError(err: unknown, context: string) {
  const text = err instanceof Error ? err.message : String(err);
  diag(`${context}: ${text}`, "error");
}

export function clearDiagError() {
  diagLastError.set(null);
}

export function installGlobalDiagHandlers() {
  window.addEventListener("error", (ev) => {
    const detail =
      ev.error instanceof Error
        ? `${ev.error.message}\n${ev.error.stack ?? ""}`
        : `${ev.message} @ ${ev.filename}:${ev.lineno}`;
    diagError(detail, "window.error");
  });
  window.addEventListener("unhandledrejection", (ev) => {
    diagError(ev.reason, "unhandledrejection");
  });
  diag(`diag handlers ready; view=${document.documentElement.dataset.appView ?? "?"}`);
}

export function recentDiagText(limit = 8): string {
  return get(diagEntries)
    .slice(-limit)
    .map((e) => `${e.level === "error" ? "E" : "I"} ${e.msg}`)
    .join("\n");
}
