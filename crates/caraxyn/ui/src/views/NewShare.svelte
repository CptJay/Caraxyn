<script lang="ts">
  import ContactPicker from '../components/ContactPicker.svelte';
  import ExpiryPicker from '../components/ExpiryPicker.svelte';
  import FormSection from '../components/FormSection.svelte';
  import SourcePicker from '../components/SourcePicker.svelte';
  import type { PeerId } from '../lib/api';
  import { app, share } from '../lib/state.svelte';

  let { ondone }: { ondone: () => void } = $props();

  let path = $state<string | null>(null);
  let recipients = $state<PeerId[]>([]);
  let duration = $state<number | null>(null);
  let busy = $state(false);

  const ready = $derived(path !== null && recipients.length > 0 && !busy);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!path) return;
    busy = true;
    const expiresAt = duration === null ? null : Date.now() + duration;
    if (await share(path, recipients, expiresAt)) ondone();
    busy = false;
  }
</script>

<form class="space-y-4 pb-4" onsubmit={submit}>
  <h2 class="text-base font-medium">New share</h2>

  <FormSection title="What to share">
    <SourcePicker bind:path />
  </FormSection>

  <FormSection title="Share with">
    {#if app.contacts.length === 0}
      <p class="text-sm text-muted">Add a contact first; shares go to contacts only.</p>
    {:else}
      <ContactPicker contacts={app.contacts} bind:selected={recipients} />
    {/if}
  </FormSection>

  <FormSection title="Available for">
    <ExpiryPicker bind:value={duration} />
  </FormSection>

  <div class="flex gap-2">
    <button class="btn btn-primary" type="submit" disabled={!ready}>{busy ? 'Sharing…' : 'Share'}</button>
    <button class="btn btn-outlined" type="button" onclick={ondone}>Cancel</button>
  </div>
</form>
