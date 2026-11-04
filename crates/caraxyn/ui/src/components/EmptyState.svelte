<script lang="ts">
  import type { Snippet } from 'svelte';

  let { title, hint, children }: { title: string; hint?: string; children?: Snippet } = $props();
</script>

<div class="flex flex-col items-center gap-2 rounded-xl border border-dashed border-outline/50 px-6 py-12 text-center">
  <p class="font-medium">{title}</p>
  {#if hint}<p class="max-w-sm text-sm text-muted">{hint}</p>{/if}
  {@render children?.()}
</div>
