<script lang="ts">
  import type { Snippet } from 'svelte';

  // One block on the Settings page: a heading above a box with its controls.
  let { title, children }: { title: string; children: Snippet } = $props();
</script>

<section class="max-w-xl space-y-2">
  <h2 class="px-1 text-sm font-medium text-muted">{title}</h2>
  <div class="card space-y-4">
    {@render children()}
  </div>
</section>
