<script lang="ts">
  import type { RecipientState } from '../lib/state.svelte';

  let { name, state, attempts }: { name: string; state: RecipientState; attempts: number } = $props();

  const LABEL: Record<RecipientState, string> = {
    pending: 'waiting to deliver',
    delivered: 'delivered',
    undelivered: 'not delivered',
    revoking: 'telling them it is withdrawn',
    revoked: 'withdrawn',
  };
  const COLOR: Record<RecipientState, string> = {
    pending: 'text-warn',
    delivered: 'text-ok',
    undelivered: 'text-danger',
    revoking: 'text-warn',
    revoked: 'text-muted',
  };
</script>

<span class="inline-flex items-center gap-1.5 h-6 rounded-lg border border-outline px-2 text-xs">
  <span class="font-medium">{name}</span>
  <span class={COLOR[state]}>{LABEL[state]}{attempts > 0 ? ` (${attempts} tries)` : ''}</span>
</span>
