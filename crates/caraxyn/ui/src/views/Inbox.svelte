<script lang="ts">
  import EmptyState from '../components/EmptyState.svelte';
  import OfferCard from '../components/OfferCard.svelte';
  import type { Offer } from '../lib/api';
  import { accept, app, contactName, decline, pairKey, pickPath } from '../lib/state.svelte';

  const byNewest = (a: Offer, b: Offer) => b.receivedAt - a.receivedAt;
  const fresh = $derived(app.inbox.filter((o) => o.state === 'new').sort(byNewest));
  const earlier = $derived(app.inbox.filter((o) => o.state !== 'new').sort(byNewest));

  async function acceptElsewhere(offer: Offer) {
    const folder = await pickPath(true);
    if (folder) await accept(offer, folder);
  }
</script>

{#snippet list(offers: Offer[])}
  <div class="space-y-3">
    {#each offers as offer (pairKey(offer.from, offer.root))}
      <OfferCard
        {offer}
        sender={contactName(offer.from)}
        download={app.downloads[pairKey(offer.from, offer.root)]}
        onaccept={() => accept(offer)}
        onacceptelsewhere={() => acceptElsewhere(offer)}
        ondecline={() => decline(offer)}
      />
    {/each}
  </div>
{/snippet}

<h1 class="mb-4 text-lg font-semibold">Inbox</h1>
{#if app.inbox.length === 0}
  <EmptyState title="Nothing here yet" hint="When a contact shares something with you, it shows up here." />
{:else}
  {@render list(fresh)}
  {#if earlier.length > 0}
    <h2 class="mt-8 mb-3 text-sm font-medium text-muted">Earlier</h2>
    {@render list(earlier)}
  {/if}
{/if}
