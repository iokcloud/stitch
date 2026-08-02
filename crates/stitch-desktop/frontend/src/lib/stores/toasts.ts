import { writable } from "svelte/store";

export type ToastKind = "info" | "error";

export interface ToastItem {
    id: number;
    kind: ToastKind;
    text: string;
}

let nextId = 1;

export const toasts = writable<ToastItem[]>([]);

/** Lightweight global notice — auto-dismiss; keep at most 3 stacked. */
export function pushToast(text: string, kind: ToastKind = "info", ttlMs = 4200): void {
    const trimmed = text.trim();
    if (!trimmed) return;
    const id = nextId++;
    toasts.update((list) => [...list.slice(-2), { id, kind, text: trimmed }]);
    setTimeout(() => dismissToast(id), ttlMs);
}

export function dismissToast(id: number): void {
    toasts.update((list) => list.filter((t) => t.id !== id));
}
