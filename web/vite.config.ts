import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// `npm run dev` serves the page with hot reload and forwards /api to a running `respak-web`
// (default port 7878: start it with `respak-web --port 7878 --no-open`).
const api = process.env.RESPAK_API ?? 'http://127.0.0.1:7878';

export default defineConfig({
  plugins: [svelte()],
  build: { outDir: 'dist', emptyOutDir: true, target: 'es2022' },
  server: {
    proxy: {
      '/api': {
        target: api,
        changeOrigin: true,
        // The server refuses foreign origins; the dev page is served from another port.
        configure: (proxy) => proxy.on('proxyReq', (req) => req.removeHeader('origin')),
      },
    },
  },
});
