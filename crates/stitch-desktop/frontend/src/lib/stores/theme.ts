import { writable, get, derived } from "svelte/store";
import type { Theme, ThemePreference } from "../types";
import { THEME_KEY } from "../types";
import { setTitlebarTheme } from "../ipc";

function systemPrefersDark(): boolean {
  try {
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  } catch {
    return false;
  }
}

function readPreference(): ThemePreference {
  try {
    const t = localStorage.getItem(THEME_KEY);
    if (t === "light" || t === "dark" || t === "system") return t;
  } catch {
    /* ignore */
  }
  // Default: follow OS (better first-run than hard-coding light).
  return "system";
}

function resolve(pref: ThemePreference): Theme {
  if (pref === "system") return systemPrefersDark() ? "dark" : "light";
  return pref;
}

export const themePreference = writable<ThemePreference>(readPreference());

/** Resolved light/dark actually applied to the shell. */
export const theme = derived(themePreference, (pref) => resolve(pref));

let mediaCleanup: (() => void) | null = null;

function bindSystemListener(pref: ThemePreference) {
  mediaCleanup?.();
  mediaCleanup = null;
  if (pref !== "system" || typeof window === "undefined") return;
  try {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    mediaCleanup = () => mq.removeEventListener("change", onChange);
  } catch {
    /* ignore */
  }
}

export function applyTheme(pref: ThemePreference) {
  const resolved = resolve(pref);
  document.documentElement.setAttribute("data-theme", resolved);
  try {
    localStorage.setItem(THEME_KEY, pref);
  } catch {
    /* ignore */
  }
  themePreference.set(pref);
  bindSystemListener(pref);
  void setTitlebarTheme(resolved === "dark");
}

/** Cycle: system → light → dark → system. */
export function toggleTheme() {
  const cur = get(themePreference);
  const next: ThemePreference =
    cur === "system" ? "light" : cur === "light" ? "dark" : "system";
  applyTheme(next);
}

export function initTheme() {
  applyTheme(readPreference());
}

export function themeAriaLabel(pref: ThemePreference): string {
  if (pref === "system") return "主题：跟随系统";
  if (pref === "dark") return "主题：深色";
  return "主题：浅色";
}
