<script lang="ts">
  import { config, fillComposer } from "../stores/app";
import { stream } from "../stores/stream.svelte";
  import { nav } from "../nav.svelte";
  import { patchItem, removeItem, clearSedimentCandidate } from "../stores/sessions";
  import { matureSceneByTitle, normalizeSedimentPlaybook } from "../mature-scenes";
  import * as ipc from "../ipc";

  interface Props {
    id: string;
    title: string;
    content: string;
    status?: "idle" | "saving" | "saved" | "error";
    errorText?: string;
    promptId?: string;
  }

  let {
    id,
    title,
    content,
    status = "idle",
    errorText = "",
    promptId: _promptId = "",
  }: Props = $props();

  const tokenReady = $derived(!!$config?.api_token_set);
  const submitExplore = $derived(($config?.sediment_visibility ?? "explore") !== "personal");
  const playbook = $derived(normalizeSedimentPlaybook(content));
  const mature = $derived(matureSceneByTitle(title));
  /** Playbook-style sediment only (mature scenes). Free-chat dumps stay title+actions. */
  const preview = $derived.by(() => {
    if (!playbook.trimStart().startsWith("# ")) return "";
    return playbook
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean)
      .slice(0, 10)
      .join("\n")
      .slice(0, 420);
  });
  let copyHint = $state("");
  let savedExplore = $state(false);

  async function copyLocal() {
    try {
      await navigator.clipboard.writeText(playbook);
      copyHint = "已复制";
      void ipc.trackUsage("stitch_sediment_copy");
      setTimeout(() => {
        copyHint = "";
      }, 1500);
    } catch {
      copyHint = "复制失败";
    }
  }

  async function saveCloud() {
    if (!tokenReady) {
      nav.showSettings({ fromChat: true, tab: "account" });
      return;
    }
    patchItem(id, { status: "saving", errorText: "" });
    savedExplore = false;
    try {
      const created = await ipc.createPrompt({
        title: title.slice(0, 255),
        content: playbook.slice(0, 5000),
        description: "来自 Stitch 会话",
        tags: ["stitch"],
      });
      if (submitExplore) {
        await ipc.submitExplore(created.id);
        savedExplore = true;
      }
      patchItem(id, {
        status: "saved",
        promptId: created.id || "",
        errorText: "",
      });
      clearSedimentCandidate();
      void ipc.trackUsage("stitch_sediment_save", {
        kind: "ok",
        explore: submitExplore ? "1" : "0",
      });
    } catch (e) {
      patchItem(id, { status: "error", errorText: String(e) });
      void ipc.trackUsage("stitch_sediment_save", { kind: "fail" });
    }
  }

  function rerun() {
    if (!mature || stream.isStreaming) return;
    fillComposer(mature.prompt);
  }

  function connectAccount() {
    nav.showSettings({ fromChat: true, tab: "account" });
  }

  function dismiss() {
    removeItem(id);
  }

  const saveLabel = $derived(
    status === "saving"
      ? "保存中…"
      : submitExplore
        ? "保存并提交公开"
        : "保存到个人库",
  );
</script>

<div class="sediment-card" data-testid="sediment-card">
  <div class="sediment-card-head">
    <p class="sediment-card-label">存成提示词</p>
    <button type="button" class="icon-btn" aria-label="关闭" title="关闭" onclick={dismiss}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path d="M18 6L6 18M6 6l12 12" />
      </svg>
    </button>
  </div>
  <p class="sediment-card-title">{title}</p>
  {#if preview}
    <pre class="sediment-card-preview" data-testid="sediment-preview">{preview}</pre>
  {/if}
  {#if status === "saved"}
    <p class="sediment-card-desc" data-testid="sediment-saved">
      {savedExplore ? "已保存并提交公开审核" : "已保存到个人库"}
    </p>
  {:else if status === "error" && errorText}
    <p class="sediment-card-error" data-testid="sediment-error">{errorText}</p>
  {/if}
  <div class="sediment-card-actions">
    {#if !tokenReady && status !== "saved"}
      <button type="button" class="sediment-action" data-testid="sediment-copy" onclick={() => void copyLocal()}>
        {copyHint || "复制"}
      </button>
      <button type="button" class="sediment-action is-muted" data-testid="sediment-connect" onclick={connectAccount}>
        连接账号
      </button>
    {:else if status !== "saved"}
      <button
        type="button"
        class="sediment-action is-emphasis"
        data-testid="sediment-save"
        disabled={status === "saving"}
        onclick={() => void saveCloud()}
      >
        {saveLabel}
      </button>
      <button type="button" class="sediment-action" data-testid="sediment-copy" onclick={() => void copyLocal()}>
        {copyHint || "复制"}
      </button>
    {:else}
      <button type="button" class="sediment-action" data-testid="sediment-copy" onclick={() => void copyLocal()}>
        {copyHint || "复制"}
      </button>
    {/if}
    {#if mature}
      <button
        type="button"
        class="sediment-action is-muted"
        data-testid="sediment-rerun"
        disabled={stream.isStreaming}
        onclick={rerun}
      >
        再跑
      </button>
    {/if}
  </div>
</div>
