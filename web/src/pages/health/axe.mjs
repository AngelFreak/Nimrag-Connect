// Accessibility scan (axe) of Sleep, Day and Health against a running dev server.
// Usage (from web/): PLAYWRIGHT_BASE_URL=http://127.0.0.1:5113 NIMRAG_PASSWORD=... node src/pages/health/axe.mjs
import AxeBuilder from '@axe-core/playwright';
import { chromium } from '@playwright/test';

const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://127.0.0.1:5113';
const password = process.env.NIMRAG_PASSWORD ?? '';
const date = process.env.NIMRAG_DATE ?? '2026-11-18';
const paths = [`/sleep/${date}`, `/day/${date}`, '/health'];

const browser = await chromium.launch();
let failed = 0;
for (const viewport of [{ width: 390, height: 844 }, { width: 1360, height: 900 }]) {
  const context = await browser.newContext({ viewport, baseURL: base, serviceWorkers: 'block' });
  await context.request.post('/api/login', { data: { password }, headers: { 'X-Nimrag': '1' } });
  const page = await context.newPage();
  for (const path of paths) {
    await page.goto(path);
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(300);
    const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa']).analyze();
    console.log(`${viewport.width} ${path}: ${result.violations.length} violations`);
    for (const v of result.violations) {
      failed += 1;
      console.log(`  ${v.id} (${v.impact}): ${v.help}; ${v.nodes.length} node(s), e.g. ${v.nodes[0]?.html.slice(0, 120)}`);
    }
  }
  await context.close();
}
await browser.close();
process.exit(failed ? 1 : 0);
