<script lang="ts">
  import type { Millis, Share } from '../lib/api';
  import { formatBytes, formatExpiry } from '../lib/format';
  import type { RecipientState } from '../lib/state.svelte';
  import type { Transfer } from '../lib/transfer';
  import ConfirmButton from './ConfirmButton.svelte';
  import ExpiryPicker from './ExpiryPicker.svelte';
  import RecipientBadge from './RecipientBadge.svelte';

  let {
    share,
    recipients,
    onrevoke,
    onremove,
    onexpiry,
  }: {
    share: Share;
    recipients: { peer: string; name: string; state: RecipientState; attempts: number; upload?: Transfer }[];
    onrevoke: () => void;
    onremove: () => void;
    onexpiry: (expiresAt: Millis | null) => void;
  } = $props();

  let editing = $state(false);
  let duration = $state<number | null>(null);
  const revoked = $derived(share.revokedAt !== null);

  function saveExpiry() {
    editing = false;
    onexpiry(duration === null ? null : Date.now() + duration);
  }
</script>

<article class="card space-y-3">
  <header class="flex items-start justify-between gap-4">
    <!-- Withdrawn: the details fade, the Remove button stays easy to read. -->
    <div class={['min-w-0', revoked && 'opacity-60']}>
      <h3 class="truncate font-medium" title={share.title}>{share.title}</h3>
      <p class="text-xs text-muted">
        {share.fileCount}
        {share.fileCount === 1 ? 'file' : 'files'} · {formatBytes(share.totalSize)} ·
        {revoked ? 'withdrawn' : formatExpiry(share.expiresAt)}
      </p>
    </div>
    {#if !revoked}
      <div class="flex shrink-0 gap-2">
        <button class="btn" onclick={() => (editing = !editing)}>Change expiry</button>
        <ConfirmButton label="Withdraw" confirmLabel="Withdraw" onconfirm={onrevoke} />
      </div>
    {:else}
      <ConfirmButton label="Remove" confirmLabel="Remove from list" onconfirm={onremove} />
    {/if}
  </header>

  {#if editing}
    <div class="flex items-center gap-2">
      <span class="text-sm">Available for</span>
      <ExpiryPicker bind:value={duration} />
      <button class="btn btn-primary" onclick={saveExpiry}>Save</button>
    </div>
  {/if}

  <div class={['flex flex-wrap gap-1.5', revoked && 'opacity-60']}>
    {#each recipients as r (r.peer)}
      <RecipientBadge name={r.name} state={r.state} attempts={r.attempts} upload={r.upload} />
    {:else}
      <span class="text-xs text-muted">no recipients left</span>
    {/each}
  </div>
</article>
