<script lang="ts">
  import { formatBytes } from '../lib/format';

  let { preview, fileCount }: { preview: [string, number][]; fileCount: number } = $props();

  /** Shown before "show all"; the rest of the preview stays folded. */
  const FOLDED = 5;
  let open = $state(false);
  const shown = $derived(open ? preview : preview.slice(0, FOLDED));
  const hidden = $derived(fileCount - shown.length);
</script>

<ul class="space-y-0.5 text-xs">
  {#each shown as [path, size] (path)}
    <li class="flex justify-between gap-4">
      <span class="truncate font-mono" title={path}>{path}</span>
      <span class="shrink-0 text-muted">{formatBytes(size)}</span>
    </li>
  {/each}
</ul>
{#if hidden > 0}
  {#if !open && preview.length > FOLDED}
    <button class="mt-1 text-xs text-accent" onclick={() => (open = true)}>show more</button>
  {:else}
    <p class="mt-1 text-xs text-muted">and {hidden} more</p>
  {/if}
{/if}
