<script lang="ts">
  import type { NetStatus } from '../lib/api';

  let { status }: { status: NetStatus } = $props();

  const LABEL: Record<NetStatus, string> = {
    online: 'Online: contacts anywhere can reach you',
    connecting: 'Connecting…',
    offline: 'Offline: no internet connection to the relay',
  };
  // currentColor drives the gradient and the glow in app.css.
  const COLOR: Record<NetStatus, string> = {
    online: 'text-ok',
    connecting: 'text-warn',
    offline: 'text-muted',
  };
</script>

<span class={['inline-flex', COLOR[status]]} role="status" title={LABEL[status]}>
  <span class={['status-dot', status !== 'offline' && 'breathe']} aria-hidden="true"></span>
  <span class="sr-only">{LABEL[status]}</span>
</span>
