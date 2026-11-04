<script lang="ts">
  import type { Snippet } from 'svelte';

  // Same height on every page, with or without buttons, so titles line up.
  let { title, children }: { title: string; children?: Snippet } = $props();
</script>

<header class="mb-4 flex h-8 items-center justify-between gap-4">
  <h1 class="text-lg font-semibold">{title}</h1>
  {#if children}
    <div class="flex gap-2">{@render children()}</div>
  {/if}
</header>
