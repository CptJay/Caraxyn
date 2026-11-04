<script lang="ts">
  import type { Millis, Share } from '../lib/api';
  import { formatBytes, formatExpiry } from '../lib/format';
  import type { RecipientState } from '../lib/state.svelte';
  import ConfirmButton from './ConfirmButton.svelte';
  import ExpiryPicker from './ExpiryPicker.svelte';
  import RecipientBadge from './RecipientBadge.svelte';

  let {
    share,
    recipients,
    onrevoke,
    onexpiry,
  }: {
    share: Share;
    recipients: { peer: string; name: string; state: RecipientState; attempts: number }[];
    onrevoke: () => void;
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

<article class={['card space-y-3', revoked && 'opacity-60']}>
  <header class="flex items-start justify-between gap-4">
    <div class="min-w-0">
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
    {/if}
  </header>

  {#if editing}
    <div class="flex items-center gap-2">
      <span class="text-sm">Available for</span>
      <ExpiryPicker bind:value={duration} />
      <button class="btn btn-primary" onclick={saveExpiry}>Save</button>
    </div>
  {/if}

  <div class="flex flex-wrap gap-1.5">
    {#each recipients as r (r.peer)}
      <RecipientBadge name={r.name} state={r.state} attempts={r.attempts} />
    {:else}
      <span class="text-xs text-muted">no recipients left</span>
    {/each}
  </div>
</article>
