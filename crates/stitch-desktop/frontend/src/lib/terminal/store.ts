import { derived, writable } from "svelte/store";
import { currentSession } from "../stores/sessions";
import type { ChatItem } from "../types";

export type TerminalEntry = {
  id: string;
  summary: string;
  detail: string;
  error: boolean;
  done: boolean;
};

/** Whether the dedicated terminal drawer is open. */
export const terminalOpen = writable(false);

function isShellTool(item: ChatItem): item is Extract<ChatItem, { type: "tool" }> {
  return item.type === "tool" && item.name === "run_command";
}

/** Shell tool cards from the active session (newest last). */
export const terminalEntries = derived(currentSession, ($session): TerminalEntry[] => {
  if (!$session?.messages?.length) return [];
  return $session.messages.filter(isShellTool).map((t) => ({
    id: t.id,
    summary: t.summary || "",
    detail: t.detail || "",
    error: !!t.error,
    done: !!t.done,
  }));
});

export function toggleTerminal() {
  terminalOpen.update((v) => !v);
}

export function openTerminal() {
  terminalOpen.set(true);
}
