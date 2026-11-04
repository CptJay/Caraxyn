<script lang="ts">
  import EmptyState from '../components/EmptyState.svelte';
  import ShareRow from '../components/ShareRow.svelte';
  import type { Share } from '../lib/api';
  import { app, contactName, recipientState, revoke, setExpiry } from '../lib/state.svelte';
  import NewShare from './NewShare.svelte';

  let creating = $state(false);

  // Active shares first, withdrawn ones after.
  const sorted = $derived([...app.shares].sort((a, b) => Number(a.revokedAt !== null) - Number(b.revokedAt !== null)));

  const recipientsOf = (share: Share) =>
    share.recipients.map((peer) => ({ peer, name: contactName(peer), ...recipientState(share, peer) }));
</script>

<div class="mb-4 flex items-center justify-between">
  <h1 class="text-lg font-semibold">My shares</h1>
  {#if !creating}
    <button class="btn btn-primary" onclick={() => (creating = true)}>New share</button>
  {/if}
</div>

<div class="space-y-3">
  {#if creating}
    <NewShare ondone={() => (creating = false)} />
  {/if}

  {#each sorted as share (share.root)}
    <ShareRow
      {share}
      recipients={recipientsOf(share)}
      onrevoke={() => revoke(share.root)}
      onexpiry={(expiresAt) => setExpiry(share.root, expiresAt)}
    />
  {:else}
    {#if !creating}
      <EmptyState title="You have not shared anything" hint="Share a file or folder with one or more contacts.">
        <button class="btn btn-primary" onclick={() => (creating = true)}>New share</button>
      </EmptyState>
    {/if}
  {/each}
</div>
