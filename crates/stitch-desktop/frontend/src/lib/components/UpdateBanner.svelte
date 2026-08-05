<script lang="ts">
  import { updateStore } from "../stores/update.svelte";
</script>

{#if updateStore.phase === "available" || updateStore.phase === "installing"}
  <div class="update-banner" data-testid="update-banner" role="status" aria-live="polite">
    <div class="update-banner-body">
      <div class="update-banner-title">新版本 v{updateStore.latest} 可用</div>
      {#if updateStore.notes}
        <div class="update-banner-notes" data-testid="update-banner-notes" title={updateStore.notes}
          >{updateStore.notes}</div
        >
      {/if}
    </div>
    <div class="update-banner-actions">
      <button
        type="button"
        class="btn-primary"
        data-testid="update-banner-install"
        disabled={updateStore.phase === "installing"}
        onclick={() => void updateStore.install()}
      >
        {updateStore.phase === "installing" ? "正在安装更新…" : "更新"}
      </button>
      <button
        type="button"
        class="btn-ghost"
        data-testid="update-banner-later"
        onclick={() => updateStore.dismiss()}
      >
        稍后
      </button>
    </div>
  </div>
{/if}
