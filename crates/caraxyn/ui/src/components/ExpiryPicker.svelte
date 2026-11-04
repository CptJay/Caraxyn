<script lang="ts">
  // How long from now a share stays available; `null` means forever.
  let { value = $bindable() }: { value: number | null } = $props();

  const HOUR = 60 * 60 * 1000;
  const OPTIONS: { label: string; ms: number | null }[] = [
    { label: 'No expiry', ms: null },
    { label: '1 hour', ms: HOUR },
    { label: '24 hours', ms: 24 * HOUR },
    { label: '7 days', ms: 7 * 24 * HOUR },
  ];
</script>

<select class="field w-auto" aria-label="Expiry" bind:value>
  {#each OPTIONS as option (option.label)}
    <option value={option.ms}>{option.label}</option>
  {/each}
</select>
