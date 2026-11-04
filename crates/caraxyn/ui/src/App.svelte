<script lang="ts">
  import ErrorBanner from './components/ErrorBanner.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import WarningList from './components/WarningList.svelte';
  import { app, init } from './lib/state.svelte';
  import Contacts from './views/Contacts.svelte';
  import Inbox from './views/Inbox.svelte';
  import Settings from './views/Settings.svelte';
  import Shares from './views/Shares.svelte';
  import Startup from './views/Startup.svelte';

  init();

  let sidebarCollapsed = $state(false);
  const newOffers = $derived(app.inbox.filter((o) => o.state === 'new').length);
</script>

{#if app.loading}
  <p class="p-8 text-muted">Starting…</p>
{:else if !app.status?.peerId}
  <Startup error={app.status?.startupError ?? app.error ?? 'Unknown error'} dataDir={app.status?.dataDir ?? ''} />
{:else}
  <div class="flex h-full flex-col">
    <div class="flex min-h-0 flex-1">
      <Sidebar
        view={app.view}
        {newOffers}
        collapsed={sidebarCollapsed}
        onselect={(view) => (app.view = view)}
        ontoggle={() => (sidebarCollapsed = !sidebarCollapsed)}
      />
      <main class="flex-1 space-y-4 overflow-y-auto p-6">
        {#if app.error}
          <ErrorBanner message={app.error} onclose={() => (app.error = null)} />
        {/if}
        {#if app.warnings.length > 0}
          <WarningList warnings={app.warnings} onclear={() => (app.warnings = [])} />
        {/if}
        <div>
          {#if app.view === 'inbox'}
            <Inbox />
          {:else if app.view === 'shares'}
            <Shares />
          {:else if app.view === 'contacts'}
            <Contacts />
          {:else}
            <Settings settings={app.status.settings} dataDir={app.status.dataDir} />
          {/if}
        </div>
      </main>
    </div>
    <StatusBar
      peerId={app.status.peerId}
      netStatus={app.status.netStatus ?? 'connecting'}
      downloads={Object.keys(app.downloads).length}
      warnings={app.warnings.length}
    />
  </div>
{/if}
