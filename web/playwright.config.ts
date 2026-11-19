import { defineConfig, devices } from '@playwright/test';

// Smoke tests run against `vite preview` of the production build (the real-server
// smoke test, against the embedded binary, comes later). `PLAYWRIGHT_BASE_URL` points
// the same tests at a running server instead and skips the preview server.
const external = process.env['PLAYWRIGHT_BASE_URL'];
const baseURL = external ?? 'http://127.0.0.1:4173';

export default defineConfig({
  testDir: './e2e',
  outputDir: './test-results',
  fullyParallel: true,
  forbidOnly: !!process.env['CI'],
  retries: process.env['CI'] ? 1 : 0,
  reporter: [['list']],
  use: {
    baseURL,
    trace: 'retain-on-failure',
    // Use a browser from PLAYWRIGHT_CHROMIUM_PATH when the matching download is not installed.
    launchOptions: process.env['PLAYWRIGHT_CHROMIUM_PATH']
      ? { executablePath: process.env['PLAYWRIGHT_CHROMIUM_PATH'] }
      : {},
  },
  projects: [
    { name: 'smoke', testMatch: /smoke\.spec\.ts/, use: { ...devices['Desktop Chrome'], serviceWorkers: 'block' } },
    { name: 'screenshots', testMatch: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'] } },
  ],
  webServer: external
    ? undefined
    : {
        command: 'npm run build && npx vite preview --host 127.0.0.1 --port 4173 --strictPort',
        url: baseURL,
        reuseExistingServer: !process.env['CI'],
        timeout: 180_000,
      },
});
