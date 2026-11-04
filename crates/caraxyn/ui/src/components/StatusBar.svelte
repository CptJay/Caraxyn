<script lang="ts">
  import { shortId } from '../lib/format';
  import { copyText } from '../lib/state.svelte';

  let { peerId, downloads, warnings }: { peerId: string; downloads: number; warnings: number } = $props();

  let copied = $state(false);
</script>

<footer class="flex items-center gap-4 border-t border-line bg-panel px-4 py-1.5 text-xs text-muted">
  <button class="font-mono hover:text-fg" title="Copy your full id" onclick={() => copyText(peerId, (c) => (copied = c))}>
    id {shortId(peerId)}… {copied ? '(copied)' : ''}
  </button>
  <span>{downloads === 0 ? 'no downloads' : `${downloads} downloading`}</span>
  {#if warnings > 0}<span class="text-warn">⚠ {warnings}</span>{/if}
</footer>
