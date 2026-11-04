<script lang="ts" module>
  // 24×24 stroke icons (shapes after Lucide, ISC license). Add new ones here.
  const PATHS = {
    inbox: [
      'M22 12h-6l-2 3h-4l-2-3H2',
      'M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z',
    ],
    share: ['M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8', 'M16 6l-4-4-4 4', 'M12 2v13'],
    contacts: [
      'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2',
      'M13 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0',
      'M22 21v-2a4 4 0 0 0-3-3.87',
      'M16 3.13a4 4 0 0 1 0 7.75',
    ],
    settings: ['M4 21v-7', 'M4 10V3', 'M12 21v-9', 'M12 8V3', 'M20 21v-5', 'M20 12V3', 'M1 14h6', 'M9 8h6', 'M17 16h6'],
    collapse: ['M11 17l-5-5 5-5', 'M18 17l-5-5 5-5'],
    expand: ['M6 17l5-5-5-5', 'M13 17l5-5-5-5'],
  };

  export type IconName = keyof typeof PATHS;
</script>

<script lang="ts">
  // Decorative: the button or link around it carries the accessible name.
  let { name, class: className = 'size-4' }: { name: IconName; class?: string } = $props();
</script>

<svg
  class={className}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#each PATHS[name] as d (d)}
    <path {d} />
  {/each}
</svg>
