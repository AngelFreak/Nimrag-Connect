import { defineConfig, devices } from '@playwright/test';
import { ORIGIN, sessionFile } from './e2e/real-server';

// Two kinds of end-to-end test (`npm run test:e2e` runs both):
//
// * `smoke`: against `vite preview` of the production build, with the API mocked by the tests
//   (gallery, login page). `PLAYWRIGHT_BASE_URL` points these at a running server instead and
//   skips the preview server.
// * `real`: against the real `nimrag-server` binary with dev-seed data. `real-setup` builds the
//   web app and the server, seeds a temp data dir, starts the server on 127.0.0.1:18121 and logs
//   in; `real-teardown` stops it. Every route is checked for errors and accessibility.
const external = process.env['PLAYWRIGHT_BASE_URL'];
const baseURL = external ?? 'http://127.0.0.1:4173';

// Use a browser from PLAYWRIGHT_CHROMIUM_PATH when the matching download is not installed.
const launchOptions = process.env['PLAYWRIGHT_CHROMIUM_PATH'] ? { executablePath: process.env['PLAYWRIGHT_CHROMIUM_PATH'] } : {};

export default defineConfig({
  testDir: './e2e',
  outputDir: './test-results',
  fullyParallel: true,
  // Each worker is a Chromium; the dev machine is short on memory.
  workers: 2,
  timeout: 60_000,
  forbidOnly: !!process.env['CI'],
  // One retry: a Chromium renderer can be killed when the machine runs out of memory.
  retries: 1,
  reporter: [['list']],
  use: {
    baseURL,
    trace: 'retain-on-failure',
    launchOptions,
  },
  projects: [
    { name: 'smoke', testMatch: /smoke\.spec\.ts/, use: { ...devices['Desktop Chrome'], serviceWorkers: 'block' } },
    { name: 'screenshots', testMatch: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'] } },
    { name: 'real-setup', testMatch: /real\.setup\.ts/, teardown: 'real-teardown', use: { baseURL: ORIGIN } },
    { name: 'real-teardown', testMatch: /real\.teardown\.ts/ },
    {
      name: 'real',
      testMatch: /real\.spec\.ts/,
      dependencies: ['real-setup'],
      use: { ...devices['Desktop Chrome'], baseURL: ORIGIN, storageState: sessionFile, serviceWorkers: 'block' },
    },
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
