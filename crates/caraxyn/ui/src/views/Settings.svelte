<script lang="ts">
  import PageHeader from '../components/PageHeader.svelte';
  import PathPicker from '../components/PathPicker.svelte';
  import SettingsSection from '../components/SettingsSection.svelte';
  import type { Settings } from '../lib/api';
  import { openLogFolder, saveSettings } from '../lib/state.svelte';

  let { settings, dataDir, version }: { settings: Settings; dataDir: string; version: string } = $props();

  // Edited locally and saved with the button; starts from the saved name.
  // svelte-ignore state_referenced_locally
  let displayName = $state(settings.displayName);
  // svelte-ignore state_referenced_locally
  let downloadDir = $state(settings.downloadDir);
  let nameSaved = $state(false);
  let folderSaved = $state(false);

  const nameValid = $derived(displayName.trim().length >= 1 && displayName.trim().length <= 64);

  async function saveName(e: SubmitEvent) {
    e.preventDefault();
    nameSaved = await saveSettings({ ...settings, displayName: displayName.trim() });
  }

  // A picked folder is saved at once: there is nothing else to fill in.
  async function saveFolder(folder: string) {
    folderSaved = await saveSettings({ ...settings, downloadDir: folder });
  }
</script>

<PageHeader title="Settings" />

<div class="space-y-4">
  <SettingsSection title="Profile">
    <form class="space-y-3" onsubmit={saveName} oninput={() => (nameSaved = false)}>
      <label class="block space-y-1">
        <span class="text-sm font-medium">Display name</span>
        <input class="field" maxlength="64" bind:value={displayName} placeholder="How contacts see you" />
      </label>
      <div class="flex items-center gap-3">
        <button class="btn btn-primary" type="submit" disabled={!nameValid}>Save</button>
        {#if nameSaved}<span class="text-sm text-ok">Saved</span>{/if}
      </div>
    </form>
  </SettingsSection>

  <SettingsSection title="Storage">
    <div class="space-y-1">
      <span class="text-sm font-medium">Download folder</span>
      <PathPicker bind:path={downloadDir} directory label="Choose folder" onpicked={saveFolder} />
      {#if folderSaved}<span class="block text-xs text-ok">Saved</span>{/if}
    </div>
    <div class="space-y-1">
      <span class="text-sm font-medium">Data folder</span>
      <code class="block font-mono text-xs break-all text-muted">{dataDir}</code>
      <span class="block text-xs text-muted">Your identity, contacts and shares. Keep it private.</span>
    </div>
    <button class="btn" onclick={openLogFolder}>Open log folder</button>
  </SettingsSection>

  <SettingsSection title="About">
    <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1 text-sm">
      <dt class="text-muted">App</dt>
      <dd>Caraxyn {version}</dd>
      <dt class="text-muted">What it is</dt>
      <dd>Peer-to-peer file sharing between people who know each other.</dd>
      <dt class="text-muted">Made by</dt>
      <dd>CptJay</dd>
    </dl>
  </SettingsSection>
</div>
