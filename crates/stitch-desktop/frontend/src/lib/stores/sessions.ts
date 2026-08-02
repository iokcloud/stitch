import { writable, get, derived } from "svelte/store";
import type {
  ChatItem,
  ConfigSnapshot,
  PlanStep,
  SedimentCandidate,
  Session,
  SessionsStore,
} from "../types";
import { SESSIONS_KEY, workspaceLabelFromPath } from "../types";
import { buildMatureSediment, matchMatureScene, normalizeSedimentPlaybook } from "../mature-scenes";
import { RECOMMENDED_SCENES } from "../scenes";
import { summarizeSessionTitle } from "../session-title";
import { config, workDir } from "./app";
import { workspacesData, normPath, ensureWorkspacePath } from "./workspaces";

export { summarizeSessionTitle } from "../session-title";

/** One sidebar tree group: a named workspace with its sessions. */
export type WorkspaceSessionGroup = {
  id: string;
  label: string;
  path: string | null;
  workspaceId: string | null;
  sessions: Session[];
};

/** Default LLM binding for a new session from the active config profile. */
export function defaultSessionLlm(cfg: ConfigSnapshot | null | undefined): {
  llmProfileId?: string;
  llmModel?: string;
} {
  if (!cfg) return {};
  const profiles = cfg.llm_profiles ?? [];
  const activeId =
    cfg.active_profile_id ||
    profiles[0]?.id ||
    undefined;
  const profile =
    (activeId && profiles.find((p) => p.id === activeId)) || profiles[0];
  return {
    llmProfileId: profile?.id || activeId,
    llmModel: profile?.model || cfg.llm_model || undefined,
  };
}

function emptyStore(): SessionsStore {
  return { current: null, sessions: {} };
}

/** Sidebar relative time — compact column text (L1 Chinese). */
export function formatRelativeTime(ts: number, now = Date.now()): string {
  if (!Number.isFinite(ts) || ts <= 0) return "";
  const diff = Math.max(0, now - ts);
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return "刚刚";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}分钟前`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}小时前`;
  const day = Math.floor(hr / 24);
  if (day === 1) return "昨天";
  if (day < 7) return `${day}天前`;
  const d = new Date(ts);
  const y = d.getFullYear();
  const m = d.getMonth() + 1;
  const dd = d.getDate();
  const nowY = new Date(now).getFullYear();
  if (y === nowY) return `${m}/${dd}`;
  return `${y}/${m}/${dd}`;
}

function uid(): string {
  return "s" + Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
}

function itemId(): string {
  return "m" + Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
}

function normalizeSedimentCandidate(raw: unknown): SedimentCandidate | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const o = raw as Record<string, unknown>;
  const title = typeof o.title === "string" ? o.title.trim() : "";
  const content = typeof o.content === "string" ? o.content.trim() : "";
  if (!title || !content) return undefined;
  const updatedAt =
    typeof o.updatedAt === "number" && Number.isFinite(o.updatedAt)
      ? o.updatedAt
      : Date.now();
  return {
    title: title.slice(0, 255),
    content: content.slice(0, 5000),
    updatedAt,
  };
}

/** Normalize one persisted chat item so UI never sees undefined arrays/strings. */
function normalizeChatItem(raw: ChatItem): ChatItem {
  const id = typeof raw.id === "string" && raw.id ? raw.id : itemId();
  switch (raw.type) {
    case "message":
      return {
        ...raw,
        id,
        role: raw.role === "assistant" ? "assistant" : "user",
        content: typeof raw.content === "string" ? raw.content : "",
        images: Array.isArray(raw.images)
          ? raw.images.filter((s): s is string => typeof s === "string")
          : undefined,
      };
    case "tool":
      return {
        ...raw,
        id,
        name: typeof raw.name === "string" ? raw.name : "tool",
        done: !!raw.done,
        error: !!raw.error,
        summary: typeof raw.summary === "string" ? raw.summary : "",
        detail: typeof raw.detail === "string" ? raw.detail : "",
      };
    case "plan": {
      const steps = Array.isArray(raw.steps)
        ? raw.steps
            .filter((s) => s && typeof s === "object")
            .map((s) => ({
              description:
                typeof s.description === "string" && s.description
                  ? s.description
                  : "步骤",
              status: normalizeStepStatus(s.status),
            }))
        : [];
      const phase =
        raw.phase === "approved" || raw.phase === "rejected" ? raw.phase : "proposed";
      return {
        ...raw,
        id,
        title: typeof raw.title === "string" && raw.title.trim() ? raw.title : "执行计划",
        phase,
        steps,
      };
    }
    case "sediment":
      return {
        ...raw,
        id,
        title: typeof raw.title === "string" ? raw.title : "会话沉淀",
        content:
          typeof raw.content === "string"
            ? normalizeSedimentPlaybook(raw.content)
            : "",
        status:
          raw.status === "saving" ||
          raw.status === "saved" ||
          raw.status === "error"
            ? raw.status
            : "idle",
        errorText: typeof raw.errorText === "string" ? raw.errorText : undefined,
      };
    default:
      // Unreachable: dedupeMessages filters unknown types before normalize.
      return raw;
  }
}

