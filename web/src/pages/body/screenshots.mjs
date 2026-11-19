// Screenshots and an axe check of the Movement, Training, Body and Calendar pages against a running
// server. Run from web/:
//   PLAYWRIGHT_BASE_URL=http://127.0.0.1:5114 NIMRAG_PASSWORD=... node src/pages/body/screenshots.mjs
// Output: web/screenshots/{movement,training,body,body-form,calendar}-{390,1360}.png (gitignored).
import AxeBuilder from '@axe-core/playwright';
import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';

const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://127.0.0.1:5114';
const password = process.env.NIMRAG_PASSWORD ?? 'nimrag-dev-pw';
const sizes = [
  { name: '390', width: 390, height: 844 },
  { name: '1360', width: 1360, height: 1000 },
];
const pages = [
  { name: 'movement', path: '/movement' },
  { name: 'training', path: '/training' },
  { name: 'body', path: '/body' },
  { name: 'body-form', path: '/body', before: async (page) => page.getByRole('button', { name: 'Add entry' }).first().click() },
  { name: 'calendar', path: '/calendar' },
];

mkdirSync('screenshots', { recursive: true });
const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM_PATH ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH } : {});
let violations = 0;
for (const size of sizes) {
  const context = await browser.newContext({ viewport: { width: size.width, height: size.height }, serviceWorkers: 'block' });
  const login = await context.request.post(`${base}/api/login`, { headers: { 'X-Nimrag': '1' }, data: { password } });
  if (!login.ok()) throw new Error(`login failed: ${login.status()}`);
  for (const p of pages) {
    const page = await context.newPage();
    await page.goto(`${base}${p.path}`);
    await page.getByRole('heading', { level: 1 }).first().waitFor();
    await page.waitForLoadState('networkidle');
    if (p.before) await p.before(page);
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: `screenshots/${p.name}-${size.name}.png`, fullPage: true });
    const result = await new AxeBuilder({ page }).analyze();
    for (const v of result.violations) {
      violations += 1;
      console.log(`${p.name} @${size.name}: ${v.id} (${v.impact}) ${v.help}: ${v.nodes.map((n) => n.target.join(' ')).join(' | ')}`);
    }
    await page.close();
  }
  await context.close();
}
await browser.close();
console.log(violations === 0 ? 'axe: no violations' : `axe: ${violations} violations`);
