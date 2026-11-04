<script lang="ts">
  import { pickPath } from '../lib/state.svelte';
  import Icon from './Icon.svelte';

  // What to share: one "Choose…" button with a small File / Folder menu, since
  // the native picker on Linux and Windows opens either files or folders, not both.
  let { path = $bindable() }: { path: string | null } = $props();

  let isFolder = $state(false);
  let open = $state(false);
  let root: HTMLElement;

  const name = $derived(path?.split(/[\\/]/).filter(Boolean).pop() ?? null);

  async function choose(folder: boolean) {
    open = false;
    const picked = await pickPath(folder);
    if (picked) {
      path = picked;
      isFolder = folder;
    }
  }

  // Closes the menu on a click elsewhere or on Escape.
  function closeOutside(e: MouseEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
  function closeOnEscape(e: KeyboardEvent) {
    if (open && e.key === 'Escape') open = false;
  }
</script>

<svelte:window onclick={closeOutside} onkeydown={closeOnEscape} />

<div class="flex items-center gap-3">
  <div class="relative" bind:this={root}>
    <button type="button" class="btn" aria-haspopup="true" aria-expanded={open} onclick={() => (open = !open)}>
      Choose…
      <Icon name="chevronDown" class="size-3.5" />
    </button>
    {#if open}
      <div class="absolute top-full left-0 z-10 mt-1 flex w-40 flex-col rounded bg-menu py-2 shadow-m3-2">
        <button type="button" class="flex h-10 items-center gap-3 px-3 text-left text-sm hover:bg-fg/8" onclick={() => choose(false)}>
          <Icon name="file" /> File…
        </button>
        <button type="button" class="flex h-10 items-center gap-3 px-3 text-left text-sm hover:bg-fg/8" onclick={() => choose(true)}>
          <Icon name="folder" /> Folder…
        </button>
      </div>
    {/if}
  </div>

  {#if path && name}
    <span class="flex min-w-0 items-center gap-1.5 text-sm" title={path}>
      <Icon name={isFolder ? 'folder' : 'file'} class="size-4 shrink-0 text-muted" />
      <span class="sr-only">{isFolder ? 'Folder:' : 'File:'}</span>
      <span class="truncate">{name}</span>
    </span>
  {:else}
    <span class="text-sm text-muted">No file or folder chosen</span>
  {/if}
</div>