function dedupeMessages(messages: ChatItem[] | undefined): ChatItem[] {
  if (!Array.isArray(messages)) return [];
  const seen = new Set<string>();
  const out: ChatItem[] = [];
  for (const m of messages) {
    if (!m || typeof m !== "object") continue;
    const type = (m as ChatItem).type;
    if (
      type !== "message" &&
      type !== "tool" &&
      type !== "plan" &&
      type !== "sediment"
    ) {
      continue;
    }
    const normalized = normalizeChatItem(m as ChatItem);
    let id = normalized.id;
    if (seen.has(id)) id = `${id}-${itemId()}`;
    seen.add(id);
    out.push({ ...normalized, id });
  }
  return reorderTurnTimeline(out);
}

/**
 * Within each user turn: plan → tools → assistant answers → other → sediment.
 * Fixes legacy streams where the final bubble was created before tool chips.
 */
function reorderTurnTimeline(messages: ChatItem[]): ChatItem[] {
  const out: ChatItem[] = [];
  let i = 0;
  while (i < messages.length) {
    const m = messages[i];
    if (m.type === "message" && m.role === "user") {
      out.push(m);
      i += 1;
      const body: ChatItem[] = [];
      while (i < messages.length) {
        const n = messages[i];
        if (n.type === "message" && n.role === "user") break;
        body.push(n);
        i += 1;
      }
      out.push(...sortTurnBody(body));
      continue;
    }
    out.push(m);
    i += 1;
  }
  return out;
}

function sortTurnBody(body: ChatItem[]): ChatItem[] {
  const plans: ChatItem[] = [];
  const tools: ChatItem[] = [];
  const assistants: ChatItem[] = [];
  const sediments: ChatItem[] = [];
  const other: ChatItem[] = [];
  for (const m of body) {
    if (m.type === "plan") plans.push(m);
    else if (m.type === "tool") tools.push(m);
    else if (m.type === "sediment") sediments.push(m);
    else if (
      m.type === "message" &&
      m.role === "assistant" &&
      !m.error &&
      !m.stopped
    ) {
      assistants.push(m);
    } else {
      other.push(m);
    }
  }
  return [...plans, ...tools, ...assistants, ...other, ...sediments];
}

/** Repair corrupt localStorage (duplicate session.id / message.id break Svelte each keys). */
function sanitize(data: SessionsStore): SessionsStore {
  const sessions: Record<string, Session> = {};
  for (const [key, session] of Object.entries(data.sessions ?? {})) {
    if (!session || typeof session !== "object") continue;
    const id = key || session.id || uid();
    if (sessions[id]) continue;
    const llmProfileId =
      typeof session.llmProfileId === "string" && session.llmProfileId.trim()
        ? session.llmProfileId.trim()
        : undefined;
    const llmModel =
      typeof session.llmModel === "string" && session.llmModel.trim()
        ? session.llmModel.trim()
        : undefined;
    const workDirPath =
      typeof session.workDirPath === "string" && session.workDirPath.trim()
        ? session.workDirPath.trim()
        : undefined;
    const sedimentCandidate = normalizeSedimentCandidate(session.sedimentCandidate);
    sessions[id] = {
      ...session,
      id,
      title: session.title || "新会话",
      createdAt: session.createdAt || Date.now(),
      updatedAt: session.updatedAt || Date.now(),
      messages: dedupeMessages(session.messages),
      llmProfileId,
      llmModel,
      workDirPath,
      sedimentCandidate,
    };
    // Upgrade titles that were truncated long prompts from known scenes
    const firstUser = sessions[id].messages.find(
      (m) => m.type === "message" && m.role === "user",
    );
    if (firstUser && firstUser.type === "message") {
      const better = summarizeSessionTitle(firstUser.content);
      const matureHit = matchMatureScene(firstUser.content);
      const known =
        RECOMMENDED_SCENES.some((s) => s.title === better) ||
        Boolean(matureHit && matureHit.title === better);
      if (known && better !== sessions[id].title) {
        sessions[id].title = better;
      }
    }
  }
  let current = data.current;
  if (current && !sessions[current]) current = null;
  if (!current) current = Object.keys(sessions)[0] ?? null;
  return { current, sessions };
}

function load(): SessionsStore {
  try {
    const raw = localStorage.getItem(SESSIONS_KEY);
    if (raw) return sanitize(JSON.parse(raw) as SessionsStore);
  } catch {
    /* ignore */
  }
  return emptyStore();
}

/** ADR-036 projection: keep localStorage lean; authority is on-disk Agent session. */
const TOOL_DETAIL_INLINE_MAX = 4096;

function projectSessionsForStorage(data: SessionsStore): SessionsStore {
  const sessions: SessionsStore["sessions"] = {};
  for (const [id, s] of Object.entries(data.sessions)) {
    sessions[id] = {
      ...s,
      messages: s.messages.map((m) => {
        // Images live in memory only — never persist multi-MB data URLs
        // (localStorage quota); the Rust session store keeps the authority.
        if (m.type === "message" && m.images?.length) {
          m = { ...m, images: undefined, imagesStripped: true };
        }
        if (m.type !== "tool" || typeof m.detail !== "string") return m;
        if (m.detail.length <= TOOL_DETAIL_INLINE_MAX) return m;
        return {
          ...m,
          detail: m.detail.slice(0, TOOL_DETAIL_INLINE_MAX) + "\n…",
          summary:
            m.summary && m.summary.trim()
              ? m.summary
              : m.detail.slice(0, 120).replace(/\s+/g, " "),
        };
      }),
    };
  }
  return { current: data.current, sessions };
}

