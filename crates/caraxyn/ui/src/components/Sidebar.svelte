<script lang="ts">
  import type { View } from '../lib/state.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  let {
    view,
    newOffers,
    collapsed,
    onselect,
    ontoggle,
  }: {
    view: View;
    newOffers: number;
    collapsed: boolean;
    onselect: (view: View) => void;
    ontoggle: () => void;
  } = $props();

  const items: { view: View; label: string; icon: IconName }[] = [
    { view: 'inbox', label: 'Inbox', icon: 'inbox' },
    { view: 'shares', label: 'My shares', icon: 'share' },
    { view: 'contacts', label: 'Contacts', icon: 'contacts' },
    { view: 'settings', label: 'Settings', icon: 'settings' },
  ];

  // Collapsed, only the icon shows, so the name (and unread count) moves into the label.
  const nameOf = (item: (typeof items)[number]) =>
    item.view === 'inbox' && newOffers > 0 ? `${item.label}, ${newOffers} new` : item.label;
</script>

<nav
  class={[
    'flex shrink-0 flex-col gap-1 border-r border-line bg-panel p-2 transition-[width] duration-150',
    collapsed ? 'w-14' : 'w-48',
  ]}
>
  <div class={['mb-3 flex items-center', collapsed ? 'justify-center' : 'justify-between pl-2']}>
    {#if !collapsed}<p class="text-base font-semibold tracking-tight">Caraxyn</p>{/if}
    <button
      class="rounded-md p-1.5 text-muted hover:bg-bg hover:text-fg"
      aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      aria-expanded={!collapsed}
      title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      onclick={ontoggle}
    >
      <Icon name={collapsed ? 'expand' : 'collapse'} />
    </button>
  </div>

  {#each items as item (item.view)}
    <button
      class={[
        'relative flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm',
        collapsed && 'justify-center',
        view === item.view ? 'bg-accent/15 font-medium text-accent' : 'hover:bg-bg',
      ]}
      aria-current={view === item.view ? 'page' : undefined}
      aria-label={collapsed ? nameOf(item) : undefined}
      title={collapsed ? nameOf(item) : undefined}
      onclick={() => onselect(item.view)}
    >
      <Icon name={item.icon} class="size-4 shrink-0" />
      {#if !collapsed}<span class="flex-1">{item.label}</span>{/if}
      {#if item.view === 'inbox' && newOffers > 0}
        {#if collapsed}
          <span class="absolute top-1 right-1 size-2 rounded-full bg-accent" aria-hidden="true"></span>
        {:else}
          <span class="rounded-full bg-accent px-1.5 text-xs text-accent-fg">{newOffers}</span>
        {/if}
      {/if}
    </button>
  {/each}
</nav>
