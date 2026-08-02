<script lang="ts">
  import "../app.css";

  let { children } = $props();
</script>

<div
  class="relative flex flex-col flex-1 min-h-0 h-full w-full overflow-hidden bg-[var(--color-background)]"
  data-testid="layout-root"
>
  {@render children()}
</div>
