/**
 * Desktop-native context menus via Tauri Menu.popup.
 * Always suppress the WebView browser menu; only popup when running under Tauri.
 */

import { LogicalPosition } from "@tauri-apps/api/dpi";
import { Menu } from "@tauri-apps/api/menu/menu";
import { MenuItem } from "@tauri-apps/api/menu/menuItem";
import { PredefinedMenuItem } from "@tauri-apps/api/menu/predefinedMenuItem";

export type SessionContextHandlers = {
  copyTitle: (sessionId: string, title: string) => void;
  startRename: (sessionId: string, title: string) => void;
  rollback: (sessionId: string) => void;
  delete: (sessionId: string) => void;
};

export type WorkspaceContextHandlers = {
  openFolder: (workspaceId: string) => void;
  remove: (workspaceId: string) => void;
};

let sessionHandlers: SessionContextHandlers | null = null;
let workspaceHandlers: WorkspaceContextHandlers | null = null;

export function registerSessionContextHandlers(
  handlers: SessionContextHandlers | null,
) {
  sessionHandlers = handlers;
}

export function registerWorkspaceContextHandlers(
  handlers: WorkspaceContextHandlers | null,
) {
  workspaceHandlers = handlers;
}

function isTauriRuntime(): boolean {
  return (
    typeof window !== "undefined" &&
    ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  );
}

function isEditableTarget(el: EventTarget | null): el is HTMLElement {
  if (!(el instanceof HTMLElement)) return false;
  const tag = el.tagName;
  if (tag === "TEXTAREA" || tag === "INPUT") {
    const input = el as HTMLInputElement | HTMLTextAreaElement;
    if (tag === "INPUT") {
      const type = (input as HTMLInputElement).type || "text";
      if (
        ["button", "checkbox", "radio", "submit", "reset", "file", "range", "color", "hidden"].includes(
          type,
        )
      ) {
        return false;
      }
    }
    return !input.disabled && !input.readOnly;
  }
  return el.isContentEditable;
}

function selectedText(): string {
  try {
    return window.getSelection()?.toString() ?? "";
  } catch {
    return "";
  }
}

async function popupAt(menu: Menu, e: MouseEvent) {
  await menu.popup(new LogicalPosition(e.clientX, e.clientY));
}

async function showEditMenu(e: MouseEvent) {
  const menu = await Menu.new({
    items: [
      await PredefinedMenuItem.new({ item: "Cut", text: "剪切" }),
      await PredefinedMenuItem.new({ item: "Copy", text: "复制" }),
      await PredefinedMenuItem.new({ item: "Paste", text: "粘贴" }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "SelectAll", text: "全选" }),
    ],
  });
  await popupAt(menu, e);
}

async function showSelectionCopyMenu(e: MouseEvent, text: string) {
  const menu = await Menu.new({
    items: [
      await MenuItem.new({
        id: "copy-selection",
        text: "复制",
        action: () => {
          void navigator.clipboard.writeText(text).catch(() => {
            /* ignore */
          });
        },
      }),
    ],
  });
  await popupAt(menu, e);
}

async function showSessionMenu(
  e: MouseEvent,
  sessionId: string,
  title: string,
  streaming: boolean,
) {
  const h = sessionHandlers;
  if (!h) return;

  const items: Array<
    Awaited<ReturnType<typeof MenuItem.new>> | Awaited<ReturnType<typeof PredefinedMenuItem.new>>
  > = [
    await MenuItem.new({
      id: "session-copy",
      text: "复制",
      action: () => h.copyTitle(sessionId, title),
    }),
  ];

  if (!streaming) {
    items.push(
      await MenuItem.new({
        id: "session-rename",
        text: "重命名",
        action: () => h.startRename(sessionId, title),
      }),
      await MenuItem.new({
        id: "session-rollback",
        text: "回退检查点",
        action: () => h.rollback(sessionId),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({
        id: "session-delete",
        text: "删除",
        action: () => h.delete(sessionId),
      }),
    );
  }

  const menu = await Menu.new({ items });
  await popupAt(menu, e);
}

async function showWorkspaceMenu(e: MouseEvent, workspaceId: string) {
  const h = workspaceHandlers;
  if (!h) return;

  const menu = await Menu.new({
    items: [
      await MenuItem.new({
        id: "workspace-open",
        text: "打开文件夹",
        action: () => h.openFolder(workspaceId),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({
        id: "workspace-remove",
        text: "移除工作区",
        action: () => h.remove(workspaceId),
      }),
    ],
  });
  await popupAt(menu, e);
}

function onContextMenu(e: MouseEvent) {
  e.preventDefault();
  e.stopPropagation();

  if (!isTauriRuntime()) return;

  const target = e.target;
  void (async () => {
    try {
      if (isEditableTarget(target)) {
        await showEditMenu(e);
        return;
      }

      const el = target instanceof Element ? target : null;
      const sessionRow = el?.closest?.("[data-ctx='session']") as HTMLElement | null;
      if (sessionRow) {
        const sessionId = sessionRow.dataset.sessionId?.trim() ?? "";
        const title = sessionRow.dataset.sessionTitle ?? "";
        const streaming = sessionRow.dataset.sessionStreaming === "1";
        if (sessionId) {
          await showSessionMenu(e, sessionId, title, streaming);
          return;
        }
      }

      const workspaceRow = el?.closest?.(
        "[data-ctx='workspace']",
      ) as HTMLElement | null;
      if (workspaceRow) {
        const workspaceId = workspaceRow.dataset.workspaceEntryId?.trim() ?? "";
        if (workspaceId) {
          await showWorkspaceMenu(e, workspaceId);
          return;
        }
      }

      const text = selectedText().trim();
      if (text) {
        await showSelectionCopyMenu(e, text);
      }
    } catch (err) {
      console.warn("[stitch] native context menu failed", err);
    }
  })();
}

/** Install capture-phase handler. Returns disposer. */
export function installNativeContextMenu(): () => void {
  document.addEventListener("contextmenu", onContextMenu, true);
  return () => document.removeEventListener("contextmenu", onContextMenu, true);
}