function stripToolDetails(data: SessionsStore): SessionsStore {
  const sessions: SessionsStore["sessions"] = {};
  for (const [id, s] of Object.entries(data.sessions)) {
    sessions[id] = {
      ...s,
      messages: s.messages.map((m) =>
        m.type === "tool" ? { ...m, detail: "" } : m,
      ),
    };
  }
  return { current: data.current, sessions };
}

function persist(data: SessionsStore) {
  const projected = projectSessionsForStorage(data);
  try {
    localStorage.setItem(SESSIONS_KEY, JSON.stringify(projected));
    return;
  } catch (e) {
    console.warn("Failed to save sessions (truncate retry):", e);
  }
  // Level 2: empty tool details for this write only (do not mutate live store).
  try {
    localStorage.setItem(SESSIONS_KEY, JSON.stringify(stripToolDetails(projected)));
  } catch (e) {
    console.warn("Failed to save sessions:", e);
  }
}

/** When true, session writes are debounced (streaming). */
export const deferSessionPersist = writable(false);

let persistTimer: ReturnType<typeof setTimeout> | null = null;

function schedulePersist(_data?: SessionsStore) {
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    persistTimer = null;
    // Always persist the live store — never a stale snapshot from an earlier update.
    persist(get(sessionsData));
  }, 450);
}

export function flushSessionPersist() {
  if (persistTimer) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  persist(get(sessionsData));
  deferSessionPersist.set(false);
}

export const sessionsData = writable<SessionsStore>(load());

/** Re-run sanitize on current store (e.g. before mounting ChatView). */
export function repairSessions() {
  sessionsData.update((data) => sanitize(data));
}

/** Drop all sessions and start fresh (last resort after corrupt history). */
export function resetSessions() {
  const prev = Object.keys(get(sessionsData).sessions);
  for (const id of prev) {
    void import("../ipc").then((ipc) => ipc.clearAgentSession(id).catch(() => {}));
  }
  const id = uid();
  const now = Date.now();
  const llm = defaultSessionLlm(get(config));
  const next: SessionsStore = {
    current: id,
    sessions: {
      [id]: {
        id,
        title: "新会话",
        createdAt: now,
        updatedAt: now,
        messages: [],
        ...llm,
      },
    },
  };
  sessionsData.set(next);
  persist(next);
}

export const currentSessionId = derived(sessionsData, ($d) => $d.current);

export const currentSession = derived(sessionsData, ($d) =>
  $d.current ? ($d.sessions[$d.current] ?? null) : null,
);

export const sessionList = derived(sessionsData, ($d) =>
  Object.values($d.sessions).sort((a, b) => b.updatedAt - a.updatedAt),
);

/**
 * Sessions nested under named workspaces (path match).
 * No path / unmatched sessions attach to the current workspace — never a「未绑定」bucket.
 */
export const sessionsByWorkspace = derived(
  [sessionsData, workspacesData],
  ([$sessions, $workspaces]): WorkspaceSessionGroup[] => {
    const byKey = new Map<string, Session[]>();
    const all = Object.values($sessions.sessions);
    for (const s of all) {
      const key = s.workDirPath?.trim()
        ? normPath(s.workDirPath).toLowerCase()
        : "";
      const list = byKey.get(key) ?? [];
      list.push(s);
      byKey.set(key, list);
    }
    for (const list of byKey.values()) {
      list.sort((a, b) => b.updatedAt - a.updatedAt);
    }

    const items = $workspaces.items;
    if (items.length === 0) return [];

    const claimed = new Set<string>();
    const groups: WorkspaceSessionGroup[] = [];
    for (const w of items) {
      const key = normPath(w.path).toLowerCase();
      claimed.add(key);
      groups.push({
        id: w.id,
        label: w.label,
        path: w.path,
        workspaceId: w.id,
        sessions: byKey.get(key) ?? [],
      });
    }

    // Unmatched paths stay in their own path bucket (never dumped into "current",
    // which previously made chats look like they jumped between projects).
    // Path-less orphans attach only to the current workspace for display.
    const orphans: Session[] = [];
    for (const [key, list] of byKey) {
      if (!key) {
        orphans.push(...list);
        continue;
      }
      if (claimed.has(key)) continue;
      const path = list[0]?.workDirPath?.trim() || key;
      groups.push({
        id: `path:${key}`,
        label: workspaceLabelFromPath(path),
        path,
        workspaceId: null,
        sessions: list,
      });
      claimed.add(key);
    }
    if (orphans.length > 0 && groups.length > 0) {
      const sink =
        groups.find((g) => g.workspaceId === $workspaces.currentId) ?? groups[0];
      sink.sessions = [...sink.sessions, ...orphans].sort(
        (a, b) => b.updatedAt - a.updatedAt,
      );
    }
    return groups;
  },
);

/**
 * Bind orphan sessions and ensure remembered paths exist as named workspaces.
 * Call after workspace bootstrap so the sidebar never needs「未绑定」.
 */
