<script lang="ts">
  import CopyField from './CopyField.svelte';

  let { oncreate }: { oncreate: () => Promise<string | null> } = $props();

  let code = $state<string | null>(null);
</script>

<section class="card space-y-2">
  <h2 class="font-medium">Invite someone</h2>
  <p class="text-sm text-muted">Send them a code. It works once, within 24 hours.</p>
  {#if code}
    <CopyField value={code} label="Invite code" />
  {/if}
  <button class="btn" onclick={async () => (code = await oncreate())}>
    {code ? 'New code' : 'Create invite code'}
  </button>
</section>
