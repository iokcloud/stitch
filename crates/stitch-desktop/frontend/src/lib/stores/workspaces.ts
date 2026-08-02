import { writable, get, derived } from "svelte/store";
import type { WorkspaceEntry, WorkspacesStore } from "../types";
import {
  WORKSPACES_KEY,
  WORKSPACE_COLLAPSE_KEY,
  workspaceLabelFromPath,
} from "../types";
import { applyWorkDir, workDir, recentDirs } from "./app";

/** true = collapsed. Missing key → default (current expanded, others collapsed). */
export type WorkspaceCollapseMap = Record<string, boolean>;

function loadCollapse(): WorkspaceCollapseMap {
  try {
    const raw = localStorage.getItem(WORKSPACE_COLLAPSE_KEY);
    if (raw) return JSON.parse(raw) as WorkspaceCollapseMap;
  } catch {
    /* ignore */
  }
  return {};
}

function persistCollapse(map: WorkspaceCollapseMap) {
  try {
    localStorage.setItem(WORKSPACE_COLLAPSE_KEY, JSON.stringify(map));
  } catch (e) {
    console.warn("Failed to save workspace collapse:", e);
  }
}

export const workspaceCollapse = writable<WorkspaceCollapseMap>(loadCollapse());

/** Whether a sidebar workspace group shows its sessions. */
export function groupExpanded(
  map: WorkspaceCollapseMap,
  groupId: string,
  currentId: string | null,
  _namedWorkspaceCount = 0,
): boolean {
  if (Object.prototype.hasOwnProperty.call(map, groupId)) {
    return !map[groupId];
  }
  return !!currentId && groupId === currentId;
}

export function setWorkspaceExpanded(groupId: string, expanded: boolean) {
  workspaceCollapse.update((map) => {
    const next = { ...map, [groupId]: !expanded };
    persistCollapse(next);
    return next;
  });
}

export function toggleWorkspaceCollapse(groupId: string) {
  const map = get(workspaceCollapse);
  const data = get(workspacesData);
  setWorkspaceExpanded(
    groupId,
    !groupExpanded(map, groupId, data.currentId, data.items.length),
  );
}

export function expandWorkspaceGroup(groupId: string) {
  setWorkspaceExpanded(groupId, true);
}

function uid(): string {
  return "w" + Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
}

export function normPath(p: string): string {
  return (p || "").replace(/^\\\\\?\\/, "").replace(/\\/g, "/").replace(/\/+$/, "");
}

function emptyStore(): WorkspacesStore {
  return { currentId: null, items: [] };
}

function sanitize(data: WorkspacesStore): WorkspacesStore {
  const seen = new Set<string>();
  const items: WorkspaceEntry[] = [];
  for (const raw of data.items ?? []) {
    if (!raw || typeof raw !== "object") continue;
    const path = typeof raw.path === "string" ? raw.path.trim() : "";
    if (!path) continue;
    const key = normPath(path).toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    const id =
      typeof raw.id === "string" && raw.id.trim() ? raw.id.trim() : uid();
    items.push({
      id,
      label:
        typeof raw.label === "string" && raw.label.trim()
          ? raw.label.trim()
          : workspaceLabelFromPath(path),
      path,
      lastUsedAt:
        typeof raw.lastUsedAt === "number" && raw.lastUsedAt > 0
          ? raw.lastUsedAt
          : Date.now(),
    });
  }
  // Keep array order stable — do not re-sort on activate / touch.
  let currentId = data.currentId;
  if (currentId && !items.some((i) => i.id === currentId)) currentId = null;
  if (!currentId && items[0]) currentId = items[0].id;
  return { currentId, items };
}

function load(): WorkspacesStore {
  try {
    const raw = localStorage.getItem(WORKSPACES_KEY);
    if (raw) return sanitize(JSON.parse(raw) as WorkspacesStore);
  } catch {
    /* ignore */
  }
  return emptyStore();
}

function persist(data: WorkspacesStore) {
  try {
    localStorage.setItem(WORKSPACES_KEY, JSON.stringify(data));
  } catch (e) {
    console.warn("Failed to save workspaces:", e);
  }
}

export const workspacesData = writable<WorkspacesStore>(load());

export const workspaceList = derived(workspacesData, ($d) => $d.items);

export const currentWorkspace = derived(workspacesData, ($d) =>
  $d.items.find((i) => i.id === $d.currentId) ?? null,
);

function update(mutator: (data: WorkspacesStore) => void) {
  workspacesData.update((data) => {
    mutator(data);
    const next = sanitize(data);
    persist(next);
    return next;
  });
}

