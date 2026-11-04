<script lang="ts">
  let { message, onclose }: { message: string; onclose: () => void } = $props();
</script>

<div role="alert" class="flex items-start gap-3 rounded-xl bg-danger-container px-4 py-3 text-sm text-on-danger-container">
  <p class="flex-1">{message}</p>
  <button class="opacity-80 hover:opacity-100" aria-label="Dismiss" onclick={onclose}>✕</button>
</div>
