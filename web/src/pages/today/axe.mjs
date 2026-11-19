// Accessibility check of the Today page with axe, against a seeded dev server.
//   PLAYWRIGHT_BASE_URL=http://127.0.0.1:5111 node src/pages/today/axe.mjs
import { chromium } from '@playwright/test';
import { AxeBuilder } from '@axe-core/playwright';

const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://127.0.0.1:5111';
const password = process.env.NIMRAG_PASSWORD ?? 'password-1234';
const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM_PATH ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH } : {});
const context = await browser.newContext({ serviceWorkers: 'block' });
await context.request.post(`${base}/api/login`, { data: { password }, headers: { 'X-Nimrag': '1' } });
let failed = false;
for (const [w, h] of [[390, 844], [1360, 1000]]) {
  const page = await context.newPage();
  await page.setViewportSize({ width: w, height: h });
  await page.goto(base + '/');
  await page.waitForSelector('.stat-link');
  if (w > 1000) await page.waitForSelector('.uplot canvas');
  await page.waitForTimeout(500);
  for (const edit of [false, true]) {
    if (edit) {
      await page.getByRole('button', { name: 'Edit home' }).click();
      await page.getByRole('dialog').waitFor();
    }
    const r = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
    if (r.violations.length) failed = true;
    console.log(w, edit ? 'edit sheet' : 'page', r.violations.map((v) => `${v.id} (${v.nodes.length}): ${v.nodes[0].html.slice(0, 140)}`));
  }
  await page.close();
}
await browser.close();
process.exit(failed ? 1 : 0);
