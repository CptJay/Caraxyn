<script lang="ts">
  let { onredeem }: { onredeem: (code: string) => Promise<boolean> } = $props();

  let code = $state('');
  let busy = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    if (await onredeem(code)) code = '';
    busy = false;
  }
</script>

<form class="card space-y-2" onsubmit={submit}>
  <h2 class="font-medium">Add with a code</h2>
  <p class="text-sm text-muted">Paste the code someone sent you. They must be online.</p>
  <textarea class="field font-mono text-xs" rows="3" placeholder="caraxyn1:…" aria-label="Invite code" bind:value={code}
  ></textarea>
  <button class="btn btn-primary" type="submit" disabled={busy || !code.trim()}>
    {busy ? 'Connecting…' : 'Add contact'}
  </button>
</form>