export function reconcileSessionWorkDirs() {
  const live = get(workDir).trim();
  const ws = get(workspacesData);
  const fallback =
    ws.items.find((i) => i.id === ws.currentId)?.path?.trim() ||
    ws.items[0]?.path?.trim() ||
    live;
  if (!fallback && ws.items.length === 0) return;

  const claimed = new Set(
    ws.items.map((i) => normPath(i.path).toLowerCase()),
  );
  const sessions = get(sessionsData).sessions;
  for (const s of Object.values(sessions)) {
    const p = s.workDirPath?.trim();
    if (!p) continue;
    const key = normPath(p).toLowerCase();
    if (!claimed.has(key)) {
      ensureWorkspacePath(p);
      claimed.add(key);
    }
  }

  const nextFallback =
    get(workspacesData).items.find((i) => i.id === get(workspacesData).currentId)
      ?.path?.trim() ||
    get(workspacesData).items[0]?.path?.trim() ||
    live;
  if (!nextFallback) return;

  let changed = false;
  sessionsData.update((data) => {
    for (const s of Object.values(data.sessions)) {
      if (!s.workDirPath?.trim()) {
        s.workDirPath = nextFallback;
        changed = true;
      }
    }
    if (changed) persist(data);
    return data;
  });
}

function update(mutator: (data: SessionsStore) => void) {
  sessionsData.update((data) => {
    mutator(data);
    // New top-level reference so Svelte store subscribers always refresh
    // (in-place nested mutations alone are skipped by safe_not_equal).
    const next: SessionsStore = {
      current: data.current,
      sessions: { ...data.sessions },
    };
    if (get(deferSessionPersist)) schedulePersist(next);
    else persist(next);
    return next;
  });
}

/** Soft cap per workspace path; oldest (empty first) are dropped. */
export const MAX_SESSIONS_PER_WORKSPACE = 40;

function sessionPathKey(s: Session): string {
  return s.workDirPath?.trim()
    ? normPath(s.workDirPath).toLowerCase()
    : "";
}

/** Unused shell: no messages and still the default title. */
export function isEmptyShellSession(s: Session): boolean {
  return (
    (!s.messages || s.messages.length === 0) &&
    (!s.title || s.title === "新会话")
  );
}

function clearAgentCaches(ids: string[]) {
  if (ids.length === 0) return;
  void import("../ipc").then((ipc) => {
    for (const id of ids) {
      void ipc.clearAgentSession(id).catch(() => {});
    }
  });
}

/** Remove ids without auto-creating a replacement session. */
function removeSessions(ids: string[]) {
  if (ids.length === 0) return;
  clearAgentCaches(ids);
  update((data) => {
    for (const id of ids) delete data.sessions[id];
    if (data.current && !data.sessions[data.current]) {
      const rest = Object.values(data.sessions).sort(
        (a, b) => b.updatedAt - a.updatedAt,
      );
      data.current = rest[0]?.id ?? null;
    }
  });
  void gcOrphanPersistedSessions();
}

/**
 * Drop empty shells except the current session.
 * Call on boot so leftover「新会话」from tests / double-clicks do not pile up.
 */
export function pruneEmptyShellSessions() {
  const data = get(sessionsData);
  const current = data.current;
  const victims = Object.values(data.sessions)
    .filter((s) => isEmptyShellSession(s) && s.id !== current)
    .map((s) => s.id);
  removeSessions(victims);
}

/**
 * Keep at most MAX_SESSIONS_PER_WORKSPACE per path key.
 * Prefer deleting empty shells, then oldest by updatedAt. Never delete preserveId.
 */
export function enforceWorkspaceSessionCaps(preserveId?: string | null) {
  const data = get(sessionsData);
  const keep =
    preserveId ??
    data.current ??
    null;
  const byKey = new Map<string, Session[]>();
  for (const s of Object.values(data.sessions)) {
    const key = sessionPathKey(s);
    const list = byKey.get(key) ?? [];
    list.push(s);
    byKey.set(key, list);
  }
  const victims: string[] = [];
  for (const list of byKey.values()) {
    if (list.length <= MAX_SESSIONS_PER_WORKSPACE) continue;
    const ranked = [...list].sort((a, b) => {
      const ae = isEmptyShellSession(a) ? 0 : 1;
      const be = isEmptyShellSession(b) ? 0 : 1;
      if (ae !== be) return ae - be;
      return a.updatedAt - b.updatedAt;
    });
    let over = list.length - MAX_SESSIONS_PER_WORKSPACE;
    for (const s of ranked) {
      if (over <= 0) break;
      if (keep && s.id === keep) continue;
      victims.push(s.id);
      over -= 1;
    }
  }
  removeSessions(victims);
}

/** Boot hygiene: empty shells + per-workspace cap + orphan disk GC. */
export function hygieneSessions() {
  pruneEmptyShellSessions();
  enforceWorkspaceSessionCaps();
  void gcOrphanPersistedSessions();
}

/** Silent A1: drop `.stitch/sessions/<id>` not present in the UI store for each workspace path. */
export async function gcOrphanPersistedSessions(): Promise<void> {
  const data = get(sessionsData);
  const ws = get(workspacesData);
  const paths = new Set<string>();
  for (const w of ws.items) {
    const p = w.path?.trim();
    if (p) paths.add(p);
  }
  const live = get(workDir).trim();
  if (live) paths.add(live);
  if (paths.size === 0) return;
  const ipc = await import("../ipc");
  for (const path of paths) {
    const key = normPath(path).toLowerCase();
    const keepIds = Object.values(data.sessions)
      .filter((s) => {
        const p = s.workDirPath?.trim();
        return p ? normPath(p).toLowerCase() === key : false;
      })
      .map((s) => s.id);
    try {
      await ipc.gcOrphanAgentSessions(path, keepIds);
    } catch {
      /* best-effort */
    }
  }
}

