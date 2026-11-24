/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { VitePWA } from 'vite-plugin-pwa';

// The built app is embedded in the server binary (server/src/web.rs) and served from `/`.
// In `npm run dev` the API is proxied to a local server (default 127.0.0.1:8080).
const apiTarget = process.env['NIMRAG_API'] ?? 'http://127.0.0.1:8080';

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: 'autoUpdate',
      injectRegister: false,
      manifest: {
        name: 'Nimrag Connect',
        short_name: 'Nimrag',
        description: 'Your Garmin watch data on your own server.',
        start_url: '/',
        scope: '/',
        display: 'standalone',
        background_color: '#ffffff',
        theme_color: '#ffffff',
        icons: [
          { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
          { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
          { src: '/icon-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
          { src: '/icon.svg', sizes: 'any', type: 'image/svg+xml' },
        ],
      },
      workbox: {
        // Offline shell: precache the built assets, answer navigations with index.html.
        globPatterns: ['**/*.{js,css,html,svg,png,woff2}'],
        // The map engine is large and needs the tile server anyway: loaded on demand, not precached.
        globIgnores: ['**/maplibre-gl-*'],
        navigateFallback: '/index.html',
        navigateFallbackDenylist: [/^\/api\//, /^\/tiles\//],
        cleanupOutdatedCaches: true,
        // Take over the page that registered the worker. Without this the worker only controls the
        // next page load, and the Android app keeps one page open for days (and reloads it with no
        // worker after a server update), so nothing reached the offline API cache.
        clientsClaim: true,
        runtimeCaching: [
          {
            // Offline viewing: network first, fall back to the last good response.
            // Credentialed same-origin GETs only; mutations are never cached. The data export
            // (a zip that can be hundreds of MB) is a download, not a page: it bypasses the cache.
            urlPattern: ({ url, request }) =>
              url.pathname.startsWith('/api/') && url.pathname !== '/api/export' && request.method === 'GET',
            handler: 'NetworkFirst',
            options: {
              cacheName: 'nimrag-api',
              networkTimeoutSeconds: 5,
              expiration: { maxEntries: 200, maxAgeSeconds: 60 * 60 * 24 * 30 },
              cacheableResponse: { statuses: [200] },
            },
          },
        ],
      },
    }),
  ],
  build: {
    target: 'es2022',
    sourcemap: false,
    // MapLibre is a lazy chunk (only pages with a map load it).
    chunkSizeWarningLimit: 1200,
  },
  server: {
    proxy: {
      '/api': apiTarget,
      '/tiles': apiTarget,
    },
  },
  // `vite preview` serves the built files only: no proxy, so /api and /tiles are plain SPA
  // fallbacks there and the Playwright tests mock the API themselves.
  preview: { proxy: {} },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
