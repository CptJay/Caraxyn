<script lang="ts">
  import type { Contact, PeerId } from '../lib/api';

  let { contacts, selected = $bindable() }: { contacts: Contact[]; selected: PeerId[] } = $props();
</script>

<fieldset class="max-h-48 space-y-1 overflow-y-auto rounded-md border border-line p-2">
  <legend class="px-1 text-xs text-muted">Recipients</legend>
  {#each contacts as contact (contact.peer)}
    <label class="flex items-center gap-2 rounded px-1 py-0.5 hover:bg-bg">
      <input type="checkbox" value={contact.peer} bind:group={selected} />
      {contact.name}
    </label>
  {/each}
</fieldset>
