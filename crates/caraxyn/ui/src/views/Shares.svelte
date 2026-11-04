<script lang="ts">
  import EmptyState from '../components/EmptyState.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import ShareRow from '../components/ShareRow.svelte';
  import type { Share } from '../lib/api';
  import { app, contactName, forgetShare, recipientState, revoke, setExpiry } from '../lib/state.svelte';
  import NewShare from './NewShare.svelte';

  let creating = $state(false);

  // Active shares first, withdrawn ones after.
  const sorted = $derived([...app.shares].sort((a, b) => Number(a.revokedAt !== null) - Number(b.revokedAt !== null)));

  const recipientsOf = (share: Share) =>
    share.recipients.map((peer) => ({ peer, name: contactName(peer), ...recipientState(share, peer) }));
</script>

<PageHeader title="My shares">
  {#if !creating}
    <button class="btn btn-primary" onclick={() => (creating = true)}>New share</button>
  {/if}
</PageHeader>

<div class="space-y-3">
  {#if creating}
    <NewShare ondone={() => (creating = false)} />
  {/if}

  {#each sorted as share (share.root)}
    <ShareRow
      {share}
      recipients={recipientsOf(share)}
      onrevoke={() => revoke(share.root)}
      onremove={() => forgetShare(share.root)}
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