export function createSession(): string {
  const liveDir = get(workDir).trim();
  const ws = get(workspacesData);
  const bindPath =
    liveDir ||
    ws.items.find((i) => i.id === ws.currentId)?.path?.trim() ||
    ws.items[0]?.path?.trim() ||
    "";
  const pathKey = bindPath ? normPath(bindPath).toLowerCase() : "";

  // Reuse an empty shell in the same workspace instead of stacking「新会话」.
  const existing = Object.values(get(sessionsData).sessions).find(
    (s) => isEmptyShellSession(s) && sessionPathKey(s) === pathKey,
  );
  if (existing) {
    switchSession(existing.id);
    return existing.id;
  }

  // Make room before inserting (cap includes the session we are about to add).
  const peers = Object.values(get(sessionsData).sessions).filter(
    (s) => sessionPathKey(s) === pathKey,
  );
  if (peers.length >= MAX_SESSIONS_PER_WORKSPACE) {
    const ranked = [...peers].sort((a, b) => {
      const ae = isEmptyShellSession(a) ? 0 : 1;
      const be = isEmptyShellSession(b) ? 0 : 1;
      if (ae !== be) return ae - be;
      return a.updatedAt - b.updatedAt;
    });
    const need = peers.length - MAX_SESSIONS_PER_WORKSPACE + 1;
    const current = get(sessionsData).current;
    const drop: string[] = [];
    for (const s of ranked) {
      if (drop.length >= need) break;
      if (current && s.id === current) continue;
      drop.push(s.id);
    }
    removeSessions(drop);
  }

  const id = uid();
  const now = Date.now();
  const llm = defaultSessionLlm(get(config));
  const session: Session = {
    id,
    title: "新会话",
    createdAt: now,
    updatedAt: now,
    messages: [],
    ...llm,
    workDirPath: bindPath || undefined,
  };
  update((data) => {
    data.sessions[id] = session;
    data.current = id;
  });
  return id;
}

/** Bind the current session to a project directory. */
export function bindCurrentSessionWorkDir(path: string) {
  const trimmed = path.trim();
  if (!trimmed) return;
  update((data) => {
    const sid = data.current;
    if (!sid || !data.sessions[sid]) return;
    data.sessions[sid].workDirPath = trimmed;
    data.sessions[sid].updatedAt = Date.now();
  });
}

/** Bind the current session to a profile + model (does not change global default). */
export function setSessionLlm(profileId: string, model: string) {
  const pid = profileId.trim();
  const mid = model.trim();
  if (!pid || !mid) return;
  update((data) => {
    const sid = data.current;
    if (!sid || !data.sessions[sid]) return;
    data.sessions[sid].llmProfileId = pid;
    data.sessions[sid].llmModel = mid;
    data.sessions[sid].updatedAt = Date.now();
  });
}

/**
 * Fill missing session LLM fields from active config; if the profile was
 * deleted, fall back to the active profile.
 */
export function ensureSessionLlm() {
  const cfg = get(config);
  const defaults = defaultSessionLlm(cfg);
  const profiles = cfg?.llm_profiles ?? [];
  update((data) => {
    for (const s of Object.values(data.sessions)) {
      let pid = s.llmProfileId?.trim();
      if (pid && profiles.length && !profiles.some((p) => p.id === pid)) {
        pid = undefined;
      }
      if (!pid) {
        s.llmProfileId = defaults.llmProfileId;
        s.llmModel = s.llmModel?.trim() || defaults.llmModel;
      } else if (!s.llmModel?.trim()) {
        const p = profiles.find((x) => x.id === pid);
        s.llmModel = p?.model || defaults.llmModel;
        s.llmProfileId = pid;
      }
    }
  });
}

export function switchSession(id: string) {
  update((data) => {
    if (data.sessions[id]) data.current = id;
  });
}

export function deleteSession(id: string) {
  removeSessions([id]);
  if (!get(sessionsData).current) createSession();
}

/** Rename a session title (sidebar ··· menu). */
export function renameSession(id: string, title: string) {
  const next = title.trim();
  if (!next) return;
  update((data) => {
    const s = data.sessions[id];
    if (!s) return;
    data.sessions[id] = {
      ...s,
      title: next.slice(0, 80),
      updatedAt: Date.now(),
    };
  });
}

export function ensureSession(): string {
  const data = get(sessionsData);
  if (data.current && data.sessions[data.current]) return data.current;
  const ids = Object.keys(data.sessions);
  if (ids.length > 0) {
    switchSession(ids[0]);
    return ids[0];
  }
  return createSession();
}

