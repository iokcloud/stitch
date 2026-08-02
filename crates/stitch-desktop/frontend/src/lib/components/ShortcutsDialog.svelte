<script lang="ts">
  import { shortcutsOpen } from "../stores/palette";
  import { SHORTCUTS } from "../shortcuts";

  function close() {
    shortcutsOpen.set(false);
  }
</script>

{#if $shortcutsOpen}
  <div
    class="modal-overlay"
    data-testid="shortcuts-dialog"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div class="modal-panel shortcuts-panel" role="dialog" aria-modal="true" aria-label="键盘快捷键">
      <div class="shortcuts-head">
        <h2>键盘快捷键</h2>
        <button type="button" class="icon-btn" aria-label="关闭" onclick={close}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
      <ul class="shortcuts-list">
        {#each SHORTCUTS as s (s.keys)}
          <li class="shortcuts-row">
            <span class="shortcuts-desc">{s.description}</span>
            <kbd>{s.keys}</kbd>
          </li>
        {/each}
      </ul>
    </div>
  </div>
{/if}