/** Seed from current work dir + recent paths when the store is empty. */
export function bootstrapWorkspaces(currentPath: string, recent: string[]) {
  const data = get(workspacesData);
  if (data.items.length > 0) {
    if (currentPath.trim()) touchWorkspacePath(currentPath.trim());
    return;
  }
  const paths = [
    currentPath.trim(),
    ...recent.map((p) => p.trim()).filter(Boolean),
  ].filter(Boolean);
  const seen = new Set<string>();
  const items: WorkspaceEntry[] = [];
  const now = Date.now();
  for (const path of paths) {
    const key = normPath(path).toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    items.push({
      id: uid(),
      label: workspaceLabelFromPath(path),
      path,
      lastUsedAt: now - items.length,
    });
  }
  if (!items.length) return;
  const next: WorkspacesStore = {
    currentId: items[0].id,
    items,
  };
  workspacesData.set(sanitize(next));
  persist(get(workspacesData));
}

/**
 * Upsert by path without switching the active workspace.
 * Used when reconciling session-remembered directories.
 */
export function ensureWorkspacePath(path: string, label?: string): string {
  const trimmed = path.trim();
  if (!trimmed) return "";
  let id = "";
  update((data) => {
    const key = normPath(trimmed).toLowerCase();
    let item = data.items.find((i) => normPath(i.path).toLowerCase() === key);
    if (item) {
      item.path = trimmed;
      if (label?.trim()) item.label = label.trim();
      id = item.id;
    } else {
      id = uid();
      data.items.push({
        id,
        label: label?.trim() || workspaceLabelFromPath(trimmed),
        path: trimmed,
        lastUsedAt: Date.now(),
      });
      if (!data.currentId) data.currentId = id;
    }
  });
  return id;
}

/** Upsert by path and mark as current (does not call set_work_dir). */
export function touchWorkspacePath(path: string, label?: string): string {
  const trimmed = path.trim();
  if (!trimmed) return "";
  let id = ensureWorkspacePath(trimmed, label);
  if (!id) return "";
  update((data) => {
    data.currentId = id;
  });
  return id;
}

export function removeWorkspace(id: string) {
  update((data) => {
    data.items = data.items.filter((i) => i.id !== id);
    if (data.currentId === id) {
      data.currentId = data.items[0]?.id ?? null;
    }
  });
}

/** Open workspace path in the OS file manager. */
export async function openWorkspaceFolder(id: string): Promise<void> {
  const item = get(workspacesData).items.find((i) => i.id === id);
  if (!item?.path?.trim()) throw new Error("工作区没有目录");
  const ipc = await import("../ipc");
  await ipc.openFolderPath(item.path.trim());
}

/** Remove workspace from the list; activate the next one if needed. */
export async function removeWorkspaceAndActivate(id: string): Promise<void> {
  const before = get(workspacesData);
  const wasCurrent = before.currentId === id;
  removeWorkspace(id);
  if (!wasCurrent) return;
  const next = get(workspacesData).currentId;
  if (next) {
    await activateWorkspace(next);
  }
}

/** Switch active workspace: set work_dir + expand — do NOT move the current session. */
export async function activateWorkspace(id: string): Promise<string> {
  const item = get(workspacesData).items.find((i) => i.id === id);
  if (!item) throw new Error("找不到工作区");
  expandWorkspaceGroup(id);
  // bindSession:false — otherwise chats from A jump under B when switching projects.
  return applyWorkDir(item.path, { bindSession: false });
}

/** After native path change: sync list; optionally rebind current session. */
export async function afterWorkDirApplied(
  canonical: string,
  prev: string,
  opts?: { bindSession?: boolean },
): Promise<void> {
  touchWorkspacePath(canonical);
  const { bindCurrentSessionWorkDir, sessionsData } = await import("./sessions");
  const sid = get(sessionsData).current;
  const existing = sid
    ? get(sessionsData).sessions[sid]?.workDirPath?.trim()
    : "";
  const shouldBind =
    opts?.bindSession === true ||
    (opts?.bindSession !== false && !existing);
  if (shouldBind) {
    bindCurrentSessionWorkDir(canonical);
  }
  if (prev && normPath(prev).toLowerCase() !== normPath(canonical).toLowerCase()) {
    if (sid) {
      const ipc = await import("../ipc");
      void ipc.clearAgentSession(sid).catch(() => {});
    }
  }
}

/** Ensure list tracks the live workDir store (bootstrap helper). */
export function syncFromLiveWorkDir() {
  const path = get(workDir).trim();
  const recent = get(recentDirs);
  bootstrapWorkspaces(path, recent);
  void import("./sessions").then((m) => m.reconcileSessionWorkDirs());
}
