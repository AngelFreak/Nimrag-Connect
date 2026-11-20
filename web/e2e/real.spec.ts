import fs from 'node:fs';
import { expect, test } from '@playwright/test';
import { expectAxeClean, openProfile, settled, VIEWPORTS, watch } from './helpers';
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
  ['Body', () => '/body'],
  ['Calendar, this month', () => '/calendar'],
  ['Calendar, one month', (f) => `/calendar/${f.month}`],
  ['New workout', () => '/workouts/new'],
  ['Watch', () => '/devices'],
  ['Watch details', () => '/devices/details'],
  ['Profile', () => '/profile'],
  ['Profile, preferences section', () => '/profile?section=preferences'],
  ['Profile, about section', () => '/profile?section=about'],
  ['Password', () => '/profile/password'],
  ['Two-step login', () => '/profile/two-step'],
  ['Signed-in phones', () => '/profile/phones'],
  ['Timezone', () => '/profile/timezone'],
  ['Goals', () => '/profile/goals'],
  ['Send activities to Garmin', () => '/profile/garmin'],
  ['Old settings address', () => '/settings'],
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

test('the demo user is a normal user: the profile says so, and /admin is not found', async ({ page }) => {
  await page.goto('/');
  await settled(page);
  await openProfile(page);
  const head = page.getByRole('region', { name: 'Your account' });
  await expect(head).toContainText('demo');
  await expect(head).toContainText('Two-step login on');
  await expect(page.getByRole('link', { name: 'Admin area' })).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'Password' })).toBeVisible();
  // The Preferences group is a section of its own on a wide screen.
  await page.goto('/profile?section=preferences');
  await settled(page, 'Profile');
  await expect(page.getByRole('link', { name: /^Timezone/ })).toBeVisible();

  await page.goto('/admin');
  await settled(page, 'Page not found');
  expect((await page.request.get('/api/admin/users')).status()).toBe(403);
});

test('Signed-in phones: a phone that signed in is listed (no token), fits 320 px, and can be signed out', async ({ page, playwright }) => {
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
  await page.goto('/profile/phones');
  await settled(page);
  await expect(page.getByRole('heading', { level: 1, name: 'Signed-in phones' })).toBeVisible();
  const list = page.getByRole('main');
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

  await page.getByRole('button', { name: new RegExp(`^Sign out ${name}`) }).click();
  const confirm = page.getByRole('dialog', { name: `Sign out ${name}?` });
  await expect(confirm).toBeVisible();
  await expectAxeClean(page);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
  await confirm.getByRole('button', { name: 'Sign out' }).click();
  await expect(list.getByText(name)).toHaveCount(0);
});

test('Goals: changing the step goal is saved and shown on Movement', async ({ page }) => {
  await page.goto('/profile/goals');
  await settled(page);
  const field = page.getByLabel('Daily step goal');
  const before = await field.inputValue();
  await field.fill('9500');
  await page.getByRole('button', { name: 'Save goals' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
  await page.goto('/movement');
  await settled(page);
  await expect(page.getByText('Goal 9,500')).toBeVisible();
  await page.goto('/profile?section=preferences');
  await settled(page, 'Profile');
  await expect(page.getByRole('link', { name: /^Goals 9,500 steps/ })).toBeVisible();
  await page.goto('/profile/goals');
  await page.getByLabel('Daily step goal').fill(before);
  await page.getByRole('button', { name: 'Save goals' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
});

test('Timezone: a pick is saved at once', async ({ page }) => {
  // The seeded zone may be one the list does not offer (the server's own, such as UTC), so it is put back through the API.
  const original = ((await (await page.request.get('/api/settings')).json()) as { timezone: string }).timezone;
  try {
    await page.goto('/profile/timezone');
    await settled(page);
    const other = original === 'Europe/Paris' ? 'Europe/Berlin' : 'Europe/Paris';
    await page.getByLabel('Timezone').selectOption(other);
    await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
    await page.goto('/profile?section=preferences');
    await settled(page, 'Profile');
    await expect(page.getByRole('link', { name: new RegExp(`^Timezone ${other}`) })).toBeVisible();
  } finally {
    const back = await page.request.patch('/api/settings', { headers: { 'X-Nimrag': '1' }, data: { timezone: original } });
    expect(back.ok()).toBe(true);
  }
});

test('Watch shows the seeded server and watch data, and the server version shows under About', async ({ page }) => {
  await page.goto('/devices');
  await settled(page, 'Watch');
  const card = page.getByRole('region', { name: 'Watch status' });
  await expect(card.getByRole('heading')).toBeVisible();
  await expect(card.getByText('Battery')).toBeVisible();
  await expect(page.getByText('Reachable')).toBeVisible();
  await expect(page.getByRole('link', { name: /^Phones/ })).toHaveAttribute('href', '/profile/phones');
  await page.getByRole('link', { name: /^Details/ }).click();
  await settled(page, 'Details');
  await expect(page.getByText(/Original files are kept as the watch sent them/)).toBeVisible();

  await page.goto('/profile?section=about');
  await settled(page, 'Profile');
  await expect(page.getByText(/Server \d+\.\d+\.\d+ · AGPL-3\.0/)).toBeVisible();
});
