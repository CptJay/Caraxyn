<script lang="ts">
  import { pickPath } from '../lib/state.svelte';

  let {
    path = $bindable(),
    directory,
    label,
    onpicked,
  }: { path: string | null; directory: boolean; label: string; onpicked?: (path: string) => void } = $props();

  async function pick() {
    const picked = await pickPath(directory);
    if (picked) {
      path = picked;
      onpicked?.(picked);
    }
  }
</script>

<div class="flex items-center gap-2">
  <button type="button" class="btn shrink-0" onclick={pick}>{label}</button>
  <span class="truncate font-mono text-xs text-muted" title={path ?? ''}>{path ?? 'nothing chosen'}</span>
</div>
