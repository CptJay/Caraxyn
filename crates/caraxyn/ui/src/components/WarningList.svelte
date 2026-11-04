<script lang="ts">
  let { warnings, onclear }: { warnings: string[]; onclear: () => void } = $props();
</script>

<section class="rounded-md border border-warn/40 bg-warn/10 px-3 py-2 text-sm">
  <div class="mb-1 flex items-center justify-between">
    <h2 class="font-medium">Background problems</h2>
    <button class="text-muted hover:text-fg" onclick={onclear}>Clear</button>
  </div>
  <ul class="list-disc space-y-0.5 pl-5">
    {#each warnings as warning, i (i)}
      <li>{warning}</li>
    {/each}
  </ul>
</section>
