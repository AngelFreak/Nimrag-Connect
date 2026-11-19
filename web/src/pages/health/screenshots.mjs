// Review screenshots of Sleep, Day and Health at phone (390) and desktop (1360) width.
// Usage (from web/): PLAYWRIGHT_BASE_URL=http://127.0.0.1:5113 NIMRAG_PASSWORD=... node src/pages/health/screenshots.mjs
// Writes web/screenshots/health-*.png (gitignored).
import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';

const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://127.0.0.1:5113';
const password = process.env.NIMRAG_PASSWORD ?? '';
const date = process.env.NIMRAG_DATE ?? '2026-11-18';
const out = new URL('../../../screenshots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

const pages = [
  ['sleep', `/sleep/${date}`],
  ['sleep-latest', '/sleep'],
  ['day', `/day/${date}`],
  ['health', '/health'],
  ['health-1y', '/health'],
  ['sleep-30', `/sleep/${date}`],
];
const sizes = [
  ['390', { width: 390, height: 844 }],
  ['1360', { width: 1360, height: 900 }],
];

const browser = await chromium.launch();
for (const [sizeName, viewport] of sizes) {
  const context = await browser.newContext({ viewport, baseURL: base, serviceWorkers: 'block' });
  const login = await context.request.post('/api/login', { data: { password }, headers: { 'X-Nimrag': '1' } });
  if (!login.ok()) throw new Error(`login failed: ${login.status()}`);
  const page = await context.newPage();
  for (const [name, path] of pages) {
    if (name === 'health-1y') {
      await page.goto('/health');
      await page.getByRole('button', { name: '1 year' }).click();
    } else {
      await page.goto(path);
      if (name === 'sleep-30') await page.getByRole('button', { name: '30 nights' }).click();
    }
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(400);
    const file = `${out}health-${name}-${sizeName}.png`;
    await page.screenshot({ path: file, fullPage: true });
    console.log(file);
  }
  await context.close();
}
await browser.close();
