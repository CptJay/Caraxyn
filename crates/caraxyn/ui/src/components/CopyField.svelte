<script lang="ts">
  let { value, label }: { value: string; label: string } = $props();

  let copied = $state(false);

  async function copy() {
    await navigator.clipboard.writeText(value);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="flex gap-2">
  <input class="field font-mono text-xs" readonly aria-label={label} {value} onfocus={(e) => e.currentTarget.select()} />
  <button class="btn shrink-0" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
</div>
