import fs from 'node:fs';
import { expect, test } from '@playwright/test';
import { expectAxeClean, settled, VIEWPORTS, watch } from './helpers';
import { DEMO, fixtureFile, ORIGIN, type Fixture } from './real-server';
import { nextTotp } from './totp';

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
  ['Sleep score, 7 days', () => '/sleep?range=7d'],
  ['Sleep score, 4 weeks', () => '/sleep?range=4w'],
  ['Sleep score, 1 year', () => '/sleep?range=1y'],
  ['Sleep score, an earlier week', (f) => `/sleep?range=7d&end=${f.night}`],
  ['Day, today', () => '/day'],
  ['Day, one day', (f) => `/day/${f.night}`],
  ['Health', () => '/health'],
  ['Movement, today', () => '/movement'],
  ['Movement, one day', (f) => `/movement/${f.night}`],
  ['Training', () => '/training'],
  ['Training status, current', () => '/training/status'],
  ['Training status, 4 weeks', () => '/training/status?range=4w'],
  ['Training status, 12 weeks', () => '/training/status?range=12w'],
  ['Training status, 6 months', () => '/training/status?range=6m'],
  ['HRV status, 7 days', () => '/hrv'],
  ['HRV status, 4 weeks', () => '/hrv?range=4w'],
  ['HRV status, one night', () => '/hrv?range=1d'],
  ['Metric, heart_rate, 7 days', () => '/metrics/heart_rate?range=7d'],
  ['Metric, stress, 7 days', () => '/metrics/stress?range=7d'],
  ['Metric, energy, 7 days', () => '/metrics/energy?range=7d'],
  ['Metric, steps, 7 days', () => '/metrics/steps?range=7d'],
  ['Metric, intensity_minutes, 7 days', () => '/metrics/intensity_minutes?range=7d'],
  ['Metric, respiration, 7 days', () => '/metrics/respiration?range=7d'],
  ['Metric, spo2, 7 days', () => '/metrics/spo2?range=7d'],
  ['Metric, steps, 1 year', () => '/metrics/steps?range=1y'],
  ['Metric, steps, one day', () => '/metrics/steps?range=1d'],
  ['Metric, training load, 12 weeks', () => '/metrics/training_load?range=12w'],
  ['Metric, VO2 max, 6 months', () => '/metrics/vo2_max?range=6m'],
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

test('Settings: a phone that signed in is listed (no token), fits 320 px, and can be signed out', async ({ page, playwright }) => {
  // The app's sign-in issues the phone's token in the background; nothing on the web page makes or shows one.
  const phone = await playwright.request.newContext({ baseURL: ORIGIN });
  const name = 'Pixel-9a-pro-max-ultra-with-a-very-long-unbroken-name-12345678';
  const login = await phone.post('/api/app/login', {
    headers: { 'X-Nimrag': '1' },
    data: { username: DEMO.username, password: DEMO.password, code: await nextTotp(DEMO.totp), device_name: name },
  });
  expect(login.status()).toBe(201);
  const issued = (await login.json()) as { token: string; device_secret: string };
  await phone.dispose();

  await page.setViewportSize({ width: 320, height: 700 });
  await page.goto('/settings');
  await settled(page);
  await expect(page.getByRole('heading', { name: 'Signed-in phones' })).toBeVisible();
  const list = page.getByRole('list', { name: 'Signed-in phones' });
  await expect(list.getByText(name)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Create token' })).toHaveCount(0);
  await expect(page.getByText(/device token/i)).toHaveCount(0);
  await expect(page.getByText(issued.token)).toHaveCount(0);
  await expect(page.getByText(issued.device_secret)).toHaveCount(0);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await expectAxeClean(page);
  // A user cannot mint a token over the web.
  const made = await page.request.post('/api/tokens', { headers: { 'X-Nimrag': '1' }, data: { name: 'x', password: DEMO.password } });
  expect(made.status()).toBe(405);

  await page.getByRole('button', { name: `Sign out this phone, ${name}` }).click();
  await expectAxeClean(page);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
  await page.getByRole('button', { name: `Yes, sign it out, ${name}` }).click();
  await expect(list.getByText(name)).toHaveCount(0);
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
