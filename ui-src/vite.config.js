import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Builds one ES module, ui/dist/library.js, with a fixed name so the Rust
// binary can embed it (src/web.rs, UI table). The vanilla modules under /js/
// stay outside the bundle and are imported at runtime, so both halves of the
// app share one live state (state.js).
export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: '../ui/dist',
    emptyOutDir: true,
    target: 'es2022',
    minify: true,
    lib: { entry: 'src/library.js', formats: ['es'], fileName: () => 'library.js' },
    // Library builds skip minifying unless the output asks for it.
    rollupOptions: { external: [/^\/js\//], output: { minify: true } },
  },
  // `npm run dev`: hot reload against a running plv (cargo run … on :8080).
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8080',
      '/ws': { target: 'ws://127.0.0.1:8080', ws: true },
      '/js': 'http://127.0.0.1:8080',
      '/app.css': 'http://127.0.0.1:8080',
      '/icon.svg': 'http://127.0.0.1:8080',
    },
  },
});
