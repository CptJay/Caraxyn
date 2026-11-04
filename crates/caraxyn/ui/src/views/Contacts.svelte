<script lang="ts">
  import ContactRow from '../components/ContactRow.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import InvitePanel from '../components/InvitePanel.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import RedeemForm from '../components/RedeemForm.svelte';
  import { app, createInvite, redeemInvite, removeContact } from '../lib/state.svelte';

  const sorted = $derived([...app.contacts].sort((a, b) => a.name.localeCompare(b.name)));
</script>

<PageHeader title="Contacts" />

<div class="mb-6 grid gap-3 md:grid-cols-2">
  <InvitePanel oncreate={createInvite} />
  <RedeemForm onredeem={redeemInvite} />
</div>

{#if sorted.length === 0}
  <EmptyState title="No contacts yet" hint="Create an invite code and send it, or paste one you received." />
{:else}
  <ul class="divide-y divide-line rounded-lg border border-line bg-panel">
    {#each sorted as contact (contact.peer)}
      <ContactRow {contact} onremove={() => removeContact(contact.peer)} />
    {/each}
  </ul>
{/if}
