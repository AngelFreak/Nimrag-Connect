// Screenshots of the Watch page and the profile pages against a seeded dev server.
//   PLAYWRIGHT_BASE_URL=http://127.0.0.1:5121 NIMRAG_PASSWORD=... node src/pages/system/screenshots.mjs
// Writes web/screenshots/system-*.png (gitignored). Not part of the test suite.
import { chromium } from '@playwright/test';

const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://127.0.0.1:5121';
const password = process.env.NIMRAG_PASSWORD ?? 'nimrag-e2e-pw';
const out = 'screenshots';

const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM_PATH ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH } : {});
const context = await browser.newContext({ serviceWorkers: 'block' });
const login = await context.request.post(`${base}/api/login`, { data: { password }, headers: { 'X-Nimrag': '1' } });
if (!login.ok()) throw new Error(`login failed: ${login.status()}`);

async function shot(path, name, width, height, steps) {
  const page = await context.newPage();
  await page.setViewportSize({ width, height });
  await page.goto(base + path);
  await page.getByRole('heading', { level: 1 }).first().waitFor();
  await page.waitForLoadState('networkidle');
  await page.evaluate(() => document.fonts.ready);
  if (steps) await steps(page);
  await page.screenshot({ path: `${out}/system-${name}.png`, fullPage: true });
  await page.close();
}

for (const [w, h] of [
  [390, 844],
  [1360, 1000],
]) {
  await shot('/devices', `watch-${w}`, w, h);
  await shot('/devices/details', `watch-details-${w}`, w, h);
  await shot('/profile', `profile-${w}`, w, h);
  await shot('/profile/phones', `phones-${w}`, w, h);
  await shot('/profile/goals', `goals-${w}`, w, h);
  await shot('/profile/garmin', `garmin-${w}`, w, h);
}
await browser.close();
