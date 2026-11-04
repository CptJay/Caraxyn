<script lang="ts">
  import { formatSpeed, formatTimeLeft } from '../lib/format';
  import type { RecipientState } from '../lib/state.svelte';
  import { secondsLeft, type Transfer } from '../lib/transfer';

  let {
    name,
    state,
    attempts,
    upload,
  }: { name: string; state: RecipientState; attempts: number; upload?: Transfer } = $props();

  // A resumed download sends only what is missing, so it can end below 100%.
  const percent = $derived(upload?.total ? Math.min(100, Math.round((upload.bytes / upload.total) * 100)) : 0);
  const left = $derived(upload ? secondsLeft(upload) : null);

  const LABEL: Record<RecipientState, string> = {
    pending: 'waiting to deliver',
    delivered: 'delivered',
    undelivered: 'not delivered',
    revoking: 'telling them it is withdrawn',
    revoked: 'withdrawn',
  };
  const COLOR: Record<RecipientState, string> = {
    pending: 'text-warn',
    delivered: 'text-ok',
    undelivered: 'text-danger',
    revoking: 'text-warn',
    revoked: 'text-muted',
  };
</script>

<span
  class="relative inline-flex h-6 items-center gap-1.5 overflow-hidden rounded-lg border border-outline px-2 text-xs"
  title={upload && left !== null ? formatTimeLeft(left) : undefined}
>
  <span class="font-medium">{name}</span>
  {#if upload}
    <span class="text-accent tabular-nums">
      downloading {percent}%{upload.speed !== null ? ` · ${formatSpeed(upload.speed)}` : ''}
    </span>
    <span class="absolute bottom-0 left-0 h-0.5 bg-accent transition-[width]" style:width="{percent}%" aria-hidden="true"></span>
  {:else}
    <span class={COLOR[state]}>{LABEL[state]}{attempts > 0 ? ` (${attempts} tries)` : ''}</span>
  {/if}
</span>
