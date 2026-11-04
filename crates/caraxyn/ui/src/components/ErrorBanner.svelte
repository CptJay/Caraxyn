<script lang="ts">
  let { message, onclose }: { message: string; onclose: () => void } = $props();
</script>

<div role="alert" class="flex items-start gap-3 rounded-md border border-danger/40 bg-danger/10 px-3 py-2 text-sm">
  <p class="flex-1">{message}</p>
  <button class="text-muted hover:text-fg" aria-label="Dismiss" onclick={onclose}>✕</button>
</div>
