<script lang="ts">
  import type { Offer } from '../lib/api';
  import { formatBytes, formatDate, formatExpiry } from '../lib/format';
  import type { Download } from '../lib/state.svelte';
  import ConfirmButton from './ConfirmButton.svelte';
  import FileList from './FileList.svelte';
  import ProgressBar from './ProgressBar.svelte';

  let {
    offer,
    sender,
    download,
    onaccept,
    onacceptelsewhere,
    ondecline,
    onremove,
  }: {
    offer: Offer;
    sender: string;
    download?: Download;
    onaccept: () => void;
    onacceptelsewhere: () => void;
    ondecline: () => void;
    onremove: () => void;
  } = $props();

  const STATE_LABEL = { new: 'New', declined: 'Declined', done: 'Saved', revoked: 'Withdrawn' };
  // shortcut: checked when the card renders, not as time passes; add a clock if cards stay open for hours.
  const expired = $derived(offer.expiresAt !== null && offer.expiresAt <= Date.now());
</script>

<article class={['card space-y-3', offer.state !== 'new' && 'opacity-75']}>
  <header class="flex items-start justify-between gap-4">
    <div class="min-w-0">
      <h3 class="truncate font-medium" title={offer.title}>{offer.title}</h3>
      <p class="text-xs text-muted">
        from <span class="text-fg">{sender}</span> · {formatDate(offer.receivedAt)}
      </p>
    </div>
    <span class="inline-flex h-6 shrink-0 items-center rounded-lg border border-outline px-2 text-xs">{STATE_LABEL[offer.state]}</span>
  </header>

  <p class="text-xs text-muted">
    {offer.fileCount}
    {offer.fileCount === 1 ? 'file' : 'files'} · {formatBytes(offer.totalSize)} · {formatExpiry(offer.expiresAt)}
  </p>
  <FileList preview={offer.preview} fileCount={offer.fileCount} />

  {#if download}
    <ProgressBar transfer={download} />
  {:else if offer.state === 'new'}
    <footer class="flex flex-wrap gap-2">
      <button class="btn btn-primary" onclick={onaccept}>Accept</button>
      <button class="btn" onclick={onacceptelsewhere}>Save to…</button>
      <ConfirmButton label="Decline" confirmLabel="Decline" onconfirm={ondecline} />
      {#if expired}
        <p class="basis-full text-xs text-warn">Expired: the sender may no longer have it. Accept still tries.</p>
      {/if}
    </footer>
  {:else}
    <footer>
      <ConfirmButton label="Remove" confirmLabel="Remove from inbox" onconfirm={onremove} />
    </footer>
  {/if}
</article>
