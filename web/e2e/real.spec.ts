import fs from 'node:fs';
import { expect, test } from '@playwright/test';
import { expectAxeClean, settled, VIEWPORTS, watch } from './helpers';
import { DEMO, fixtureFile, ORIGIN, type Fixture } from './real-server';

// Every route against the real nimrag-server with dev-seed data (see real.setup.ts), signed in as
// the seeded user `demo` (the session saved by the setup project): each page must render, make no
// failing request, log no console error, and pass axe (WCAG 2 A and AA) at phone and desktop width.

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

let fixture: Fixture;
test.beforeAll(() => {
  fixture = JSON.parse(fs.readFileSync(fixtureFile, 'utf8')) as Fixture;
});

for (const vp of VIEWPORTS) {
  test.describe(vp.name, () => {
    test.use({ viewport: { width: vp.width, height: vp.height } });

    for (const [name, path] of ROUTES) {
      test(`${name} renders and passes axe`, async ({ page }) => {
        const problems = watch(page, ORIGIN);
        await page.goto(path(fixture));
        await settled(page);
        // A real route is not the "page not found" page, and the not-found page is.
        const notFound = (await page.getByRole('heading', { level: 1 }).first().innerText()) === 'Page not found';
        expect(notFound, 'the Page not found screen').toBe(name === 'Page not found');
        // The error screens of the pages are failures here: the seeded server must answer every call.
        await expect(page.getByRole('alert')).toHaveCount(0);

        await expectAxeClean(page);
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

test('the demo user is a normal user: the profile menu says so, and /admin is not found', async ({ page }) => {
  await page.goto('/');
  await settled(page);
  await page.getByRole('button', { name: 'Profile and account' }).first().click();
  const menu = page.getByRole('group', { name: 'Account' }).first();
  await expect(menu).toContainText('demo');
  await expect(menu).toContainText('User');
  await expect(menu.getByRole('link', { name: 'Settings' })).toBeVisible();
  await expect(menu.getByRole('link', { name: 'Admin area' })).toHaveCount(0);

  await page.goto('/admin');
  await settled(page, 'Page not found');
  expect((await page.request.get('/api/admin/users')).status()).toBe(403);
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
  await page.getByLabel('Your password').fill(DEMO.password);
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
