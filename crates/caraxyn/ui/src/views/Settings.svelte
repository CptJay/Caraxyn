<script lang="ts">
  import PathPicker from '../components/PathPicker.svelte';
  import type { Settings } from '../lib/api';
  import { openLogFolder, saveSettings } from '../lib/state.svelte';

  let { settings, dataDir }: { settings: Settings; dataDir: string } = $props();

  // Edited locally; saved with the button. The form starts from the saved settings.
  // svelte-ignore state_referenced_locally
  let displayName = $state(settings.displayName);
  // svelte-ignore state_referenced_locally
  let downloadDir = $state(settings.downloadDir);
  let saved = $state(false);

  const nameValid = $derived(displayName.trim().length >= 1 && displayName.trim().length <= 64);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    saved = await saveSettings({ displayName: displayName.trim(), downloadDir });
  }
</script>

<h1 class="mb-4 text-lg font-semibold">Settings</h1>

<form class="card max-w-xl space-y-4" onsubmit={submit} oninput={() => (saved = false)}>
  <label class="block space-y-1">
    <span class="text-sm font-medium">Display name</span>
    <input class="field" maxlength="64" bind:value={displayName} placeholder="How contacts see you" />
    <span class="block text-xs text-muted">Sent with your invites. 1–64 characters.</span>
  </label>

  <div class="space-y-1">
    <span class="text-sm font-medium">Download folder</span>
    <PathPicker bind:path={downloadDir} directory label="Choose folder" />
    <span class="block text-xs text-muted">Accepted shares are saved here. Without one, you are asked each time.</span>
  </div>

  <div class="flex items-center gap-3">
    <button class="btn btn-primary" type="submit" disabled={!nameValid}>Save</button>
    {#if saved}<span class="text-sm text-ok">Saved</span>{/if}
  </div>
</form>

<section class="card mt-4 max-w-xl space-y-2">
  <h2 class="font-medium">Data</h2>
  <code class="block font-mono text-xs break-all text-muted">{dataDir}</code>
  <button class="btn" onclick={openLogFolder}>Open log folder</button>
</section>
