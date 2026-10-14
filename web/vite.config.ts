import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// `npm run dev` serves the app on :5173 and forwards /api to `grocery serve`
// on :8080 (override with GROCERY_API).
export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      '/api': { target: process.env.GROCERY_API ?? 'http://127.0.0.1:8080', changeOrigin: true },
    },
  },
});
