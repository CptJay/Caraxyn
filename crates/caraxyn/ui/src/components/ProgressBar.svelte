<script lang="ts">
  import { formatBytes } from '../lib/format';

  let { bytes, total }: { bytes: number; total: number | null } = $props();

  const percent = $derived(total ? Math.min(100, Math.round((bytes / total) * 100)) : null);
</script>

<div class="space-y-1">
  <div
    class="h-1.5 overflow-hidden rounded-full bg-line"
    role="progressbar"
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={percent ?? undefined}
  >
    <div class="h-full bg-accent transition-[width]" style:width="{percent ?? 0}%"></div>
  </div>
  <p class="text-xs text-muted">
    {formatBytes(bytes)}{total ? ` of ${formatBytes(total)}` : ''}{percent !== null ? ` · ${percent}%` : ''}
  </p>
</div>
