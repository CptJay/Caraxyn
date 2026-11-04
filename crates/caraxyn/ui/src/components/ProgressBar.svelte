<script lang="ts">
  import { formatBytes, formatSpeed, formatTimeLeft, RELAY_HINT, routeLabel } from '../lib/format';
  import { secondsLeft, type Transfer } from '../lib/transfer';

  let { transfer }: { transfer: Transfer } = $props();

  const { bytes, total } = $derived(transfer);
  const percent = $derived(total ? Math.min(100, Math.round((bytes / total) * 100)) : null);
  const left = $derived(secondsLeft(transfer));
</script>

<div class="space-y-1">
  <div
    class="h-1 overflow-hidden rounded-full bg-tonal"
    role="progressbar"
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={percent ?? undefined}
  >
    <div class="h-full bg-accent transition-[width]" style:width="{percent ?? 0}%"></div>
  </div>
  <p class="text-xs text-muted tabular-nums">
    {formatBytes(bytes)}{total ? ` of ${formatBytes(total)}` : ''}{percent !== null ? ` · ${percent}%` : ''}
    {#if transfer.speed !== null}
      · {formatSpeed(transfer.speed)}{left !== null ? ` · ${formatTimeLeft(left)}` : ''}
    {/if}
    {#if transfer.direct !== null}
      ·
      <span class={transfer.direct ? '' : 'text-warn'} title={transfer.direct ? undefined : RELAY_HINT}>
        {routeLabel(transfer.direct)}
      </span>
    {/if}
  </p>
</div>
