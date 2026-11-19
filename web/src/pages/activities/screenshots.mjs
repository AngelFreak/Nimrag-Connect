// Screenshots of the activity pages against a seeded server (see the stage brief).
// Run from web/: PLAYWRIGHT_BASE_URL=http://127.0.0.1:5112 NIMRAG_PASSWORD=... node src/pages/activities/screenshots.mjs
import { chromium } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://127.0.0.1:5112';
const password = process.env.NIMRAG_PASSWORD ?? 'password-1234';
const SIZES = [
  { name: '390', width: 390, height: 844 },
  { name: '1360', width: 1360, height: 1000 },
];

const browser = await chromium.launch();
const context = await browser.newContext({ baseURL: base });
const login = await context.request.post('/api/login', { data: { password }, headers: { 'X-Nimrag': '1' } });
if (!login.ok()) throw new Error(`login failed: ${login.status()}`);

const list = await (await context.request.get('/api/activities?limit=200')).json();
const run = list.items.find((a) => a.kind === 'run');
const strength = list.items.find((a) => a.kind === 'strength');
const walk = list.items.find((a) => a.kind === 'walk');

const pages = [
  { name: 'activities', path: '/activities', full: true },
  { name: 'activities-strength-filter', path: '/activities?type=strength', full: false },
  { name: 'activity-run', path: `/activities/${run.id}`, full: true },
  walk ? { name: 'activity-walk', path: `/activities/${walk.id}`, full: true } : null,
  { name: 'strength', path: `/activities/${strength.id}/strength`, full: true },
  { name: 'workout', path: '/workouts/new', full: false },
].filter(Boolean);

for (const size of SIZES) {
  const page = await context.newPage();
  await page.setViewportSize({ width: size.width, height: size.height });
  for (const p of pages) {
    await page.goto(p.path);
    await page.getByRole('heading', { level: 1 }).first().waitFor();
    await page.waitForLoadState('networkidle');
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(400);
    await page.screenshot({ path: `screenshots/${p.name}-${size.name}.png`, fullPage: p.full });
    const axe = await new AxeBuilder({ page }).analyze();
    for (const v of axe.violations) console.log(`axe ${p.name} ${size.name}: ${v.id} (${v.impact}) x${v.nodes.length}: ${v.nodes[0]?.html.slice(0, 120)}`);
  }
  // Strength: edit mode and comparison.
  await page.goto(`/activities/${strength.id}/strength`);
  await page.getByRole('button', { name: 'Compare to last' }).click();
  await page.getByText('Compared with').first().waitFor();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: `screenshots/strength-compare-${size.name}.png`, fullPage: true });
  await page.getByRole('button', { name: 'Hide comparison' }).click();
  await page.getByRole('button', { name: 'Edit sets' }).click();
  await page.screenshot({ path: `screenshots/strength-edit-${size.name}.png`, fullPage: true });
  await page.close();
}
await browser.close();
