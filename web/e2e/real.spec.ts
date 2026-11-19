import fs from 'node:fs';
import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { fixtureFile, ORIGIN, type Fixture } from './real-server';

// Every route against the real nimrag-server with dev-seed data (see real.setup.ts): each page
// must render, make no failing request, log no console error, and pass axe (WCAG 2 A and AA) at
// phone and desktop width.

const ROUTES: [name: string, path: (f: Fixture) => string][] = [
  ['Today', () => '/'],
  ['Activities', () => '/activities'],
  ['Activities, strength filter', () => '/activities?type=strength'],
  ['Strength list', () => '/strength'],
  ['Activity detail', (f) => `/activities/${f.runId}`],
  ['Strength session', (f) => `/activities/${f.strengthId}/strength`],
  ['Sleep, latest night', () => '/sleep'],
  ['Sleep, one night', (f) => `/sleep/${f.night}`],
  ['Day, today', () => '/day'],
  ['Day, one day', (f) => `/day/${f.night}`],
  ['Health', () => '/health'],
  ['Movement, today', () => '/movement'],
  ['Movement, one day', (f) => `/movement/${f.night}`],
  ['Training', () => '/training'],
  ['Body', () => '/body'],
  ['Calendar, this month', () => '/calendar'],
  ['Calendar, one month', (f) => `/calendar/${f.month}`],
  ['New workout', () => '/workouts/new'],
  ['Devices', () => '/devices'],
  ['Settings', () => '/settings'],
  ['Page not found', () => '/does-not-exist'],
];

// WCAG 2 A and AA (with the 2.1 additions), plus axe's best-practice rules (landmarks, heading order).
const AXE_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'];

const VIEWPORTS = [
  { name: 'phone 390', width: 390, height: 844 },
  { name: 'desktop 1360', width: 1360, height: 1000 },
];

/** Collects everything that should never happen on a healthy page. */
function watch(page: Page) {
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(`page error: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() !== 'error') return;
    // A server without a tile file answers the map probe with 404: expected, the page shows a placeholder.
    if (m.location().url.includes('/tiles/')) return;
    problems.push(`console error: ${m.text()}`);
  });
  page.on('requestfailed', (r) => {
    // A query cancelled by leaving the page is not a failure.
    if (r.failure()?.errorText === 'net::ERR_ABORTED') return;
    problems.push(`request failed: ${r.method()} ${r.url()} (${r.failure()?.errorText})`);
  });
  page.on('response', (r) => {
    if (r.status() < 400 || new URL(r.url()).pathname.startsWith('/tiles/')) return;
    problems.push(`HTTP ${r.status()}: ${r.request().method()} ${r.url()}`);
  });
  page.on('request', (r) => {
    if (r.url().startsWith('data:') || r.url().startsWith('blob:')) return;
    if (new URL(r.url()).origin !== ORIGIN) problems.push(`third-party request: ${r.url()}`);
  });
  return problems;
}

/** The page has its heading, nothing is loading any more, and charts have drawn. */
async function settled(page: Page, heading?: string) {
  const h1 = page.getByRole('heading', { level: 1 }).first();
  await expect(h1).toBeVisible();
  if (heading) await expect(h1).toContainText(heading);
  await page.waitForLoadState('networkidle');
  await expect(page.locator('[aria-busy="true"]')).toHaveCount(0);
  await expect(page.locator('.skeleton')).toHaveCount(0);
  await page.evaluate(() => document.fonts.ready);
}

let fixture: Fixture;
test.beforeAll(() => {
  fixture = JSON.parse(fs.readFileSync(fixtureFile, 'utf8')) as Fixture;
});

for (const vp of VIEWPORTS) {
  test.describe(vp.name, () => {
    test.use({ viewport: { width: vp.width, height: vp.height } });

    for (const [name, path] of ROUTES) {
      test(`${name} renders and passes axe`, async ({ page }) => {
        const problems = watch(page);
        await page.goto(path(fixture));
        await settled(page);
        // A real route is not the "page not found" page, and the not-found page is.
        const notFound = (await page.getByRole('heading', { level: 1 }).first().innerText()) === 'Page not found';
        expect(notFound, 'the Page not found screen').toBe(name === 'Page not found');
        // The error screens of the pages are failures here: the seeded server must answer every call.
        await expect(page.getByRole('alert')).toHaveCount(0);

        const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
        const violations = results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).slice(0, 4).join(' | ')}`);
        expect(violations, 'axe violations').toEqual([]);
        expect(problems, 'console errors and failed requests').toEqual([]);
      });
    }
  });
}

test('the session works: the API answers 401 without it, and the app sends the user to the login page', async ({ browser }) => {
  const context = await browser.newContext({ storageState: { cookies: [], origins: [] }, baseURL: ORIGIN });
  const page = await context.newPage();
  const res = await page.request.get('/api/today');
  expect(res.status()).toBe(401);
  await page.goto('/activities');
  await expect(page).toHaveURL(/\/login\?next=/);
  await expect(page.getByRole('heading', { level: 1, name: 'Nimrag Connect' })).toBeVisible();
  await context.close();
});

test('Settings: a new device token is shown once, with a warning', async ({ page }) => {
  await page.goto('/settings');
  await settled(page);
  await page.getByRole('button', { name: 'Create token' }).click();
  await page.getByLabel('Name', { exact: true }).fill('e2e phone');
  await page.getByLabel('Your password').fill('wrong password here');
  await page.getByRole('button', { name: 'Create token' }).last().click();
  await expect(page.getByRole('alert')).toHaveText('Wrong password.');
  // Still logged in: a wrong token password is 403, not 401.
  await expect(page).toHaveURL(/\/settings$/);
  await page.getByLabel('Your password').fill('nimrag-e2e-password');
  await page.getByRole('button', { name: 'Create token' }).last().click();
  await expect(page.getByText('shown only once')).toBeVisible();
  const secret = await page.getByLabel('Token', { exact: true }).inputValue();
  expect(secret.length).toBeGreaterThan(20);
  await page.getByRole('button', { name: 'I have saved it' }).click();
  await expect(page.getByText(secret)).toHaveCount(0);
  await expect(page.getByRole('table', { name: 'Device tokens' }).getByText('e2e phone')).toBeVisible();
  // Clean up the token so reruns against a kept data dir stay tidy.
  await page.getByRole('button', { name: 'Revoke token e2e phone' }).click();
  await page.getByRole('button', { name: 'Confirm revoking token e2e phone' }).click();
  await expect(page.getByRole('button', { name: 'Revoke token e2e phone' })).toHaveCount(0);
});

test('Settings: changing the step goal is saved and shown on Movement', async ({ page }) => {
  await page.goto('/settings');
  await settled(page);
  const field = page.getByLabel('Daily step goal');
  const before = await field.inputValue();
  await field.fill('9500');
  await page.getByRole('button', { name: 'Save settings' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
  await page.goto('/movement');
  await settled(page);
  await expect(page.getByText('Goal 9,500')).toBeVisible();
  await page.goto('/settings');
  await page.getByLabel('Daily step goal').fill(before);
  await page.getByRole('button', { name: 'Save settings' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
});

test('Devices shows the seeded server and watch data', async ({ page }) => {
  await page.goto('/devices');
  await settled(page, 'Devices and sync');
  await expect(page.getByRole('heading', { name: 'Your watch' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Your server' })).toBeVisible();
  await expect(page.getByText('Reachable')).toBeVisible();
});