export function setMessages(messages: ChatItem[], sessionId?: string | null) {
  update((data) => {
    const id = sessionId ?? data.current;
    if (!id || !data.sessions[id]) return;
    const s = data.sessions[id];
    const firstUser = messages.find((m) => m.type === "message" && m.role === "user");
    if (firstUser && firstUser.type === "message" && s.title === "新会话") {
      data.sessions[id] = {
        ...s,
        title: summarizeSessionTitle(firstUser.content),
        messages,
        updatedAt: Date.now(),
      };
      return;
    }
    data.sessions[id] = {
      ...s,
      messages,
      updatedAt: Date.now(),
    };
  });
}

export function appendItem(item: ChatItem, sessionId?: string | null) {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  setMessages([...data.sessions[sid].messages, item], sid);
}

/** Insert `item` immediately before `beforeId` (append if missing). */
export function insertItemBefore(
  beforeId: string,
  item: ChatItem,
  sessionId?: string | null,
) {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  const messages = data.sessions[sid].messages;
  const idx = messages.findIndex((m) => m.id === beforeId);
  if (idx < 0) {
    setMessages([...messages, item], sid);
    return;
  }
  const next = messages.slice();
  next.splice(idx, 0, item);
  setMessages(next, sid);
}

/** Move an existing item to the end of the session timeline. */
export function moveItemToEnd(id: string, sessionId?: string | null) {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  const messages = data.sessions[sid].messages;
  const idx = messages.findIndex((m) => m.id === id);
  if (idx < 0 || idx === messages.length - 1) return;
  const next = messages.slice();
  const [item] = next.splice(idx, 1);
  next.push(item);
  setMessages(next, sid);
}

export function patchItem(id: string, patch: Partial<ChatItem>, sessionId?: string | null) {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  setMessages(
    data.sessions[sid].messages.map((m) => (m.id === id ? ({ ...m, ...patch } as ChatItem) : m)),
    sid,
  );
}

/** Append live output to a running tool's detail (ADR-037). */
export function appendToolDetail(id: string, chunk: string, sessionId?: string | null) {
  if (!chunk) return;
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  setMessages(
    data.sessions[sid].messages.map((m) =>
      m.id === id && m.type === "tool" ? ({ ...m, detail: (m.detail ?? "") + chunk } as ChatItem) : m,
    ),
    sid,
  );
}

export function removeItem(id: string, sessionId?: string | null) {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  setMessages(
    data.sessions[sid].messages.filter((m) => m.id !== id),
    sid,
  );
}

/** Remove every item strictly after `id` (keeps `id`). No-op when missing. */
export function removeItemsAfter(id: string, sessionId?: string | null) {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  const msgs = data.sessions[sid].messages;
  const idx = msgs.findIndex((m) => m.id === id);
  if (idx < 0 || idx === msgs.length - 1) return;
  setMessages(msgs.slice(0, idx + 1), sid);
}

/** Remove `id` and everything after it (edit-resend replaces the turn). */
export function removeItemsFrom(id: string, sessionId?: string | null) {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return;
  const msgs = data.sessions[sid].messages;
  const idx = msgs.findIndex((m) => m.id === id);
  if (idx < 0) return;
  setMessages(msgs.slice(0, idx), sid);
}

export function newMessage(
  role: "user" | "assistant",
  content: string,
  error = false,
  opts?: { stopped?: boolean; images?: string[] },
): ChatItem {
  return {
    id: itemId(),
    type: "message",
    role,
    content,
    error,
    stopped: opts?.stopped,
    images: opts?.images?.length ? opts.images : undefined,
  };
}

export function newTool(name: string, opts?: { recorded?: boolean }): ChatItem {
  return {
    id: itemId(),
    type: "tool",
    name,
    done: false,
    error: false,
    summary: "运行中…",
    detail: "",
    expanded: false,
    startedAt: Date.now(),
    recorded: opts?.recorded,
  };
}

/** User stop / cancel: freeze any still-running tool chips (stop spinner). */
export function markUndoneToolsStopped(sessionId: string | null) {
  if (!sessionId) return;
  const session = get(sessionsData).sessions[sessionId];
  if (!session) return;
  for (const m of session.messages) {
    if (m.type === "tool" && !m.done) {
      patchItem(
        m.id,
        {
          done: true,
          error: false,
          summary: "已停止",
          detail: m.detail || "",
          expanded: false,
        },
        sessionId,
      );
    }
  }
}

/** User stop / cancel: close open plan steps so the card does not stay “进行中”. */
export function markActivePlanInterrupted(sessionId: string | null) {
  const pid = findLatestPlanId(sessionId);
  if (!pid || !sessionId) return;
  const plan = get(sessionsData).sessions[sessionId]?.messages.find(
    (m) => m.id === pid && m.type === "plan",
  );
  if (!plan || plan.type !== "plan") return;
  const baseSteps = Array.isArray(plan.steps) ? plan.steps : [];
  const steps: PlanStep[] = baseSteps.map((s) =>
    s.status === "in_progress"
      ? { ...s, status: "failed" }
      : s.status === "pending"
        ? { ...s, status: "skipped" }
        : s,
  );
  patchItem(pid, { steps }, sessionId);
}

export function newPlan(
  plan: {
    title?: string | null;
    steps: { description: string; status?: string }[];
  },
  planId?: string,
  opts?: { phase?: "proposed" | "approved" | "rejected" },
): ChatItem {
  return {
    id: itemId(),
    type: "plan",
    planId,
    title: (plan.title && String(plan.title).trim()) || "执行计划",
    phase: opts?.phase ?? "proposed",
    steps: (plan.steps || []).map((s) => ({
      description: s.description,
      status: normalizeStepStatus(s.status),
    })),
  };
}

