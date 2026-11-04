<script lang="ts">
  import type { Contact, PeerId } from '../lib/api';

  let { contacts, selected = $bindable() }: { contacts: Contact[]; selected: PeerId[] } = $props();
</script>

<!-- The section around it shows the heading; the legend is for screen readers. -->
<fieldset class="-mx-3 max-h-48 space-y-0.5 overflow-y-auto">
  <legend class="sr-only">Recipients</legend>
  {#each contacts as contact (contact.peer)}
    <label class="flex h-9 items-center gap-3 rounded-lg px-3 hover:bg-fg/8">
      <input type="checkbox" value={contact.peer} bind:group={selected} />
      {contact.name}
    </label>
  {/each}
</fieldset>
