<script lang="ts">
  let { version, oninstall, onlater }: { version: string; oninstall: () => Promise<unknown>; onlater: () => void } =
    $props();

  let installing = $state(false);

  async function install() {
    installing = true;
    await oninstall();
    // Only reached when it failed; the error shows above.
    installing = false;
  }
</script>

<div role="status" class="flex items-center gap-3 rounded-xl bg-tonal px-4 py-3 text-sm text-on-tonal">
  <p class="flex-1">
    {installing ? `Downloading Caraxyn ${version}… the app restarts when it is installed.` : `Caraxyn ${version} is available.`}
  </p>
  {#if !installing}
    <button class="btn" onclick={onlater}>Later</button>
    <button class="btn btn-primary" onclick={install}>Update</button>
  {/if}
</div>
