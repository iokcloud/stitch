<script lang="ts">
  import { toasts, dismissToast } from "../stores/toasts";
</script>

{#if $toasts.length > 0}
  <div class="toast-stack" data-testid="toast-stack" role="status" aria-live="polite">
    {#each $toasts as toast (toast.id)}
      <div class="toast" class:is-error={toast.kind === "error"} data-toast-kind={toast.kind}>
        <span class="toast-text">{toast.text}</span>
        <button
          type="button"
          class="toast-x"
          aria-label="关闭"
          onclick={() => dismissToast(toast.id)}
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
    {/each}
  </div>
{/if}
