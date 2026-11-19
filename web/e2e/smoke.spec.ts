import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

// Smoke test against `vite preview` of the production build. The real-server smoke test
// (against the embedded binary with dev-seed data) comes later.

/** Fails the test on page errors, console errors, and any request to another origin. */
function watch(page: Page, baseURL: string) {
  const problems: string[] = [];
  const origin = new URL(baseURL).origin;
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    // A server without a tile file answers the map probe with 404: expected, shows the placeholder.
    if (m.type() === 'error' && !m.location().url.includes('/tiles/')) problems.push(`console: ${m.text()}`);
  });
  page.on('request', (r) => {
    const url = r.url();
    if (url.startsWith('data:') || url.startsWith('blob:')) return;
    if (new URL(url).origin !== origin) problems.push(`third-party request: ${url}`);
  });
  return problems;
}

test('gallery renders every component and makes no third-party request', async ({ page, baseURL }) => {
  const problems = watch(page, baseURL as string);
  await page.goto('/__gallery');
  await expect(page.getByRole('heading', { level: 1, name: 'Component gallery' })).toBeVisible();

  // A sample of components in their states.
  await expect(page.getByRole('group', { name: 'Time range' })).toBeVisible();
  await expect(page.getByRole('group', { name: 'Filter by type' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'Energy level 68 out of 100' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'Overnight HRV over 4 weeks' })).toBeVisible();
  await expect(page.getByText('unavailable').first()).toBeVisible();
  await expect(page.locator('.uplot canvas').first()).toBeVisible();
  await expect(page.getByText('Map placeholder. Tiles served from your own server.')).toBeVisible();

  // Fonts are self-hosted and actually load.
  const fonts = await page.evaluate(async () => {
    await document.fonts.ready;
    return Array.from(document.fonts)
      .filter((f) => f.status === 'loaded')
      .map((f) => f.family.replace(/["']/g, ''));
  });
  expect(fonts).toEqual(expect.arrayContaining(['Inter', 'Space Grotesk']));

  expect(problems).toEqual([]);
});

test('gallery has no serious accessibility violations (incl. 4.5:1 contrast)', async ({ page }) => {
  await page.goto('/__gallery');
  await expect(page.locator('.uplot canvas').first()).toBeVisible();
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  const summary = results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).slice(0, 3).join(' | ')}`);
  expect(summary).toEqual([]);
});

test('login page logs in via POST /api/login then GET /api/me', async ({ page, baseURL }) => {
  const problems = watch(page, baseURL as string);
  const calls: string[] = [];
  let loginHeaders: Record<string, string> = {};
  let loginBody = '';
  await page.route('**/api/login', async (route) => {
    const req = route.request();
    calls.push(`${req.method()} /api/login`);
    loginHeaders = req.headers();
    loginBody = req.postData() ?? '';
    await route.fulfill({ status: 204 });
  });
  await page.route('**/api/me', async (route) => {
    calls.push(`${route.request().method()} /api/me`);
    await route.fulfill({ status: 200, contentType: 'application/json', body: '{"user":"owner"}' });
  });

  await page.goto('/login');
  await expect(page.getByRole('heading', { name: 'Nimrag Connect' })).toBeVisible();
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(results.violations.map((v) => v.id)).toEqual([]);

  const submit = page.getByRole('button', { name: 'Log in' });
  await expect(submit).toBeDisabled();
  await page.getByLabel('Password').fill('correct horse');
  await submit.click();

  await expect(page).toHaveURL(/\/$/);
  expect(calls).toEqual(['POST /api/login', 'GET /api/me']);
  expect(loginHeaders['x-nimrag']).toBe('1');
  expect(JSON.parse(loginBody)).toEqual({ password: 'correct horse' });
  expect(problems.filter((p) => !p.includes('/api/'))).toEqual([]);
});

test('login shows an error for a wrong password and stays on the page', async ({ page }) => {
  await page.route('**/api/login', (route) => route.fulfill({ status: 401, contentType: 'application/json', body: '{"error":"invalid"}' }));
  await page.goto('/login');
  await page.getByLabel('Password').fill('nope');
  await page.getByRole('button', { name: 'Log in' }).click();
  await expect(page.getByRole('alert')).toContainText('Wrong password.');
  await expect(page).toHaveURL(/\/login/);
});

test('every route renders without errors', async ({ page, baseURL }) => {
  const problems = watch(page, baseURL as string);
  const paths = [
    '/',
    '/activities',
    '/activities/1',
    '/activities/1/strength',
    '/sleep',
    '/sleep/2026-11-18',
    '/day',
    '/day/2026-11-18',
    '/health',
    '/movement',
    '/training',
    '/body',
    '/calendar',
    '/calendar/2026-11',
    '/workouts/new',
    '/devices',
    '/settings',
    '/does-not-exist',
  ];
  for (const path of paths) {
    await page.goto(path);
    await expect(page.getByRole('heading', { level: 1 }).first(), path).toBeVisible();
  }
  expect(problems).toEqual([]);
});