/** Light sediment CTA after a successful agent turn (not sent to the LLM). */
export function newSediment(title: string, content: string): ChatItem {
  return {
    id: itemId(),
    type: "sediment",
    title: title.trim() || "会话沉淀",
    content: content.slice(0, 5000),
    status: "idle",
  };
}

/**
 * After Done: silently store candidate; do not append a stream card (ADR-036).
 * Pass `assistantOverride` when the final Done response is known — streamed
 * tokens alone may still be under the free-chat length gate.
 */
export function prefillSedimentCandidate(
  sessionId?: string | null,
  assistantOverride?: string | null,
): boolean {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]) return false;
  const override = (assistantOverride ?? "").trim();
  const built = override
    ? buildSedimentPayload(sid, { assistantOverride: override, minAssistant: 20 })
    : buildSedimentPayload(sid);
  if (!built) {
    clearSedimentCandidate(sid);
    return false;
  }
  const candidate: SedimentCandidate = {
    title: built.title,
    content: built.content,
    updatedAt: Date.now(),
  };
  update((d) => {
    const s = d.sessions[sid];
    if (!s) return;
    d.sessions[sid] = { ...s, sedimentCandidate: candidate, updatedAt: Date.now() };
  });
  return true;
}

export function clearSedimentCandidate(sessionId?: string | null): void {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  if (!sid || !data.sessions[sid]?.sedimentCandidate) return;
  update((d) => {
    const s = d.sessions[sid];
    if (!s?.sedimentCandidate) return;
    const next = { ...s, updatedAt: Date.now() };
    delete next.sedimentCandidate;
    d.sessions[sid] = next;
  });
}

export function peekSedimentCandidate(
  sessionId?: string | null,
): SedimentCandidate | null {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  const cand = sid ? data.sessions[sid]?.sedimentCandidate : undefined;
  return cand ?? null;
}

/**
 * Roll back authoritative Agent state to a specific checkpoint (ADR-036 S2).
 * Replaces UI projection with a short resume note.
 */
export async function rollbackSessionToCheckpoint(
  sessionId: string,
  targetEpoch: number,
): Promise<{ ok: true; epoch: number } | { ok: false; reason: string }> {
  const sid = sessionId.trim();
  if (!sid) return { ok: false, reason: "无会话" };
  if (!Number.isFinite(targetEpoch) || targetEpoch < 1) {
    return { ok: false, reason: "无效检查点" };
  }
  const ipc = await import("../ipc");
  try {
    const result = await ipc.rollbackSessionEpoch(sid, targetEpoch);
    const resume =
      result.resume_text.trim() ||
      result.summary.trim() ||
      `已回退到检查点 ${result.epoch}`;
    const msg: ChatItem = {
      id: `rollback-${result.epoch}-${Date.now()}`,
      type: "message",
      role: "assistant",
      content: `已回退到检查点 ${result.epoch}。\n\n${resume}`,
    };
    update((data) => {
      const s = data.sessions[sid];
      if (!s) return;
      const next = {
        ...s,
        messages: [msg],
        updatedAt: Date.now(),
      };
      delete next.sedimentCandidate;
      data.sessions[sid] = next;
      data.current = sid;
    });
    return { ok: true, epoch: result.epoch };
  } catch (e) {
    return { ok: false, reason: String(e) };
  }
}

/** @deprecated prefer openCheckpointDialog + rollbackSessionToCheckpoint */
export async function rollbackSessionToPreviousCheckpoint(
  sessionId: string,
): Promise<{ ok: true; epoch: number } | { ok: false; reason: string }> {
  const sid = sessionId.trim();
  if (!sid) return { ok: false, reason: "无会话" };
  const ipc = await import("../ipc");
  const list = await ipc.listSessionCheckpoints(sid);
  if (list.length < 2) {
    return { ok: false, reason: "没有可回退的检查点" };
  }
  return rollbackSessionToCheckpoint(sid, list[1].epoch);
}

/** Session id for the checkpoint picker dialog; null = closed. */
export const checkpointDialogSessionId = writable<string | null>(null);

export function openCheckpointDialog(sessionId: string) {
  const sid = sessionId.trim();
  if (!sid) return;
  checkpointDialogSessionId.set(sid);
}

export function closeCheckpointDialog() {
  checkpointDialogSessionId.set(null);
}

/** Peek newest checkpoint in the session's work dir (excludes this session). */
export async function peekLatestWorkspaceCheckpoint(
  sessionId?: string | null,
): Promise<import("../ipc").WorkspaceCheckpointDto | null> {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  const s = sid ? data.sessions[sid] : null;
  const path = (s?.workDirPath?.trim() || get(workDir).trim());
  if (!path) return null;
  const ipc = await import("../ipc");
  try {
    return await ipc.latestWorkspaceCheckpoint(path, sid);
  } catch {
    return null;
  }
}

/**
 * Opt-in A2: inject previous workspace checkpoint summary into an empty session.
 */
