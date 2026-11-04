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

  // Labels stay in place (no layout jump) but are invisible while closed.
  const fade = $derived([
    'transition-opacity duration-150 motion-reduce:transition-none',
    collapsed && 'opacity-0',
  ]);
</script>

<!--
  One layout for both states: icons keep their place and labels never wrap;
  closed, the labels fade out instead of being cut off. Only width, padding
  and opacity animate, so nothing jumps.
-->
<nav
  class={[
    'flex shrink-0 flex-col gap-1 overflow-hidden border-r border-line bg-panel p-2',
    'transition-[width] duration-150 motion-reduce:transition-none',
    collapsed ? 'w-14' : 'w-48',
  ]}
>
  <div class="mb-3 flex items-center">
    <!-- Shrinks to nothing when closed, so the toggle slides over to the icon column. -->
    <p
      class={[
        'min-w-0 flex-1 overflow-hidden text-base font-semibold tracking-tight whitespace-nowrap',
        'transition-[opacity,padding] duration-150 motion-reduce:transition-none',
        collapsed ? 'pl-0 opacity-0' : 'pl-3',
      ]}
      aria-hidden={collapsed}
    >
      Caraxyn
    </p>
    <button
      class="shrink-0 rounded-md px-3 py-2 text-muted hover:bg-bg hover:text-fg"
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
        'relative flex items-center gap-2.5 rounded-md px-3 py-2 text-left text-sm whitespace-nowrap',
        // Pushed to the bottom of the bar, away from the everyday pages.
        item.view === 'settings' && 'mt-auto',
        view === item.view ? 'bg-accent/15 font-medium text-accent' : 'hover:bg-bg',
      ]}
      aria-current={view === item.view ? 'page' : undefined}
      title={collapsed ? item.label : undefined}
      onclick={() => onselect(item.view)}
    >
      <Icon name={item.icon} class="size-4 shrink-0" />
      <span class={['flex-1', fade]}>{item.label}</span>
      {#if item.view === 'inbox' && newOffers > 0}
        <span class={['rounded-full bg-accent px-1.5 text-xs text-accent-fg', fade]}
          >{newOffers}<span class="sr-only"> new</span></span
        >
        <!-- The count is hidden when collapsed; this dot stands in for it. -->
        <span
          class={['absolute top-1 left-7 size-2 rounded-full bg-accent transition-opacity', !collapsed && 'opacity-0']}
          aria-hidden="true"
        ></span>
      {/if}
    </button>
  {/each}
</nav>
