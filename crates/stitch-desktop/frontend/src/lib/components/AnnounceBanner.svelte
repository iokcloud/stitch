<script lang="ts">
  import * as ipc from "../ipc";
  import { announceStore } from "../stores/announce.svelte";
</script>

{#if announceStore.phase === "visible" && announceStore.item}
  <div class="announce-banner" data-testid="announce-banner" role="status" aria-live="polite">
    <div class="announce-banner-body">
      <div class="announce-banner-title">{announceStore.item.title}</div>
      <div class="announce-banner-text" data-testid="announce-banner-text" title={announceStore.item.body}>
        {announceStore.item.body}
      </div>
    </div>
    <div class="announce-banner-actions">
      {#if announceStore.item.url}
        <button
          type="button"
          class="btn-primary"
          data-testid="announce-banner-open"
          onclick={() => {
            void ipc
              .openExternalUrl(announceStore.item!.url!)
              .catch(() => {});
            announceStore.markRead();
          }}
        >
          查看详情
        </button>
      {/if}
      <button
        type="button"
        class="btn-ghost"
        data-testid="announce-banner-read"
        onclick={() => announceStore.markRead()}
      >
        知道了
      </button>
      <button
        type="button"
        class="btn-ghost"
        data-testid="announce-banner-later"
        onclick={() => announceStore.dismiss()}
      >
        稍后
      </button>
    </div>
  </div>
{/if}
