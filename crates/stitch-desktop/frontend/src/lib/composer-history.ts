/**
 * Composer send history (↑ recalls previously sent messages, like a shell).
 * Global across sessions; deduped, most-recent-first.
 */

const KEY = "stitch-composer-history";
const MAX = 50;

export function loadComposerHistory(): string[] {
    try {
        const raw = localStorage.getItem(KEY);
        const arr: unknown = raw ? JSON.parse(raw) : [];
        if (!Array.isArray(arr)) return [];
        return arr.filter((x): x is string => typeof x === "string" && x.trim().length > 0);
    } catch {
        return [];
    }
}

export function pushComposerHistory(text: string): void {
    const t = text.trim();
    if (!t) return;
    const list = [t, ...loadComposerHistory().filter((x) => x !== t)].slice(0, MAX);
    try {
        localStorage.setItem(KEY, JSON.stringify(list));
    } catch {
        /* storage full / private mode — history is best-effort */
    }
}
