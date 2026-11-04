<script lang="ts">
  import { shortId } from '../lib/format';

  let { peerId, downloads, warnings }: { peerId: string; downloads: number; warnings: number } = $props();

  let copied = $state(false);

  async function copy() {
    await navigator.clipboard.writeText(peerId);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<footer class="flex items-center gap-4 border-t border-line bg-panel px-4 py-1.5 text-xs text-muted">
  <button class="font-mono hover:text-fg" title="Copy your full id" onclick={copy}>
    id {shortId(peerId)}… {copied ? '(copied)' : ''}
  </button>
  <span>{downloads === 0 ? 'no downloads' : `${downloads} downloading`}</span>
  {#if warnings > 0}<span class="text-warn">⚠ {warnings}</span>{/if}
</footer>
