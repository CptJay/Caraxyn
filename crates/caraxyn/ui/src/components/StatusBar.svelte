<script lang="ts">
  import type { NetStatus } from '../lib/api';
  import { shortId } from '../lib/format';
  import { copyText } from '../lib/state.svelte';
  import NetStatusDot from './NetStatusDot.svelte';

  let {
    peerId,
    netStatus,
    downloads,
    uploads,
    warnings,
  }: { peerId: string; netStatus: NetStatus; downloads: number; uploads: number; warnings: number } = $props();

  let copied = $state(false);
</script>

<footer class="flex h-8 items-center gap-4 bg-rail px-4 text-xs text-muted">
  <NetStatusDot status={netStatus} />
  <button class="tabular-nums hover:text-fg" title="Copy your full id" onclick={() => copyText(peerId, (c) => (copied = c))}>
    id {shortId(peerId)}… {copied ? '(copied)' : ''}
  </button>
  {#if downloads > 0}<span>{downloads} downloading</span>{/if}
  {#if uploads > 0}<span>{uploads} uploading</span>{/if}
  {#if warnings > 0}<span class="text-warn">⚠ {warnings}</span>{/if}
</footer>
