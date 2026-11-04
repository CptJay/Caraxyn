import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  // Tauri prints its own output in the same terminal.
  clearScreen: false,
  // Must match `build.devUrl` in tauri.conf.json.
  server: { port: 5173, strictPort: true },
});
