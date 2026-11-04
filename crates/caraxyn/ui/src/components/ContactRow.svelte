<script lang="ts">
  import type { Contact } from '../lib/api';
  import { formatDate, shortId } from '../lib/format';
  import ConfirmButton from './ConfirmButton.svelte';

  let { contact, onremove }: { contact: Contact; onremove: () => void } = $props();
</script>

<li class="flex items-center justify-between gap-4 px-4 py-3">
  <div class="min-w-0">
    <p class="truncate font-medium">{contact.name}</p>
    <p class="text-xs text-muted">
      <span class="font-mono" title={contact.peer}>{shortId(contact.peer)}</span> · added {formatDate(contact.addedAt)}
    </p>
  </div>
  <ConfirmButton label="Remove" confirmLabel="Remove" onconfirm={onremove} />
</li>
