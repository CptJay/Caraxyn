<script lang="ts">
  // A button that asks "sure?" before doing something that cannot be undone.
  let {
    label,
    confirmLabel = 'Sure?',
    onconfirm,
    disabled = false,
  }: { label: string; confirmLabel?: string; onconfirm: () => void; disabled?: boolean } = $props();

  let asking = $state(false);
</script>

{#if asking}
  <span class="inline-flex gap-1">
    <button
      class="btn btn-danger"
      onclick={() => {
        asking = false;
        onconfirm();
      }}>{confirmLabel}</button
    >
    <button class="btn" onclick={() => (asking = false)}>Cancel</button>
  </span>
{:else}
  <button class="btn" {disabled} onclick={() => (asking = true)}>{label}</button>
{/if}
