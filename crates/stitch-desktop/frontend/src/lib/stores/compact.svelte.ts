/**
 * Compact overlay controller — the window morphs into a 420x64 floating bar.
 *
 * Two modes:
 * - auto: a desktop tool starts → enter(); the turn ending schedules the
 *   restore after a short「已完成」linger (scheduleExit).
 * - pinned (manual): toggled by hotkey / command palette — the bar stays
 *   until the user switches back; turns never auto-restore.
 */
import { get } from "svelte/store";
import { lastUserMessage } from "./app";
import { toolLabel } from "../output-format";
import { setCompactMode } from "../ipc";

class CompactController {
  /** Overlay active (window morphed to the bar). */
  mode = $state(false);
  /** User pinned the bar manually — turns do not auto-restore. */
  pinned = $state(false);
  /** Current desktop tool running (label + stopwatch source). */
  tool = $state<string | null>(null);
  /** Turn finished — bar holds a visible「已完成」state before restoring. */
  finished = $state(false);
  /** Turn-level stopwatch start: when the overlay engaged. */
  since = $state(0);
  elapsedMs = $state(0);

  #exitTimer: ReturnType<typeof setTimeout> | null = null;

  async enter(opts: { pinned?: boolean } = {}): Promise<void> {
    if (this.mode) {
      if (opts.pinned) this.pinned = true;
      return;
    }
    this.mode = true;
    this.since = Date.now();
    if (opts.pinned) this.pinned = true;
    document.documentElement.setAttribute("data-compact", "true");
    try {
      await setCompactMode(true);
    } catch {
      /* mock / non-tauri — ok */
    }
  }

  async exit(): Promise<void> {
    if (this.#exitTimer) {
      clearTimeout(this.#exitTimer);
      this.#exitTimer = null;
    }
    this.finished = false;
    this.pinned = false;
    if (!this.mode) return;
    this.mode = false;
    document.documentElement.removeAttribute("data-compact");
    try {
      await setCompactMode(false);
    } catch {
      /* mock / non-tauri — ok */
    }
  }

  /** Hold the bar in a visible「已完成」state, then restore the window. */
  scheduleExit(delayMs = 2600): void {
    if (!this.mode || this.pinned) return;
    this.finished = true;
    if (this.#exitTimer) clearTimeout(this.#exitTimer);
    this.#exitTimer = setTimeout(() => {
      this.#exitTimer = null;
      void this.exit();
    }, delayMs);
  }

  /** A new turn starts — drop any lingering done state immediately. */
  beginRun(): void {
    if (this.#exitTimer) {
      clearTimeout(this.#exitTimer);
      this.#exitTimer = null;
    }
    this.finished = false;
  }

  /** Turn ended — clear the execution state (pinned keeps the bar). */
  clearRun(): void {
    this.tool = null;
    this.finished = false;
  }

  /** Manual switch: full window → bar (pinned); auto bar → pin it so the
   * turn finishing keeps the bar; pinned bar → back to the full window. */
  async toggle(): Promise<void> {
    if (this.mode && this.pinned) {
      await this.exit();
    } else if (this.mode) {
      this.pinned = true;
    } else {
      await this.enter({ pinned: true });
    }
  }
}

export const compact = new CompactController();

/** One-line bar label: execution state first, manual state second.
 * A function (not $derived) — module-level derived exports are invalid;
 * template call sites still track the state reads inside. */
export function compactLabel(): string {
  if (compact.finished) return "已完成";
  if (compact.tool) return `正在执行 ${toolLabel(compact.tool)}`;
  if (compact.pinned) return "紧凑模式";
  const msg = (get(lastUserMessage) || "").trim();
  if (!msg) return "Stitch 正在执行桌面操作";
  return `正在执行：${msg.length > 36 ? `${msg.slice(0, 36)}…` : msg}`;
}
