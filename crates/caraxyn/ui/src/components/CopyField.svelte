<script lang="ts">
  import { copyText } from '../lib/state.svelte';

  let { value, label }: { value: string; label: string } = $props();

  let copied = $state(false);
</script>

<div class="flex gap-2">
  <input class="field font-mono text-xs" readonly aria-label={label} {value} onfocus={(e) => e.currentTarget.select()} />
  <button class="btn shrink-0" onclick={() => copyText(value, (c) => (copied = c))}>{copied ? 'Copied' : 'Copy'}</button>
</div>
