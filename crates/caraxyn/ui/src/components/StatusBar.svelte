<script lang="ts">
  import type { NetStatus } from '../lib/api';
  import { shortId } from '../lib/format';
  import { copyText } from '../lib/state.svelte';
  import NetStatusDot from './NetStatusDot.svelte';

  let {
    peerId,
    netStatus,
    downloads,
    warnings,
  }: { peerId: string; netStatus: NetStatus; downloads: number; warnings: number } = $props();

  let copied = $state(false);
</script>

<footer class="flex items-center gap-4 border-t border-line bg-panel px-4 py-1.5 text-xs text-muted">
  <NetStatusDot status={netStatus} />
  <button class="tabular-nums hover:text-fg" title="Copy your full id" onclick={() => copyText(peerId, (c) => (copied = c))}>
    id {shortId(peerId)}… {copied ? '(copied)' : ''}
  </button>
  {#if downloads > 0}<span>{downloads} downloading</span>{/if}
  {#if warnings > 0}<span class="text-warn">⚠ {warnings}</span>{/if}
</footer>
