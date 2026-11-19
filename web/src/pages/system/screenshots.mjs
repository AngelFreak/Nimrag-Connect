// Screenshots of Devices and Settings against a seeded dev server.
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
  await shot('/devices', `devices-${w}`, w, h);
  await shot('/settings', `settings-${w}`, w, h);
  await shot('/settings', `settings-token-${w}`, w, h, async (page) => {
    await page.getByRole('button', { name: 'Create token' }).click();
    await page.getByLabel('Name', { exact: true }).fill('Screenshot phone');
    await page.getByLabel('Your password').fill(password);
    await page.getByRole('button', { name: 'Create token' }).last().click();
    await page.getByText('shown only once').waitFor();
  });
}
await browser.close();