export async function applyLatestWorkspaceCheckpoint(
  sessionId: string,
): Promise<{ ok: true; epoch: number } | { ok: false; reason: string }> {
  const sid = sessionId.trim();
  if (!sid) return { ok: false, reason: "无会话" };
  const data = get(sessionsData);
  const s = data.sessions[sid];
  if (!s) return { ok: false, reason: "会话不存在" };
  if (s.messages?.length) return { ok: false, reason: "会话已有内容" };
  const ref = await peekLatestWorkspaceCheckpoint(sid);
  if (!ref) return { ok: false, reason: "没有可载入的检查点" };
  const body =
    ref.resume_text.trim() ||
    ref.summary_preview.trim() ||
    `检查点 #${ref.epoch}`;
  const msg: ChatItem = {
    id: `checkpoint-import-${ref.epoch}-${Date.now()}`,
    type: "message",
    role: "assistant",
    content: `已载入上一检查点 #${ref.epoch}。\n\n${body}`,
  };
  update((d) => {
    const cur = d.sessions[sid];
    if (!cur) return;
    d.sessions[sid] = {
      ...cur,
      messages: [msg],
      updatedAt: Date.now(),
    };
    d.current = sid;
  });
  return { ok: true, epoch: ref.epoch };
}

/** Build prompt body from the latest user + assistant pair in a session. */
export function buildSedimentPayload(
  sessionId?: string | null,
  opts?: { assistantOverride?: string; minAssistant?: number },
): {
  title: string;
  content: string;
} | null {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  const session = sid ? data.sessions[sid] : null;
  if (!session) return null;
  let userText = "";
  let assistantText = (opts?.assistantOverride ?? "").trim();
  for (let i = session.messages.length - 1; i >= 0; i--) {
    const m = session.messages[i];
    if (m.type !== "message" || m.error || m.stopped) continue;
    if (!assistantText && m.role === "assistant" && m.content.trim()) {
      assistantText = m.content.trim();
      continue;
    }
    if (assistantText && m.role === "user" && m.content.trim()) {
      userText = m.content.trim();
      break;
    }
  }
  if (!userText || !assistantText) return null;

  // Official mature scenes → short replay playbook (not full chat dump).
  const mature = matchMatureScene(userText);
  if (mature) {
    if (assistantText.length < 20) return null;
    return buildMatureSediment(mature, assistantText);
  }

  // Skip trivial / empty-feeling turns for free chat
  const minLen = opts?.minAssistant ?? 40;
  if (assistantText.length < minLen) return null;

  // Free chat: short title + compact body (strip fences so sediment isn't a prompt dump).
  const taskOneLiner = userText
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 72);
  const title =
    (session.title && session.title !== "新会话"
      ? session.title.replace(/\s+/g, " ").trim().slice(0, 80)
      : taskOneLiner) || "Stitch 会话";
  const taskBrief = userText
    .replace(/```[\s\S]*?```/g, "\n[代码块]\n")
    .replace(/\s+\n/g, "\n")
    .trim()
    .slice(0, 400);
  const content = [
    "## 任务",
    taskBrief,
    "",
    "## 结果",
    assistantText.replace(/\s+/g, " ").trim().slice(0, 800),
  ]
    .join("\n")
    .slice(0, 5000);
  return { title, content };
}

/**
 * Build LLM history from current session (text messages only).
 * Aborted turns (user → stopped assistant) are dropped entirely so the next
 * send does not look like consecutive user messages continuing the old task.
 */
export function historyForSend(sessionId?: string | null): import("../types").HistoryMessage[] {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  const session = sid ? data.sessions[sid] : null;
  if (!session) return [];

  const msgs = session.messages.filter(
    (m): m is Extract<ChatItem, { type: "message" }> => m.type === "message",
  );
  const skipIds = new Set<string>();
  for (let i = 0; i < msgs.length; i++) {
    const m = msgs[i];
    if (m.role !== "user") continue;
    let nextAsst: (typeof msgs)[number] | null = null;
    for (let j = i + 1; j < msgs.length; j++) {
      if (msgs[j].role === "user") break;
      if (msgs[j].role === "assistant") {
        nextAsst = msgs[j];
        break;
      }
    }
    if (nextAsst?.stopped) skipIds.add(m.id);
  }

  return msgs
    .filter((m) => !skipIds.has(m.id))
    // Image-only messages carry no text — keep them when they have images.
    .filter(
      (m) =>
        !m.error &&
        !m.stopped &&
        (m.content.trim().length > 0 || (m.images?.length ?? 0) > 0),
    )
    .map((m) => ({
      role: m.role,
      // Strip UI stop suffix if any older messages still have it.
      content: m.content.replace(/\n\n— 已停止生成\s*$/, "").trim(),
      images: m.images?.length ? m.images : undefined,
    }))
    .filter((m) => m.content.length > 0 || (m.images?.length ?? 0) > 0);
}

function normalizeStepStatus(s?: string): import("../types").PlanStepStatus {
  if (s === "in_progress" || s === "done" || s === "skipped" || s === "failed") return s;
  return "pending";
}

/** Latest plan item in the target session (for step updates). */
export function findLatestPlanId(sessionId?: string | null): string | null {
  const data = get(sessionsData);
  const sid = sessionId ?? data.current;
  const session = sid ? data.sessions[sid] : null;
  if (!session) return null;
  for (let i = session.messages.length - 1; i >= 0; i--) {
    const m = session.messages[i];
    if (m.type === "plan") return m.id;
  }
  return null;
}
