<script lang="ts">
  // Shown instead of the app when the node could not start. Nothing is deleted.
  let { error, dataDir }: { error: string; dataDir: string } = $props();
</script>

<main class="mx-auto flex h-full max-w-lg flex-col justify-center gap-4 p-8">
  <h1 class="text-xl font-semibold">Caraxyn could not start</h1>
  <p role="alert" class="rounded-xl bg-danger-container p-4 text-sm text-on-danger-container">{error}</p>
  <p class="text-sm text-muted">
    Your data is in the folder below and was left untouched. If it was made by another version of Caraxyn,
    move the folder somewhere else and start again to begin fresh, or use the matching version.
  </p>
  <code class="card block font-mono text-xs break-all">{dataDir}</code>
</main>
