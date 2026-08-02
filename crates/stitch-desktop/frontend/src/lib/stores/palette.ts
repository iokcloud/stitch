import { get, writable } from "svelte/store";
import { isStreaming } from "./app";

export const paletteOpen = writable(false);
export const shortcutsOpen = writable(false);
/** In-session find bar (Ctrl+F). */
export const chatFindOpen = writable(false);

export function togglePalette(): void {
    paletteOpen.update((v) => !v);
}

export function toggleShortcuts(): void {
    shortcutsOpen.update((v) => !v);
}

/** Hand focus back to the composer next frame — unless another overlay
 *  opened in the meantime, or the composer is gone (settings view). */
export function refocusComposerSoon(): void {
    requestAnimationFrame(() => {
        if (get(paletteOpen) || get(chatFindOpen) || get(shortcutsOpen)) return;
        const el = document.getElementById("chat-input");
        if (el instanceof HTMLTextAreaElement && !el.disabled) el.focus();
    });
}

// Deferred refocus: when an overlay closes while the composer is disabled
// (stream in progress), remember to refocus after the stream ends.
let pendingRefocus = false;

isStreaming.subscribe((streaming) => {
    if (!streaming && pendingRefocus) {
        pendingRefocus = false;
        refocusComposerSoon();
    }
});

// Focus restore: closing any of the three global overlays (by any path —
// Esc, backdrop click, close button) returns focus to the composer.
// If the composer is currently disabled (stream in progress), defer the
// refocus until the stream finishes — otherwise it's silently skipped.
if (typeof window !== "undefined") {
    for (const store of [paletteOpen, chatFindOpen, shortcutsOpen]) {
        let prev = false;
        store.subscribe((open) => {
            if (prev && !open) {
                if (get(isStreaming)) {
                    pendingRefocus = true;
                } else {
                    refocusComposerSoon();
                }
            }
            prev = open;
        });
    }
}
