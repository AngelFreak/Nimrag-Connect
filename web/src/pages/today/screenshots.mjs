// Screenshots of the Today page against a seeded dev server.
//   PLAYWRIGHT_BASE_URL=http://127.0.0.1:5111 NIMRAG_PASSWORD=... node src/pages/today/screenshots.mjs
// Writes web/screenshots/today-*.png (gitignored). Not part of the test suite.
import { chromium } from '@playwright/test';

const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://127.0.0.1:5111';
const password = process.env.NIMRAG_PASSWORD ?? 'password-1234';
const out = 'screenshots';

const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM_PATH ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH } : {});
const context = await browser.newContext({ serviceWorkers: 'block' });
const login = await context.request.post(`${base}/api/login`, { data: { password }, headers: { 'X-Nimrag': '1' } });
if (!login.ok()) throw new Error(`login failed: ${login.status()}`);

async function shot(name, width, height, steps) {
  const page = await context.newPage();
  await page.setViewportSize({ width, height });
  await page.goto(base + '/');
  await page.getByRole('heading', { level: 1 }).first().waitFor();
  await page.waitForSelector('.glance, .state--error', { timeout: 15000 });
  if (width >= 1024) await page.waitForSelector('.uplot canvas', { timeout: 15000 });
  await page.evaluate(() => document.fonts.ready);
  if (steps) await steps(page);
  await page.screenshot({ path: `${out}/today-${name}.png`, fullPage: !name.endsWith('-edit') });
  await page.close();
}

await shot('390', 390, 844);
await shot('390-slide-sleep', 390, 844, async (page) => {
  await page.getByRole('button', { name: /Show Sleep/ }).click();
  await page.waitForTimeout(700);
});
await shot('390-slide-hrv', 390, 844, async (page) => {
  await page.getByRole('button', { name: /Show HRV/ }).click();
  await page.waitForTimeout(700);
});
await shot('390-edit', 390, 844, async (page) => {
  await page.getByRole('button', { name: 'Edit home' }).click();
  await page.getByRole('dialog', { name: 'Edit home' }).waitFor();
});
await shot('1360', 1360, 1120);
await shot('1360-edit', 1360, 1000, async (page) => {
  await page.getByRole('button', { name: 'Edit home' }).click();
  await page.getByRole('dialog', { name: 'Edit home' }).waitFor();
});
await browser.close();
