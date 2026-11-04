<script lang="ts">
  import { pickPath } from '../lib/state.svelte';

  let {
    path = $bindable(),
    directory,
    label,
  }: { path: string | null; directory: boolean; label: string } = $props();

  async function pick() {
    path = (await pickPath(directory)) ?? path;
  }
</script>

<div class="flex items-center gap-2">
  <button class="btn shrink-0" onclick={pick}>{label}</button>
  <span class="truncate font-mono text-xs text-muted" title={path ?? ''}>{path ?? 'nothing chosen'}</span>
</div>
